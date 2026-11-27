// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Shared bridge ingress envelope, errors, and secret verification helpers.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::marlin_gateway::MarlinGatewayTurnRequest;
use crate::protocol::{
    BridgeOrigin, ChannelId, MessageAttachment, PrincipalId, PrincipalKind, PrincipalRole,
    ProtocolError, RoomGovernanceEnvelope, RoomId, ScopeId, SessionKey,
};

/// Normalized inbound message translated from any external bot bridge into Cove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboundBridgeMessage {
    /// Source transport identifier.
    pub origin: BridgeOrigin,
    /// Platform-native message id.
    pub external_message_id: String,
    /// Platform-native conversation/chat/channel id used for outbound replies.
    pub external_channel_id: String,
    /// Sender principal id (namespaced by bridge, e.g. `tg:42`, `discord:99`).
    pub sender_id: PrincipalId,
    /// Human-readable sender display name.
    pub sender_name: String,
    /// Target Cove room id.
    pub target_room_id: RoomId,
    /// Target Cove channel id.
    pub target_channel_id: ChannelId,
    /// Deterministic session routing key for Marlin Gateway (`marlin-agent-core`).
    pub session_key: SessionKey,
    /// Message text body.
    pub content: String,
    /// Structured attachments.
    #[serde(default)]
    pub attachments: Vec<MessageAttachment>,
    /// Unix epoch timestamp in seconds.
    pub timestamp_unix: u64,
}

impl InboundBridgeMessage {
    /// Build a typed [`MarlinGatewayTurnRequest`] carrying a Cove governance
    /// envelope so `marlin-agent-core` can run agent-side `.org` context governance.
    #[must_use]
    pub fn into_marlin_gateway_request(
        &self,
        actor_role: PrincipalRole,
        granted_scopes: Vec<ScopeId>,
        target_agent_handle: Option<String>,
    ) -> MarlinGatewayTurnRequest {
        let governance = RoomGovernanceEnvelope {
            room_id: self.target_room_id.clone(),
            channel_id: self.target_channel_id.clone(),
            actor_id: self.sender_id.clone(),
            actor_kind: PrincipalKind::Human,
            actor_role,
            granted_scopes,
            origin: self.origin,
            tainted_external: self.origin.is_external_bridge()
                || actor_role == PrincipalRole::ExternalGuest,
        };
        MarlinGatewayTurnRequest {
            session_id: self.session_key.clone(),
            governance,
            target_agent_handle,
            message: self.content.clone(),
            attachments: self.attachments.clone(),
        }
    }
}

/// Errors produced by external bot bridge adapters.
#[derive(Debug, Error)]
pub enum BridgeError {
    /// Bridge webhook or ingress shared secret did not match.
    #[error("bridge authentication failed: invalid or missing secret header")]
    Unauthorized,
    /// Inbound payload was ignored by policy (e.g. missing bot mention or empty text).
    #[error("bridge message ignored: {reason}")]
    Ignored {
        /// Explanation of why the message did not enter Cove.
        reason: &'static str,
    },
    /// Protocol identifier validation failed.
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
    /// Outbound HTTP delivery to the remote bot API failed.
    #[error("bridge outbound delivery failed: {message}")]
    Delivery {
        /// Underlying failure details.
        message: String,
    },
}

/// Verify an optional shared secret header using constant-time byte XOR fold.
#[must_use]
pub fn verify_shared_secret(expected: Option<&str>, provided: Option<&str>) -> bool {
    let Some(expected) = expected.map(str::trim).filter(|value| !value.is_empty()) else {
        return true;
    };
    let Some(provided) = provided.map(str::trim) else {
        return false;
    };
    if expected.len() != provided.len() {
        return false;
    }
    expected
        .as_bytes()
        .iter()
        .zip(provided.as_bytes())
        .fold(0u8, |diff, (left, right)| diff | (left ^ right))
        == 0
}
