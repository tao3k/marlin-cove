// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Formally verified thread-safe state store for Marlin Cove rooms, invites,
//! messages, Tier-0 gates, replay deduplication, and bounded capacity limits.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::sync::broadcast;
use uuid::Uuid;

use crate::bridges::InboundBridgeMessage;
use crate::config::RelayCapacityLimits;
use crate::protocol::{
    BridgeOrigin, ChannelId, GateApprovalReceipt, GateDecision, GateId, GateResolutionInput,
    InviteToken, MessageId, PresenceStatus, PrincipalId, PrincipalKind, PrincipalRole, RoomChannel,
    RoomEvent, RoomEventKind, RoomGate, RoomGovernanceEnvelope, RoomId, RoomInvite, RoomMessage,
    RoomParticipant, RoomSnapshot, ScopeId, extract_mentions,
};
use crate::relay::requests::{
    CreateInviteRequest, CreateRoomRequest, InviteTokenError, PostMessageRequest,
    RedeemInviteRequest, RelayError, RequestGateRequest, ResolveGateRequest, RoomSummary,
};

struct RoomRecord {
    id: RoomId,
    title: String,
    latest_seq: u64,
    channels: Vec<RoomChannel>,
    participants: BTreeMap<PrincipalId, RoomParticipant>,
    messages: Vec<RoomMessage>,
    gates: BTreeMap<GateId, RoomGate>,
    seen_bridge_ids: BTreeSet<String>,
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
    limits: RelayCapacityLimits,
}

#[allow(clippy::significant_drop_tightening)]
impl CoveRelayStore {
    /// Construct a new store pre-seeded from [`crate::config::CoveConfig`] defaults.
    #[must_use]
    pub fn from_config(config: &crate::config::CoveConfig) -> Self {
        let guest_scope = ScopeId::new(&config.governance.default_guest_scope)
            .unwrap_or_else(|_| ScopeId::from_trusted("public".to_string()));
        let store = Self {
            inner: Arc::new(RwLock::new(StoreInner {
                rooms: BTreeMap::new(),
                invites: BTreeMap::new(),
            })),
            max_bot_chain_depth: config.relay.max_bot_chain_depth,
            default_guest_scope: guest_scope,
            limits: config.relay.limits,
        };
        let _ = store.seed_default_cove_room(
            config.relay.default_room_id.clone(),
            config.relay.default_room_title.clone(),
            config.relay.default_channel_id.clone(),
        );
        store
    }

    fn seed_default_cove_room(
        &self,
        room_id: RoomId,
        room_title: String,
        default_channel_id: ChannelId,
    ) -> Result<(), RelayError> {
        let owner_id = PrincipalId::new("human:owner")?;
        let core_scope = ScopeId::new("org:core")?;
        let arch_scope = ScopeId::new("room:arch")?;
        let public_scope = self.default_guest_scope.clone();

        let (events_tx, _) = broadcast::channel(self.limits.event_bus_capacity.max(1));
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

        let channels = default_channels(default_channel_id)?;
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        guard.rooms.insert(
            room_id.clone(),
            RoomRecord {
                id: room_id,
                title: room_title,
                latest_seq: 0,
                channels,
                participants,
                messages: Vec::new(),
                gates: BTreeMap::new(),
                seen_bridge_ids: BTreeSet::new(),
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

    /// Create a new Cove room enforcing bounded room capacity.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::CapacityExceeded`] or [`RelayError::RoomAlreadyExists`].
    pub fn create_room(&self, request: CreateRoomRequest) -> Result<RoomSnapshot, RelayError> {
        let room_id = match request.room_id {
            Some(id) => id,
            None => RoomId::new(format!("room-{}", Uuid::new_v4().simple()))?,
        };
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        if guard.rooms.len() >= self.limits.max_rooms {
            return Err(RelayError::CapacityExceeded {
                resource: "rooms",
                limit: self.limits.max_rooms,
            });
        }
        if guard.rooms.contains_key(&room_id) {
            return Err(RelayError::RoomAlreadyExists(room_id));
        }

        let (events_tx, _) = broadcast::channel(self.limits.event_bus_capacity.max(1));
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

        let record = RoomRecord {
            id: room_id.clone(),
            title: request.title,
            latest_seq: 0,
            channels: default_channels(ChannelId::from_trusted("general".to_string()))?,
            participants,
            messages: Vec::new(),
            gates: BTreeMap::new(),
            seen_bridge_ids: BTreeSet::new(),
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

    /// Look up an AI role agent's [`PrincipalId`] by `@handle` inside a room.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] if the room or matching agent is missing.
    pub fn find_agent_by_handle(
        &self,
        room_id: &RoomId,
        handle: &str,
    ) -> Result<PrincipalId, RelayError> {
        let normalized = handle.trim().trim_start_matches('@').to_ascii_lowercase();
        let guard = self.inner.read().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        room.participants
            .values()
            .find(|p| p.kind == PrincipalKind::Agent && p.handle == normalized)
            .map(|p| p.id.clone())
            .ok_or_else(|| RelayError::ParticipantNotFound {
                room_id: room_id.clone(),
                principal_id: PrincipalId::from_trusted(format!("agent:{normalized}")),
            })
    }

    /// Build a verified [`RoomGovernanceEnvelope`] for Marlin Gateway (`marlin-agent-core`).
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

    /// Issue a scoped invite token enforcing `InvInviteAuthorizationAndNoEscalation`.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::UnauthorizedAction`], [`RelayError::PrivilegeEscalation`],
    /// or [`RelayError::CapacityExceeded`].
    pub fn create_invite(
        &self,
        room_id: &RoomId,
        request: CreateInviteRequest,
    ) -> Result<RoomInvite, RelayError> {
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        if guard.invites.len() >= self.limits.max_invites {
            return Err(RelayError::CapacityExceeded {
                resource: "invites",
                limit: self.limits.max_invites,
            });
        }
        let room = guard
            .rooms
            .get(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        let issuer = room.participants.get(&request.issued_by).ok_or_else(|| {
            RelayError::ParticipantNotFound {
                room_id: room_id.clone(),
                principal_id: request.issued_by.clone(),
            }
        })?;
        if !is_privileged_human(issuer) {
            return Err(RelayError::UnauthorizedAction {
                action: "create_invite",
                actor_id: request.issued_by,
            });
        }
        if request.grant_role == PrincipalRole::Owner {
            return Err(RelayError::PrivilegeEscalation {
                reason: "owner role cannot be granted via invite",
            });
        }

        let scopes = if request.grant_scopes.is_empty() {
            vec![self.default_guest_scope.clone()]
        } else {
            request.grant_scopes
        };
        if request.grant_role == PrincipalRole::ExternalGuest
            && scopes
                .iter()
                .any(|scope| scope != &self.default_guest_scope)
        {
            return Err(RelayError::PrivilegeEscalation {
                reason: "external_guest invite restricted to default guest scope",
            });
        }
        if scopes.iter().any(|scope| !issuer.scopes.contains(scope)) {
            return Err(RelayError::PrivilegeEscalation {
                reason: "issuer cannot grant scopes outside own scope set",
            });
        }

        let token = InviteToken::new(format!("inv_{}", Uuid::new_v4().simple()))?;
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

    /// Redeem an invite token without allowing overwrite of existing participants.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::InvalidInvite`], [`RelayError::ParticipantAlreadyExists`],
    /// or [`RelayError::PrincipalKindRoleMismatch`].
    pub fn redeem_invite(
        &self,
        token: &str,
        request: RedeemInviteRequest,
    ) -> Result<RoomParticipant, RelayError> {
        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let (room_id, grant_role, grant_scopes) = {
            let invite = guard
                .invites
                .get(token)
                .ok_or_else(|| RelayError::InvalidInvite(InviteTokenError(token.to_string())))?;
            if invite.uses >= invite.max_uses {
                return Err(RelayError::InvalidInvite(InviteTokenError(
                    token.to_string(),
                )));
            }
            let expected_agent = invite.grant_role == PrincipalRole::RoleAgent;
            let actual_agent = request.kind == PrincipalKind::Agent;
            if expected_agent != actual_agent {
                return Err(RelayError::PrincipalKindRoleMismatch);
            }
            (
                invite.room_id.clone(),
                invite.grant_role,
                invite.grant_scopes.clone(),
            )
        };

        let room = guard
            .rooms
            .get(&room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        if room.participants.contains_key(&request.principal_id) {
            return Err(RelayError::ParticipantAlreadyExists {
                room_id,
                principal_id: request.principal_id,
            });
        }

        if let Some(invite) = guard.invites.get_mut(token) {
            invite.uses = invite.uses.saturating_add(1);
        }
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

    /// Post a message into a Cove room enforcing bot-loop, byte-size, and capacity bounds.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] on validation, membership, loop, or capacity violation.
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
        if request.body.len() > self.limits.max_message_bytes {
            return Err(RelayError::MessageTooLarge {
                actual: request.body.len(),
                limit: self.limits.max_message_bytes,
            });
        }

        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get_mut(room_id)
            .ok_or_else(|| RelayError::RoomNotFound(room_id.clone()))?;
        if room.messages.len() >= self.limits.max_messages_per_room {
            return Err(RelayError::CapacityExceeded {
                resource: "messages",
                limit: self.limits.max_messages_per_room,
            });
        }
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

    /// Ingest a normalized Telegram or Discord bridge message with replay deduplication.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::DuplicateBridgeMessage`], [`RelayError::CapacityExceeded`],
    /// or [`RelayError::MessageTooLarge`].
    pub fn ingest_bridge_message(
        &self,
        inbound: InboundBridgeMessage,
    ) -> Result<RoomMessage, RelayError> {
        if inbound.content.len() > self.limits.max_message_bytes {
            return Err(RelayError::MessageTooLarge {
                actual: inbound.content.len(),
                limit: self.limits.max_message_bytes,
            });
        }

        let mut guard = self.inner.write().map_err(|_| RelayError::LockPoisoned)?;
        let room = guard
            .rooms
            .get_mut(&inbound.target_room_id)
            .ok_or_else(|| RelayError::RoomNotFound(inbound.target_room_id.clone()))?;
        validate_bridge_room_admission(room, &inbound, self.limits)?;
        ensure_bridge_guest_participant(room, &inbound, &self.default_guest_scope);

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

    /// Open a Tier-0 `PromptGate` enforcing bounded gate capacity.
    ///
    /// # Errors
    ///
    /// Returns [`RelayError`] if capacity is exceeded or room/channel/requester is missing.
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
        if room.gates.len() >= self.limits.max_gates_per_room {
            return Err(RelayError::CapacityExceeded {
                resource: "gates",
                limit: self.limits.max_gates_per_room,
            });
        }
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

    /// Resolve a Tier-0 `PromptGate` enforcing `InvHumanOnlyTier0Gate` (human
    /// Owner/Maintainer only, exactly-once immutable transition).
    ///
    /// # Errors
    ///
    /// Returns [`RelayError::UnauthorizedAction`], [`RelayError::GateAlreadyResolved`],
    /// or [`RelayError::Protocol`].
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
        let actor = room.participants.get(&request.actor_id).ok_or_else(|| {
            RelayError::ParticipantNotFound {
                room_id: room_id.clone(),
                principal_id: request.actor_id.clone(),
            }
        })?;

        let receipt = GateApprovalReceipt::resolve_by_human(GateResolutionInput {
            gate_id: gate_id.clone(),
            actor_id: request.actor_id.clone(),
            actor_kind: actor.kind,
            approved: request.approved,
            reason: request.reason,
            resolved_at_unix: now_unix_secs(),
        })?;

        if !is_privileged_human(actor) {
            return Err(RelayError::UnauthorizedAction {
                action: "resolve_gate",
                actor_id: request.actor_id,
            });
        }

        let gate = room
            .gates
            .get_mut(gate_id)
            .ok_or_else(|| RelayError::GateNotFound {
                room_id: room_id.clone(),
                gate_id: gate_id.clone(),
            })?;
        if gate.decision != GateDecision::Pending {
            return Err(RelayError::GateAlreadyResolved {
                gate_id: gate_id.clone(),
            });
        }
        gate.decision = receipt.decision;
        gate.receipt = Some(receipt.clone());

        let _ = room.emit(RoomEventKind::GateResolved {
            receipt: receipt.clone(),
        });
        Ok(receipt)
    }
}

fn validate_bridge_room_admission(
    room: &mut RoomRecord,
    inbound: &InboundBridgeMessage,
    limits: RelayCapacityLimits,
) -> Result<(), RelayError> {
    if room.messages.len() >= limits.max_messages_per_room {
        return Err(RelayError::CapacityExceeded {
            resource: "messages",
            limit: limits.max_messages_per_room,
        });
    }
    if !room
        .channels
        .iter()
        .any(|channel| channel.id == inbound.target_channel_id)
    {
        return Err(RelayError::ChannelNotFound {
            room_id: inbound.target_room_id.clone(),
            channel_id: inbound.target_channel_id.clone(),
        });
    }
    let dedup_key = format!(
        "{:?}:{}:{}",
        inbound.origin, inbound.external_channel_id, inbound.external_message_id
    );
    if room.seen_bridge_ids.contains(&dedup_key) {
        return Err(RelayError::DuplicateBridgeMessage {
            external_message_id: inbound.external_message_id.clone(),
        });
    }
    if room.seen_bridge_ids.len() >= limits.max_bridge_dedup_entries {
        return Err(RelayError::CapacityExceeded {
            resource: "bridge_dedup_entries",
            limit: limits.max_bridge_dedup_entries,
        });
    }
    room.seen_bridge_ids.insert(dedup_key);
    Ok(())
}

fn ensure_bridge_guest_participant(
    room: &mut RoomRecord,
    inbound: &InboundBridgeMessage,
    default_guest_scope: &ScopeId,
) {
    if room.participants.contains_key(&inbound.sender_id) {
        return;
    }
    let participant = RoomParticipant {
        id: inbound.sender_id.clone(),
        display_name: inbound.sender_name.clone(),
        handle: inbound
            .sender_name
            .trim_start_matches('@')
            .to_ascii_lowercase(),
        kind: PrincipalKind::Human,
        role: PrincipalRole::ExternalGuest,
        scopes: vec![default_guest_scope.clone()],
        presence: PresenceStatus::Online,
    };
    room.participants
        .insert(participant.id.clone(), participant.clone());
    let _ = room.emit(RoomEventKind::ParticipantJoined { participant });
}

const fn is_privileged_human(participant: &RoomParticipant) -> bool {
    matches!(participant.kind, PrincipalKind::Human)
        && matches!(
            participant.role,
            PrincipalRole::Owner | PrincipalRole::Maintainer
        )
}

fn default_channels(primary_channel: ChannelId) -> Result<Vec<RoomChannel>, RelayError> {
    let mut channels = vec![RoomChannel {
        name: primary_channel.as_str().to_string(),
        id: primary_channel,
        topic: "Shared Marlin Cove collaboration channel".to_string(),
        is_direct: false,
    }];
    if channels[0].id.as_str() != "architecture" {
        channels.push(RoomChannel {
            id: ChannelId::new("architecture")?,
            name: "architecture".to_string(),
            topic: "Architecture decisions & Marlin Gateway coordination".to_string(),
            is_direct: false,
        });
    }
    if channels[0].id.as_str() != "security" {
        channels.push(RoomChannel {
            id: ChannelId::new("security")?,
            name: "security".to_string(),
            topic: "Tier-0 PromptGate approvals & room governance".to_string(),
            is_direct: false,
        });
    }
    for (id, name, topic) in [
        (
            "dm:agent:architect",
            "Architect",
            "Direct 1:1 channel with @architect",
        ),
        (
            "dm:agent:reviewer",
            "Reviewer",
            "Direct 1:1 channel with @reviewer",
        ),
        (
            "dm:agent:security",
            "Security",
            "Direct 1:1 channel with @security",
        ),
    ] {
        channels.push(RoomChannel {
            id: ChannelId::new(id)?,
            name: name.to_string(),
            topic: topic.to_string(),
            is_direct: true,
        });
    }
    Ok(channels)
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
