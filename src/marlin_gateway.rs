// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Typed gRPC HTTP/2 client targeting `marlin-agent-core`'s native
//! `marlin.app.v1.AppServer` (`StreamTurn`).
//!
//! `marlin-agent-core` requires zero `marlin-cove` crate dependencies: Cove
//! acts purely as an external `AppServer` gRPC client and passes its verified
//! [`RoomGovernanceEnvelope`] via typed HTTP/2 gRPC metadata headers.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tonic::metadata::{MetadataMap, MetadataValue};
use tonic::transport::Endpoint;

use crate::proto_wire::app_v1::app_server_client::AppServerClient;
use crate::proto_wire::app_v1::{ConversationMessage, MessageRole, StreamTurnRequest, turn_event};
use crate::protocol::{
    BridgeOrigin, MessageAttachment, PrincipalKind, PrincipalRole, RoomGovernanceEnvelope, ScopeId,
    SessionKey,
};

/// gRPC metadata key carrying Cove's verified `room_id`.
pub const COVE_META_ROOM_ID: &str = "x-marlin-cove-room-id";
/// gRPC metadata key carrying Cove's verified `channel_id`.
pub const COVE_META_CHANNEL_ID: &str = "x-marlin-cove-channel-id";
/// gRPC metadata key carrying Cove's verified `actor_id`.
pub const COVE_META_ACTOR_ID: &str = "x-marlin-cove-actor-id";
/// gRPC metadata key carrying Cove's verified `actor_kind`.
pub const COVE_META_ACTOR_KIND: &str = "x-marlin-cove-actor-kind";
/// gRPC metadata key carrying Cove's verified `actor_role`.
pub const COVE_META_ACTOR_ROLE: &str = "x-marlin-cove-actor-role";
/// gRPC metadata key carrying Cove's comma-separated `granted_scopes`.
pub const COVE_META_GRANTED_SCOPES: &str = "x-marlin-cove-granted-scopes";
/// gRPC metadata key carrying Cove's message `origin`.
pub const COVE_META_ORIGIN: &str = "x-marlin-cove-origin";
/// gRPC metadata key carrying Cove's `tainted_external` flag (`"true"` / `"false"`).
pub const COVE_META_TAINTED_EXTERNAL: &str = "x-marlin-cove-tainted-external";
/// gRPC metadata key carrying Cove's deterministic `session_id`.
pub const COVE_META_SESSION_ID: &str = "x-marlin-cove-session-id";
/// gRPC metadata key carrying the target `@handle` agent role when specified.
pub const COVE_META_TARGET_AGENT: &str = "x-marlin-cove-target-agent";

/// Typed turn dispatch request sent from Marlin Cove to `marlin-agent-core` (`marlin.app.v1.AppServer`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarlinGatewayTurnRequest {
    /// Stable session routing key.
    pub session_id: SessionKey,
    /// Verified Cove room/people/bot governance envelope forwarded as gRPC metadata.
    pub governance: RoomGovernanceEnvelope,
    /// Optional target `@handle` role (e.g. `architect`, `reviewer`, `security`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_agent_handle: Option<String>,
    /// User message body.
    pub message: String,
    /// Structured attachments accompanying the turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<MessageAttachment>,
}

/// Response assembled from `marlin.app.v1.AppServer/StreamTurn` events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarlinGatewayTurnResponse {
    /// Markdown assistant response body.
    pub response: String,
}

/// Errors produced when building or invoking [`MarlinGatewayClient`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum MarlinGatewayError {
    /// Marlin Gateway gRPC endpoint URL was empty or malformed.
    #[error("Marlin Gateway gRPC endpoint is empty or invalid")]
    InvalidEndpoint,
    /// Bearer token or governance header contained non-ASCII metadata characters.
    #[error("Marlin Gateway gRPC metadata value is not valid ASCII")]
    InvalidMetadata,
    /// gRPC HTTP/2 connection or RPC invocation failed.
    #[error("Marlin Gateway gRPC request failed: {message}")]
    Request {
        /// Diagnostic failure description.
        message: String,
    },
}

/// Async gRPC over HTTP/2 client calling `marlin-agent-core`'s native
/// `marlin.app.v1.AppServer` service.
#[derive(Debug, Clone)]
pub struct MarlinGatewayClient {
    endpoint: String,
    bearer_token: Option<String>,
}

impl MarlinGatewayClient {
    /// Construct a Marlin Gateway gRPC HTTP/2 client targeting a
    /// `marlin.app.v1.AppServer` endpoint URI and optional bearer token.
    ///
    /// # Errors
    ///
    /// Returns [`MarlinGatewayError::InvalidEndpoint`] when the URI is blank or malformed,
    /// or [`MarlinGatewayError::InvalidMetadata`] when the bearer token is non-ASCII.
    pub fn new(
        endpoint: impl Into<String>,
        bearer_token: Option<String>,
    ) -> Result<Self, MarlinGatewayError> {
        let raw = endpoint.into();
        let trimmed = raw.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(MarlinGatewayError::InvalidEndpoint);
        }
        let _ = Endpoint::from_shared(trimmed.to_string())
            .map_err(|_| MarlinGatewayError::InvalidEndpoint)?;

        let normalized_token = bearer_token
            .map(|token| token.trim().to_string())
            .filter(|token| !token.is_empty());
        if let Some(token) = &normalized_token {
            let header = format!("Bearer {token}");
            if MetadataValue::try_from(header.as_str()).is_err() {
                return Err(MarlinGatewayError::InvalidMetadata);
            }
        }

        Ok(Self {
            endpoint: trimmed.to_string(),
            bearer_token: normalized_token,
        })
    }

    /// Return the configured `marlin.app.v1.AppServer` gRPC HTTP/2 endpoint URI.
    #[must_use]
    pub fn grpc_endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Dispatch a typed turn over HTTP/2 to `marlin.app.v1.AppServer/StreamTurn`,
    /// attaching Cove's verified [`RoomGovernanceEnvelope`] in gRPC metadata and
    /// assembling the streamed `TextDelta` response chunks.
    ///
    /// # Errors
    ///
    /// Returns [`MarlinGatewayError::Request`] when HTTP/2 transport or RPC fails.
    pub async fn dispatch_turn(
        &self,
        request: &MarlinGatewayTurnRequest,
    ) -> Result<MarlinGatewayTurnResponse, MarlinGatewayError> {
        let channel = Endpoint::from_shared(self.endpoint.clone())
            .map_err(|_| MarlinGatewayError::InvalidEndpoint)?
            .connect()
            .await
            .map_err(|source| MarlinGatewayError::Request {
                message: source.to_string(),
            })?;

        let mut client = AppServerClient::new(channel);
        let mut grpc_req = tonic::Request::new(StreamTurnRequest {
            messages: vec![ConversationMessage {
                role: MessageRole::User as i32,
                text: request.message.clone(),
            }],
        });

        attach_request_metadata(
            grpc_req.metadata_mut(),
            self.bearer_token.as_deref(),
            request,
        )?;

        let mut stream = client
            .stream_turn(grpc_req)
            .await
            .map_err(|status| MarlinGatewayError::Request {
                message: status.to_string(),
            })?
            .into_inner();

        let mut assembled = String::new();
        while let Some(event) =
            stream
                .message()
                .await
                .map_err(|status| MarlinGatewayError::Request {
                    message: status.to_string(),
                })?
        {
            if let Some(turn_event::Event::TextDelta(delta)) = event.event {
                assembled.push_str(&delta.text);
            }
        }

        Ok(MarlinGatewayTurnResponse {
            response: assembled,
        })
    }
}

fn attach_request_metadata(
    metadata: &mut MetadataMap,
    bearer_token: Option<&str>,
    request: &MarlinGatewayTurnRequest,
) -> Result<(), MarlinGatewayError> {
    if let Some(token) = bearer_token {
        insert_meta(metadata, "authorization", &format!("Bearer {token}"))?;
    }
    let gov = &request.governance;
    let scopes = gov
        .granted_scopes
        .iter()
        .map(ScopeId::as_str)
        .collect::<Vec<_>>()
        .join(",");
    insert_meta(metadata, COVE_META_ROOM_ID, gov.room_id.as_str())?;
    insert_meta(metadata, COVE_META_CHANNEL_ID, gov.channel_id.as_str())?;
    insert_meta(metadata, COVE_META_ACTOR_ID, gov.actor_id.as_str())?;
    insert_meta(
        metadata,
        COVE_META_ACTOR_KIND,
        principal_kind_label(gov.actor_kind),
    )?;
    insert_meta(
        metadata,
        COVE_META_ACTOR_ROLE,
        principal_role_label(gov.actor_role),
    )?;
    insert_meta(metadata, COVE_META_GRANTED_SCOPES, &scopes)?;
    insert_meta(metadata, COVE_META_ORIGIN, bridge_origin_label(gov.origin))?;
    insert_meta(
        metadata,
        COVE_META_TAINTED_EXTERNAL,
        if gov.tainted_external {
            "true"
        } else {
            "false"
        },
    )?;
    insert_meta(metadata, COVE_META_SESSION_ID, request.session_id.as_str())?;
    if let Some(handle) = request.target_agent_handle.as_deref() {
        let trimmed = handle.trim();
        if !trimmed.is_empty() {
            insert_meta(metadata, COVE_META_TARGET_AGENT, trimmed)?;
        }
    }
    Ok(())
}

fn insert_meta(
    metadata: &mut MetadataMap,
    key: &'static str,
    value: &str,
) -> Result<(), MarlinGatewayError> {
    let parsed = MetadataValue::try_from(value).map_err(|_| MarlinGatewayError::InvalidMetadata)?;
    metadata.insert(key, parsed);
    Ok(())
}

const fn principal_kind_label(kind: PrincipalKind) -> &'static str {
    match kind {
        PrincipalKind::Human => "human",
        PrincipalKind::Agent => "agent",
    }
}

const fn principal_role_label(role: PrincipalRole) -> &'static str {
    match role {
        PrincipalRole::Owner => "owner",
        PrincipalRole::Maintainer => "maintainer",
        PrincipalRole::Member => "member",
        PrincipalRole::ExternalGuest => "external_guest",
        PrincipalRole::RoleAgent => "role_agent",
    }
}

const fn bridge_origin_label(origin: BridgeOrigin) -> &'static str {
    match origin {
        BridgeOrigin::CoveNative => "cove_native",
        BridgeOrigin::Telegram => "telegram",
        BridgeOrigin::Discord => "discord",
        BridgeOrigin::MarlinGateway => "marlin_gateway",
    }
}
