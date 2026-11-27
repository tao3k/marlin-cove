// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Typed domain protocol for Marlin Cove rooms, principals, invites, and gates.
//!
//! Marlin Cove owns room/people/bot governance; `.org` context governance stays
//! inside `marlin-agent-core`, which consumes [`RoomGovernanceEnvelope`].

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Errors produced when validating protocol identifiers or governance invariants.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ProtocolError {
    /// A required identifier was empty or whitespace-only.
    #[error("identifier `{field}` must not be empty")]
    EmptyIdentifier {
        /// Name of the invalid identifier field.
        field: &'static str,
    },
    /// Only a human principal may resolve a Tier-0 governance gate.
    #[error("tier-0 gate `{gate_id}` requires human approval; agent `{actor_id}` rejected")]
    HumanApprovalRequired {
        /// Target gate identifier.
        gate_id: GateId,
        /// Rejected non-human actor identifier.
        actor_id: PrincipalId,
    },
}

macro_rules! define_id_newtype {
    ($name:ident, $doc:literal, $field:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct a validated identifier, trimming surrounding whitespace.
            ///
            /// # Errors
            ///
            /// Returns [`ProtocolError::EmptyIdentifier`] when the input is blank.
            pub fn new(value: impl AsRef<str>) -> Result<Self, ProtocolError> {
                let trimmed = value.as_ref().trim();
                if trimmed.is_empty() {
                    return Err(ProtocolError::EmptyIdentifier { field: $field });
                }
                Ok(Self(trimmed.to_string()))
            }

            #[allow(dead_code)]
            pub(crate) const fn from_trusted(value: String) -> Self {
                Self(value)
            }

            /// Return the underlying string slice.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Display for $name {
            fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

define_id_newtype!(RoomId, "Unique Marlin Cove room identifier.", "room_id");
define_id_newtype!(
    ChannelId,
    "Unique channel identifier inside a Cove room.",
    "channel_id"
);
define_id_newtype!(
    PrincipalId,
    "Unique participant identifier (human or AI agent).",
    "principal_id"
);
define_id_newtype!(
    InviteToken,
    "Opaque invite token granting scoped entry to a Cove room.",
    "invite_token"
);
define_id_newtype!(
    MessageId,
    "Unique message identifier in a Cove room timeline.",
    "message_id"
);
define_id_newtype!(
    GateId,
    "Unique Tier-0 `PromptGate` approval identifier.",
    "gate_id"
);
define_id_newtype!(
    ScopeId,
    "Room membership scope tag forwarded to Marlin Gateway for agent-side context governance.",
    "scope_id"
);
define_id_newtype!(
    SessionKey,
    "Typed conversation session routing key across bridges and Marlin Gateway.",
    "session_key"
);

/// Whether a participant in a Cove room is a human or an AI agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// Human participant (may approve Tier-0 governance gates).
    Human,
    /// AI agent participant (may never self-approve Tier-0 gates).
    Agent,
}

/// Role assigned to a principal inside a Cove room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalRole {
    /// Room owner with full administrative and invite privileges.
    Owner,
    /// Maintainer who can manage channels, invites, and Tier-0 gates.
    Maintainer,
    /// Standard trusted team member.
    Member,
    /// External invited guest restricted to explicitly granted scopes.
    ExternalGuest,
    /// Specialized AI agent bound to a role (e.g. Architect, Reviewer, Security).
    RoleAgent,
}

/// Live presence state for a room member or agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PresenceStatus {
    /// Online and ready.
    #[default]
    Online,
    /// Agent is actively reasoning on a turn.
    Thinking,
    /// Agent is executing an approved tool or workflow step.
    Working,
    /// Temporarily idle.
    Idle,
    /// Disconnected.
    Offline,
}

/// Origin transport for a message entering a Cove room.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BridgeOrigin {
    /// Native Marlin Cove client (`marlin-ui` Desktop/Mobile/TUI).
    #[default]
    CoveNative,
    /// Bridged from Telegram Bot API.
    Telegram,
    /// Bridged from Discord Bot API.
    Discord,
    /// Dispatched from Marlin Gateway (`marlin-agent-core`).
    MarlinGateway,
}

impl BridgeOrigin {
    /// Return true when the origin is an external third-party bridge (Telegram/Discord).
    #[must_use]
    pub const fn is_external_bridge(self) -> bool {
        matches!(self, Self::Telegram | Self::Discord)
    }
}

/// Verified room governance metadata forwarded to Marlin Gateway (`marlin-agent-core`).
///
/// `marlin-agent-core` uses this envelope to run its own atomic `.org` context
/// projection without `marlin-cove` ever storing or inspecting `.org` content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomGovernanceEnvelope {
    /// Room identifier where the turn originated.
    pub room_id: RoomId,
    /// Channel identifier where the turn originated.
    pub channel_id: ChannelId,
    /// Verified actor principal id.
    pub actor_id: PrincipalId,
    /// Human vs AI agent kind.
    pub actor_kind: PrincipalKind,
    /// Actor's room role evaluated by Cove governance.
    pub actor_role: PrincipalRole,
    /// Room scopes granted to the actor by Cove invite/membership policy.
    pub granted_scopes: Vec<ScopeId>,
    /// Source transport bridge.
    pub origin: BridgeOrigin,
    /// True when message arrived via an external bridge or external guest.
    pub tainted_external: bool,
}

/// Participant profile inside a Cove room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomParticipant {
    /// Stable principal identifier.
    pub id: PrincipalId,
    /// Human-readable display name.
    pub display_name: String,
    /// `@handle` used for mention routing.
    pub handle: String,
    /// Human vs AI agent distinction.
    pub kind: PrincipalKind,
    /// Room membership role.
    pub role: PrincipalRole,
    /// Scopes granted to this participant inside the room.
    pub scopes: Vec<ScopeId>,
    /// Current presence status.
    pub presence: PresenceStatus,
}

impl RoomParticipant {
    /// Build a [`RoomGovernanceEnvelope`] for forwarding this participant's turn
    /// to Marlin Gateway (`marlin-agent-core`).
    #[must_use]
    pub fn governance_envelope(
        &self,
        room_id: RoomId,
        channel_id: ChannelId,
        origin: BridgeOrigin,
    ) -> RoomGovernanceEnvelope {
        let tainted_external =
            origin.is_external_bridge() || self.role == PrincipalRole::ExternalGuest;
        RoomGovernanceEnvelope {
            room_id,
            channel_id,
            actor_id: self.id.clone(),
            actor_kind: self.kind,
            actor_role: self.role,
            granted_scopes: self.scopes.clone(),
            origin,
            tainted_external,
        }
    }
}

/// Channel inside a Cove room (e.g. `#general`, `#architecture`, `#security`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomChannel {
    /// Stable channel identifier.
    pub id: ChannelId,
    /// Display slug without leading `#`.
    pub name: String,
    /// Short topic description.
    pub topic: String,
    /// Whether the channel is a 1:1 direct channel with an agent or human.
    pub is_direct: bool,
}

/// Scoped invite granting entry to a Cove room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomInvite {
    /// Opaque invite token.
    pub token: InviteToken,
    /// Target room identifier.
    pub room_id: RoomId,
    /// Principal who issued the invite.
    pub issued_by: PrincipalId,
    /// Role granted upon redemption.
    pub grant_role: PrincipalRole,
    /// Governance scopes granted upon redemption.
    pub grant_scopes: Vec<ScopeId>,
    /// Maximum number of redemptions allowed.
    pub max_uses: u32,
    /// Number of times already redeemed.
    pub uses: u32,
}

/// Structured attachment on a room or bridge message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MessageAttachment {
    /// Remote image URL attachment.
    ImageUrl {
        /// Remote image location.
        url: String,
    },
}

/// Immutable timeline message in a Cove room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomMessage {
    /// Unique message identifier.
    pub id: MessageId,
    /// Room identifier.
    pub room_id: RoomId,
    /// Channel identifier.
    pub channel_id: ChannelId,
    /// Optional thread root message identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_root_id: Option<MessageId>,
    /// Author principal identifier.
    pub author_id: PrincipalId,
    /// Author kind (human or agent).
    pub author_kind: PrincipalKind,
    /// Markdown message body.
    pub body: String,
    /// Mentioned `@handles` parsed from the message.
    #[serde(default)]
    pub mentions: Vec<String>,
    /// Optional linked Tier-0 governance gate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_id: Option<GateId>,
    /// Structured attachments.
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
    /// Source transport bridge.
    pub origin: BridgeOrigin,
    /// Monotonic bot-to-bot chain depth used to prevent infinite agent loops.
    pub bot_chain_depth: u8,
    /// Unix epoch timestamp in seconds.
    pub created_at_unix: u64,
}

/// Decision recorded for a Tier-0 governance gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDecision {
    /// Awaiting human review.
    Pending,
    /// Approved by an authorized human principal.
    Approved,
    /// Denied by an authorized human principal.
    Denied,
}

/// Tier-0 `PromptGate` item embedded in a Cove room.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomGate {
    /// Unique gate identifier.
    pub id: GateId,
    /// Owning room identifier.
    pub room_id: RoomId,
    /// Owning channel identifier.
    pub channel_id: ChannelId,
    /// Agent or human requesting the action.
    pub requested_by: PrincipalId,
    /// Concise summary of the guarded action.
    pub summary: String,
    /// Target tool or capability name.
    pub capability: String,
    /// Current gate decision.
    pub decision: GateDecision,
    /// Signed receipt once resolved by a human.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receipt: Option<GateApprovalReceipt>,
}

/// Typed input for constructing a verified human-only [`GateApprovalReceipt`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateResolutionInput {
    /// Target gate identifier.
    pub gate_id: GateId,
    /// Actor resolving the gate.
    pub actor_id: PrincipalId,
    /// Actor kind (must be [`PrincipalKind::Human`]).
    pub actor_kind: PrincipalKind,
    /// `true` for approved, `false` for denied.
    pub approved: bool,
    /// Rationale recorded on the receipt.
    pub reason: String,
    /// Unix epoch timestamp in seconds.
    pub resolved_at_unix: u64,
}

/// Typed receipt recording human-only resolution of a Tier-0 gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateApprovalReceipt {
    /// Gate identifier.
    pub gate_id: GateId,
    /// Human principal who resolved the gate.
    pub resolved_by: PrincipalId,
    /// Recorded decision (`Approved` or `Denied`).
    pub decision: GateDecision,
    /// Optional human rationale.
    pub reason: String,
    /// Unix epoch timestamp in seconds.
    pub resolved_at_unix: u64,
}

impl GateApprovalReceipt {
    /// Construct a verified Tier-0 gate receipt, rejecting non-human actors.
    ///
    /// # Errors
    ///
    /// Returns [`ProtocolError::HumanApprovalRequired`] if `actor_kind` is not
    /// [`PrincipalKind::Human`].
    pub fn resolve_by_human(input: GateResolutionInput) -> Result<Self, ProtocolError> {
        if input.actor_kind != PrincipalKind::Human {
            return Err(ProtocolError::HumanApprovalRequired {
                gate_id: input.gate_id,
                actor_id: input.actor_id,
            });
        }

        Ok(Self {
            gate_id: input.gate_id,
            resolved_by: input.actor_id,
            decision: if input.approved {
                GateDecision::Approved
            } else {
                GateDecision::Denied
            },
            reason: input.reason,
            resolved_at_unix: input.resolved_at_unix,
        })
    }
}

/// Monotonic real-time event emitted by a Cove room stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomEvent {
    /// Monotonically increasing sequence number within the room.
    pub seq: u64,
    /// Room identifier.
    pub room_id: RoomId,
    /// Typed event payload.
    pub kind: RoomEventKind,
}

/// Event payload variants streamed to `marlin-ui` clients and bridges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoomEventKind {
    /// A new message was posted to a channel.
    MessagePosted {
        /// Posted message.
        message: RoomMessage,
    },
    /// A participant joined the room.
    ParticipantJoined {
        /// Joined participant.
        participant: RoomParticipant,
    },
    /// A participant updated presence status.
    PresenceChanged {
        /// Target participant identifier.
        principal_id: PrincipalId,
        /// New presence status.
        presence: PresenceStatus,
    },
    /// A Tier-0 governance gate was opened in a channel.
    GateRequested {
        /// Requested gate.
        gate: RoomGate,
    },
    /// A Tier-0 governance gate was resolved by a human.
    GateResolved {
        /// Resolution receipt.
        receipt: GateApprovalReceipt,
    },
}

/// Full room snapshot returned to `marlin-ui` clients upon connect or resync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomSnapshot {
    /// Room identifier.
    pub room_id: RoomId,
    /// Human-readable room title.
    pub title: String,
    /// Latest event sequence number included in this snapshot.
    pub latest_seq: u64,
    /// Channels inside this room.
    pub channels: Vec<RoomChannel>,
    /// Participants (humans + AI agents) in this room.
    pub participants: Vec<RoomParticipant>,
    /// Timeline messages across channels.
    pub messages: Vec<RoomMessage>,
    /// Active and historical Tier-0 gates.
    pub gates: Vec<RoomGate>,
}

/// Extract normalized unique `@handle` mentions from Markdown message text.
#[must_use]
pub fn extract_mentions(body: &str) -> Vec<String> {
    body.split_whitespace()
        .filter_map(parse_mention_handle)
        .fold(Vec::new(), |mut acc, handle| {
            if !acc.contains(&handle) {
                acc.push(handle);
            }
            acc
        })
}

fn parse_mention_handle(token: &str) -> Option<String> {
    let rest = token.strip_prefix('@')?;
    let handle = rest
        .trim_end_matches(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        .to_ascii_lowercase();
    (!handle.is_empty()).then_some(handle)
}
