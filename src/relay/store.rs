// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Thread-safe in-memory state store for Cloud Cove rooms, invites, messages,
//! Tier-0 governance gates, and Marlin Gateway governance envelopes.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::bridges::InboundBridgeMessage;
use crate::protocol::{
    BridgeOrigin, ChannelId, GateApprovalReceipt, GateDecision, GateId, GateResolutionInput,
    MessageAttachment, MessageId, PresenceStatus, PrincipalId, PrincipalKind, PrincipalRole,
    ProtocolError, RoomChannel, RoomEvent, RoomEventKind, RoomGate, RoomGovernanceEnvelope, RoomId,
    RoomInvite, RoomMessage, RoomParticipant, RoomSnapshot, ScopeId, extract_mentions,
};

const EVENT_BUS_CAPACITY: usize = 256;

/// Errors returned by [`CoveRelayStore`] mutations and queries.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RelayError {
    /// Target room does not exist.
    #[error("room `{0}` not found")]
    RoomNotFound(RoomId),
    /// Target channel does not exist in the room.
    #[error("channel `{channel_id}` not found in room `{room_id}`")]
    ChannelNotFound {
        /// Room identifier.
        room_id: RoomId,
        /// Missing channel identifier.
        channel_id: ChannelId,
    },
    /// Principal is not a member of the room.
    #[error("principal `{principal_id}` is not a member of room `{room_id}`")]
    ParticipantNotFound {
        /// Room identifier.
        room_id: RoomId,
        /// Missing principal identifier.
        principal_id: PrincipalId,
    },
    /// Invite token was not found or exhausted.
    #[error("invite token `{0}` is invalid or exhausted")]
    InvalidInvite(InviteTokenError),
    /// Gate was not found in the target room.
    #[error("gate `{gate_id}` not found in room `{room_id}`")]
    GateNotFound {
        /// Room identifier.
        room_id: RoomId,
        /// Missing gate identifier.
        gate_id: GateId,
    },
    /// Bot reply chain exceeded configured maximum depth (anti-loop guard).
    #[error("bot chain depth {depth} exceeds max allowed {max_depth}")]
    BotLoopLimitExceeded {
        /// Incoming chain depth.
        depth: u8,
        /// Configured ceiling.
        max_depth: u8,
    },
    /// Room already exists.
    #[error("room `{0}` already exists")]
    RoomAlreadyExists(RoomId),
    /// Protocol invariant or identifier validation failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// Internal lock error.
    #[error("relay state lock poisoned")]
    LockPoisoned,
}

/// Invite token display wrapper for [`RelayError::InvalidInvite`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InviteTokenError(pub String);

impl std::fmt::Display for InviteTokenError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Compact summary for room listing endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomSummary {
    /// Room identifier.
    pub room_id: RoomId,
    /// Room display title.
    pub title: String,
    /// Number of channels.
    pub channel_count: usize,
    /// Number of joined participants (humans + agents).
    pub participant_count: usize,
    /// Number of timeline messages.
    pub message_count: usize,
    /// Number of pending Tier-0 governance gates.
    pub pending_gates: usize,
}

/// Request to create a new Cove room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateRoomRequest {
    /// Optional explicit room id; generated when omitted.
    #[serde(default)]
    pub room_id: Option<RoomId>,
    /// Human-readable room title.
    pub title: String,
    /// Owning human principal id.
    pub owner_id: PrincipalId,
    /// Owning human display name.
    pub owner_name: String,
    /// Owning human handle.
    pub owner_handle: String,
}

/// Request to issue a scoped invite token for a room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateInviteRequest {
    /// Issuing principal id (must belong to room).
    pub issued_by: PrincipalId,
    /// Role granted to redeeming principal.
    pub grant_role: PrincipalRole,
    /// Governance scopes granted upon join.
    #[serde(default)]
    pub grant_scopes: Vec<ScopeId>,
    /// Maximum redemptions allowed (defaults to 1 when 0).
    #[serde(default = "default_max_uses")]
    pub max_uses: u32,
}

const fn default_max_uses() -> u32 {
    1
}

/// Request to redeem an invite token and join a room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedeemInviteRequest {
    /// Joining principal identifier.
    pub principal_id: PrincipalId,
    /// Display name.
    pub display_name: String,
    /// `@handle`.
    pub handle: String,
    /// Human vs AI agent kind.
    pub kind: PrincipalKind,
}

/// Request to post a message to a room channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PostMessageRequest {
    /// Target channel id.
    pub channel_id: ChannelId,
    /// Optional thread root message id.
    #[serde(default)]
    pub thread_root_id: Option<MessageId>,
    /// Author principal id.
    pub author_id: PrincipalId,
    /// Message text body.
    pub body: String,
    /// Optional linked Tier-0 gate id.
    #[serde(default)]
    pub gate_id: Option<GateId>,
    /// Structured attachments.
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
    /// Source transport origin.
    #[serde(default)]
    pub origin: BridgeOrigin,
    /// Bot chain depth (0 for human messages).
    #[serde(default)]
    pub bot_chain_depth: u8,
}

/// Request to open an inline Tier-0 `PromptGate` inside a channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestGateRequest {
    /// Target channel id.
    pub channel_id: ChannelId,
    /// Principal requesting execution.
    pub requested_by: PrincipalId,
    /// Summary shown in inline gate embed.
    pub summary: String,
    /// Target capability or tool name.
    pub capability: String,
}

/// Request to resolve a Tier-0 `PromptGate` (human-only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveGateRequest {
    /// Acting principal id (must be a `PrincipalKind::Human` room member).
    pub actor_id: PrincipalId,
    /// `true` to approve, `false` to deny.
    pub approved: bool,
    /// Rationale recorded on the receipt.
    #[serde(default)]
    pub reason: String,
}

struct RoomRecord {
    id: RoomId,
    title: String,
    latest_seq: u64,
    channels: Vec<RoomChannel>,
    participants: BTreeMap<PrincipalId, RoomParticipant>,
    messages: Vec<RoomMessage>,
    gates: BTreeMap<GateId, RoomGate>,
    events_tx: broadcast::Sender<RoomEvent>,
}

impl RoomRecord {
    fn emit(&mut self, kind: RoomEventKind) -> RoomEvent {
        self.latest_seq = self.latest_seq.saturating_add(1);
        let event = RoomEvent {
            seq: self.latest_seq,
            room_id: self.id.clone(),
            kind,
        };
        let _ = self.events_tx.send(event.clone());
        event
    }

    fn snapshot(&self) -> RoomSnapshot {
        RoomSnapshot {
            room_id: self.id.clone(),
            title: self.title.clone(),
            latest_seq: self.latest_seq,
            channels: self.channels.clone(),
            participants: self.participants.values().cloned().collect(),
            messages: self.messages.clone(),
            gates: self.gates.values().cloned().collect(),
        }
    }

    fn summary(&self) -> RoomSummary {
        let pending_gates = self
            .gates
            .values()
            .filter(|gate| gate.decision == GateDecision::Pending)
            .count();
        RoomSummary {
            room_id: self.id.clone(),
            title: self.title.clone(),
            channel_count: self.channels.len(),
            participant_count: self.participants.len(),
            message_count: self.messages.len(),
            pending_gates,
        }
    }
}

struct StoreInner {
    rooms: BTreeMap<RoomId, RoomRecord>,
    invites: BTreeMap<String, RoomInvite>,
}

/// Thread-safe handle to the Cloud Cove Relay room store.
#[derive(Clone)]
pub struct CoveRelayStore {
    inner: Arc<RwLock<StoreInner>>,
    max_bot_chain_depth: u8,
    default_guest_scope: ScopeId,
}

#[allow(clippy::significant_drop_tightening)]
impl CoveRelayStore {
    /// Construct a new store pre-seeded with the default `cove-main` workspace room.
    #[must_use]
    pub fn with_default_room(max_bot_chain_depth: u8, default_guest_scope: &str) -> Self {
        let guest_scope = ScopeId::new(default_guest_scope)
            .unwrap_or_else(|_| ScopeId::from_trusted("public".to_string()));
        let store = Self {
            inner: Arc::new(RwLock::new(StoreInner {
                rooms: BTreeMap::new(),
                invites: BTreeMap::new(),
            })),
            max_bot_chain_depth,
            default_guest_scope: guest_scope,
        };
        let _ = store.seed_default_cove_room();
        store
    }

    fn seed_default_cove_room(&self) -> Result<(), RelayError> {
        let room_id = RoomId::new("cove-main")?;
        let owner_id = PrincipalId::new("human:owner")?;
        let core_scope = ScopeId::new("org:core")?;
        let arch_scope = ScopeId::new("room:arch")?;
        let public_scope = self.default_guest_scope.clone();

        let (events_tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let mut participants = BTreeMap::new();
        participants.insert(
            owner_id.clone(),
            RoomParticipant {
                id: owner_id,
                display_name: "Workspace Owner".to_string(),
                handle: "owner".to_string(),
                kind: PrincipalKind::Human,
                role: PrincipalRole::Owner,
                scopes: vec![core_scope, arch_scope.clone(), public_scope.clone()],
                presence: PresenceStatus::Online,
            },
        );
        for (id, name, handle) in [
            ("agent:architect", "Architect", "architect"),
            ("agent:reviewer", "Reviewer", "reviewer"),
            ("agent:security", "Security", "security"),
        ] {
            let pid = PrincipalId::new(id)?;
            participants.insert(
                pid.clone(),
                RoomParticipant {
                    id: pid,
                    display_name: name.to_string(),
                    handle: handle.to_string(),
                    kind: PrincipalKind::Agent,
                    role: PrincipalRole::RoleAgent,
                    scopes: vec![arch_scope.clone(), public_scope.clone()],
                    presence: PresenceStatus::Online,
                },
            );
        }

        let channels = vec![
            RoomChannel {
                id: ChannelId::new("general")?,
                name: "general".to_string(),
                topic: "Shared Marlin Cove collaboration channel".to_string(),
                is_direct: false,
            },
            RoomChannel {
                id: ChannelId::new("architecture")?,
                name: "architecture".to_string(),
                topic: "Architecture decisions & Marlin Gateway coordination".to_string(),
                is_direct: false,
            },
            RoomChannel {
                id: ChannelId::new("security")?,
                name: "security".to_string(),
                topic: "Tier-0 PromptGate approvals & room governance".to_string(),
                is_direct: false,
            },
        ];

        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        guard.rooms.insert(
            room_id.clone(),
            RoomRecord {
                id: room_id,
                title: "Marlin Cove Main".to_string(),
                latest_seq: 0,
                channels,
                participants,
                messages: Vec::new(),
                gates: BTreeMap::new(),
                events_tx,
            },
        );
        Ok(())
    }

    /// List all rooms in deterministic order.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::LockPoisoned`] if internal synchronization fails.
    pub fn list_rooms(&self) -> Result<Vec<RoomSummary>, RelayError> {
        let guard = self.inner.read().map_err(|_| RelayError::LockPoisoned)?;
        Ok(guard.rooms.values().map(RoomRecord::summary).collect())
    }

    /// Create a new Cove room with default `#general`, `#architecture`, `#security` channels.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::RoomAlreadyExists`] if the room id is already taken.
    pub fn create_room(&self, request: CreateRoomRequest) -> Result<RoomSnapshot, RelayError> {
        let room_id = match request.room_id {
            Some(id) => id,
            None => RoomId::new(format!("room-{}", Uuid::new_v4().simple()))?,
        };
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        if guard.rooms.contains_key(&room_id) {
            return Err(RelayError::RoomAlreadyExists(room_id));
        }

        let (events_tx, _) = broadcast::channel(EVENT_BUS_CAPACITY);
        let mut participants = BTreeMap::new();
        participants.insert(
            request.owner_id.clone(),
            RoomParticipant {
                id: request.owner_id,
                display_name: request.owner_name,
                handle: request.owner_handle.trim_start_matches('@').to_lowercase(),
                kind: PrincipalKind::Human,
                role: PrincipalRole::Owner,
                scopes: vec![ScopeId::new("org:core")?, self.default_guest_scope.clone()],
                presence: PresenceStatus::Online,
            },
        );

        let channels = vec![
            RoomChannel {
                id: ChannelId::new("general")?,
                name: "general".to_string(),
                topic: "General collaboration".to_string(),
                is_direct: false,
            },
            RoomChannel {
                id: ChannelId::new("architecture")?,
                name: "architecture".to_string(),
                topic: "Design & architecture".to_string(),
                is_direct: false,
            },
            RoomChannel {
                id: ChannelId::new("security")?,
                name: "security".to_string(),
                topic: "Governance gates".to_string(),
                is_direct: false,
            },
        ];

        let record = RoomRecord {
            id: room_id.clone(),
            title: request.title,
            latest_seq: 0,
            channels,
            participants,
            messages: Vec::new(),
            gates: BTreeMap::new(),
            events_tx,
        };
        let snapshot = record.snapshot();
        guard.rooms.insert(room_id, record);
        Ok(snapshot)
    }

    /// Return the full snapshot for a room.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::RoomNotFound`] if the room does not exist.
    pub fn room_snapshot(&self, room_id: &RoomId) -> Result<RoomSnapshot, RelayError> {
        let guard = self.inner.read().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        Ok(room.snapshot())
    }

    /// Build a verified [`RoomGovernanceEnvelope`] for a participant in a room,
    /// ready to be handed off to **Marlin Gateway** (`marlin-agent-core`).
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] if the room or participant does not exist.
    pub fn governance_envelope(
        &self,
        room_id: &RoomId,
        channel_id: ChannelId,
        principal_id: &PrincipalId,
        origin: BridgeOrigin,
    ) -> Result<RoomGovernanceEnvelope, RelayError> {
        let guard = self.inner.read().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        let participant =
            room.participants
                .get(principal_id)
                .ok_or_else(|| RelayError::ParticipantNotFound {
                    room_id: room_id.clone(),
                    principal_id: principal_id.clone(),
                })?;
        Ok(participant.governance_envelope(room_id.clone(), channel_id, origin))
    }

    /// Subscribe to real-time [`RoomEvent`] broadcasts for a room.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::RoomNotFound`] if the room does not exist.
    pub fn subscribe_room_events(
        &self,
        room_id: &RoomId,
    ) -> Result<broadcast::Receiver<RoomEvent>, RelayError> {
        let guard = self.inner.read().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        Ok(room.events_tx.subscribe())
    }

    /// Issue a scoped invite token for a room.
    ///
    /// # Errors
    ///
    /// Returns an error if the room or issuing principal does not exist.
    pub fn create_invite(
        &self,
        room_id: &RoomId,
        request: CreateInviteRequest,
    ) -> Result<RoomInvite, RelayError> {
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        if !room.participants.contains_key(&request.issued_by) {
            return Err(RelayError::ParticipantNotFound {
                room_id: room_id.clone(),
                principal_id: request.issued_by,
            });
        }

        let token = crate::protocol::InviteToken::new(format!("inv_{}", Uuid::new_v4().simple()))?;
        let scopes = if request.grant_scopes.is_empty() {
            vec![self.default_guest_scope.clone()]
        } else {
            request.grant_scopes
        };
        let invite = RoomInvite {
            token: token.clone(),
            room_id: room_id.clone(),
            issued_by: request.issued_by,
            grant_role: request.grant_role,
            grant_scopes: scopes,
            max_uses: request.max_uses.max(1),
            uses: 0,
        };
        guard
            .invites
            .insert(token.as_str().to_string(), invite.clone());
        Ok(invite)
    }

    /// Redeem an invite token and admit the human or AI agent into the room.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::InvalidInvite`] when the token is unknown or exhausted.
    pub fn redeem_invite(
        &self,
        token: &str,
        request: RedeemInviteRequest,
    ) -> Result<RoomParticipant, RelayError> {
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let invite = guard
            .invites
            .get_mut(token)
            .ok_or_else(|| RelayError::InvalidInvite(InviteTokenError(token.to_string())))?;
        if invite.uses >= invite.max_uses {
            return Err(RelayError::InvalidInvite(InviteTokenError(
                token.to_string(),
            )));
        }
        invite.uses = invite.uses.saturating_add(1);
        let room_id = invite.room_id.clone();
        let grant_role = invite.grant_role;
        let grant_scopes = invite.grant_scopes.clone();

        let room = guard
            .rooms
            .get_mut(&room_id)
            .ok_or(RelayError::RoomNotFound(room_id))?;
        let participant = RoomParticipant {
            id: request.principal_id.clone(),
            display_name: request.display_name,
            handle: request.handle.trim_start_matches('@').to_lowercase(),
            kind: request.kind,
            role: grant_role,
            scopes: grant_scopes,
            presence: PresenceStatus::Online,
        };
        room.participants
            .insert(participant.id.clone(), participant.clone());
        let _ = room.emit(RoomEventKind::ParticipantJoined {
            participant: participant.clone(),
        });
        Ok(participant)
    }

    /// Post a message into a Cove room channel, enforcing membership and bot-loop guards.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] when the room/channel/author is invalid or bot depth exceeds limit.
    pub fn post_message(
        &self,
        room_id: &RoomId,
        request: PostMessageRequest,
    ) -> Result<RoomMessage, RelayError> {
        if request.bot_chain_depth > self.max_bot_chain_depth {
            return Err(RelayError::BotLoopLimitExceeded {
                depth: request.bot_chain_depth,
                max_depth: self.max_bot_chain_depth,
            });
        }

        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        if !room
            .channels
            .iter()
            .any(|channel| channel.id == request.channel_id)
        {
            return Err(RelayError::ChannelNotFound {
                room_id: room_id.clone(),
                channel_id: request.channel_id,
            });
        }
        let author_kind = room
            .participants
            .get(&request.author_id)
            .ok_or_else(|| RelayError::ParticipantNotFound {
                room_id: room_id.clone(),
                principal_id: request.author_id.clone(),
            })?
            .kind;

        let message = RoomMessage {
            id: MessageId::new(format!("msg_{}", Uuid::new_v4().simple()))?,
            room_id: room_id.clone(),
            channel_id: request.channel_id,
            thread_root_id: request.thread_root_id,
            author_id: request.author_id,
            author_kind,
            mentions: extract_mentions(&request.body),
            body: request.body,
            gate_id: request.gate_id,
            attachments: request.attachments,
            origin: request.origin,
            bot_chain_depth: request.bot_chain_depth,
            created_at_unix: now_unix_secs(),
        };

        room.messages.push(message.clone());
        let _ = room.emit(RoomEventKind::MessagePosted {
            message: message.clone(),
        });
        Ok(message)
    }

    /// Ingest a normalized Telegram or Discord bridge message into a Cove room.
    ///
    /// Bridged external senders not yet in the room are admitted with
    /// [`PrincipalRole::ExternalGuest`] and the configured guest scope only.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] if the target room or channel does not exist.
    pub fn ingest_bridge_message(
        &self,
        inbound: InboundBridgeMessage,
    ) -> Result<RoomMessage, RelayError> {
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get_mut(&inbound.target_room_id)
            .ok_or_else(|| RelayError::RoomNotFound(inbound.target_room_id.clone()))?;
        if !room
            .channels
            .iter()
            .any(|channel| channel.id == inbound.target_channel_id)
        {
            return Err(RelayError::ChannelNotFound {
                room_id: inbound.target_room_id.clone(),
                channel_id: inbound.target_channel_id,
            });
        }

        if !room.participants.contains_key(&inbound.sender_id) {
            let participant = RoomParticipant {
                id: inbound.sender_id.clone(),
                display_name: inbound.sender_name.clone(),
                handle: inbound
                    .sender_name
                    .trim_start_matches('@')
                    .to_ascii_lowercase(),
                kind: PrincipalKind::Human,
                role: PrincipalRole::ExternalGuest,
                scopes: vec![self.default_guest_scope.clone()],
                presence: PresenceStatus::Online,
            };
            room.participants
                .insert(participant.id.clone(), participant.clone());
            let _ = room.emit(RoomEventKind::ParticipantJoined { participant });
        }

        let message = RoomMessage {
            id: MessageId::new(format!("msg_{}", Uuid::new_v4().simple()))?,
            room_id: inbound.target_room_id,
            channel_id: inbound.target_channel_id,
            thread_root_id: None,
            author_id: inbound.sender_id,
            author_kind: PrincipalKind::Human,
            mentions: extract_mentions(&inbound.content),
            body: inbound.content,
            gate_id: None,
            attachments: inbound.attachments,
            origin: inbound.origin,
            bot_chain_depth: 0,
            created_at_unix: inbound.timestamp_unix,
        };
        room.messages.push(message.clone());
        let _ = room.emit(RoomEventKind::MessagePosted {
            message: message.clone(),
        });
        Ok(message)
    }

    /// Open a Tier-0 `PromptGate` inside a Cove room channel.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] if the room, channel, or requester is missing.
    pub fn request_gate(
        &self,
        room_id: &RoomId,
        request: RequestGateRequest,
    ) -> Result<RoomGate, RelayError> {
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        if !room
            .channels
            .iter()
            .any(|channel| channel.id == request.channel_id)
        {
            return Err(RelayError::ChannelNotFound {
                room_id: room_id.clone(),
                channel_id: request.channel_id,
            });
        }
        if !room.participants.contains_key(&request.requested_by) {
            return Err(RelayError::ParticipantNotFound {
                room_id: room_id.clone(),
                principal_id: request.requested_by,
            });
        }

        let gate = RoomGate {
            id: GateId::new(format!("gate_{}", Uuid::new_v4().simple()))?,
            room_id: room_id.clone(),
            channel_id: request.channel_id,
            requested_by: request.requested_by,
            summary: request.summary,
            capability: request.capability,
            decision: GateDecision::Pending,
            receipt: None,
        };
        room.gates.insert(gate.id.clone(), gate.clone());
        let _ = room.emit(RoomEventKind::GateRequested { gate: gate.clone() });
        Ok(gate)
    }

    /// Resolve a Tier-0 `PromptGate`, strictly enforcing the Human-Only invariant.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::Protocol`] with [`ProtocolError::HumanApprovalRequired`]
    /// if an AI agent attempts to resolve the gate.
    pub fn resolve_gate(
        &self,
        room_id: &RoomId,
        gate_id: &GateId,
        request: ResolveGateRequest,
    ) -> Result<GateApprovalReceipt, RelayError> {
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        let actor_kind = room
            .participants
            .get(&request.actor_id)
            .ok_or_else(|| RelayError::ParticipantNotFound {
                room_id: room_id.clone(),
                principal_id: request.actor_id.clone(),
            })?
            .kind;

        let receipt = GateApprovalReceipt::resolve_by_human(GateResolutionInput {
            gate_id: gate_id.clone(),
            actor_id: request.actor_id,
            actor_kind,
            approved: request.approved,
            reason: request.reason,
            resolved_at_unix: now_unix_secs(),
        })?;

        let gate = room
            .gates
            .get_mut(gate_id)
            .ok_or_else(|| RelayError::GateNotFound {
                room_id: room_id.clone(),
                gate_id: gate_id.clone(),
            })?;
        gate.decision = receipt.decision;
        gate.receipt = Some(receipt.clone());

        let _ = room.emit(RoomEventKind::GateResolved {
            receipt: receipt.clone(),
        });
        Ok(receipt)
    }
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
