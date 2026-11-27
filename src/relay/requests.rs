// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Relay request/response DTOs and typed errors for Marlin Cove.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::protocol::{
    BridgeOrigin, ChannelId, GateId, MessageAttachment, MessageId, PrincipalId, PrincipalKind,
    PrincipalRole, ProtocolError, RoomGovernanceEnvelope, RoomId, RoomMessage, ScopeId,
};

/// Errors returned by [`crate::relay::CoveRelayStore`] mutations and queries.
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
    /// Invite redemption tried to overwrite an existing room participant.
    #[error("principal `{principal_id}` already exists in room `{room_id}`")]
    ParticipantAlreadyExists {
        /// Room identifier.
        room_id: RoomId,
        /// Conflicting principal identifier.
        principal_id: PrincipalId,
    },
    /// Invite token was not found or exhausted.
    #[error("invite token `{0}` is invalid or exhausted")]
    InvalidInvite(InviteTokenError),
    /// Actor lacked required role or kind to perform a privileged operation.
    #[error("unauthorized action `{action}` by principal `{actor_id}`")]
    UnauthorizedAction {
        /// Privileged action name.
        action: &'static str,
        /// Rejected actor identifier.
        actor_id: PrincipalId,
    },
    /// Invite attempted to grant a privileged role or escalate scope ceiling.
    #[error("privilege escalation rejected: {reason}")]
    PrivilegeEscalation {
        /// Explanation of the rejected escalation.
        reason: &'static str,
    },
    /// Redeeming principal kind did not match the invite's role class.
    #[error("principal kind does not match invite role")]
    PrincipalKindRoleMismatch,
    /// Gate was not found in the target room.
    #[error("gate `{gate_id}` not found in room `{room_id}`")]
    GateNotFound {
        /// Room identifier.
        room_id: RoomId,
        /// Missing gate identifier.
        gate_id: GateId,
    },
    /// Tier-0 gate was already resolved (replay/overwrite rejected).
    #[error("gate `{gate_id}` is already resolved and immutable")]
    GateAlreadyResolved {
        /// Already-resolved gate identifier.
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
    /// Bounded retention capacity exceeded (`CapacityModel` overflow refusal).
    #[error("capacity limit reached for `{resource}` (max {limit})")]
    CapacityExceeded {
        /// Bounded resource name.
        resource: &'static str,
        /// Configured limit.
        limit: usize,
    },
    /// Message body exceeded configured byte limit.
    #[error("message body size {actual} exceeds limit {limit} bytes")]
    MessageTooLarge {
        /// Incoming byte length.
        actual: usize,
        /// Configured maximum bytes.
        limit: usize,
    },
    /// External webhook message id was already ingested (replay deduplication).
    #[error("duplicate external bridge message `{external_message_id}` ignored")]
    DuplicateBridgeMessage {
        /// External platform message id.
        external_message_id: String,
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
    /// Issuing principal id (must be a human Owner or Maintainer).
    pub issued_by: PrincipalId,
    /// Role granted to redeeming principal (cannot be `Owner`).
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
    /// Joining principal identifier (must not collide with existing members).
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

/// Request to post a message and dispatch it live to Marlin Gateway (`marlin-agent-core`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchTurnRequest {
    /// Target channel id.
    pub channel_id: ChannelId,
    /// Author principal id (must belong to the room).
    pub author_id: PrincipalId,
    /// Target role agent handle (e.g. `architect`, `reviewer`, `security`).
    pub target_agent_handle: String,
    /// User prompt body.
    pub body: String,
}

/// Combined receipt returned after live Marlin Gateway dispatch completes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchTurnResponse {
    /// User message recorded in the Cove room.
    pub user_message: RoomMessage,
    /// Verified Cove governance envelope sent to Marlin Gateway.
    pub governance: RoomGovernanceEnvelope,
    /// Assistant reply posted back into the Cove room from Marlin Gateway.
    pub agent_reply: RoomMessage,
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

/// Request to resolve a Tier-0 `PromptGate` (human Owner/Maintainer only, once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolveGateRequest {
    /// Acting principal id (must be a human Owner or Maintainer).
    pub actor_id: PrincipalId,
    /// `true` to approve, `false` to deny.
    pub approved: bool,
    /// Rationale recorded on the receipt.
    #[serde(default)]
    pub reason: String,
}
