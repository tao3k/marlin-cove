// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Typed gRPC HTTP/2 client for Marlin Gateway (`marlin-agent-core`).
//!
//! Cove forwards a verified [`RoomGovernanceEnvelope`] over HTTP/2 protobuf so
//! `marlin-agent-core` enforces its own atomic `.org` context governance on the
//! agent side.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tonic::metadata::MetadataValue;
use tonic::transport::Endpoint;

use crate::proto_codec::{attachment_to_proto, governance_envelope_to_proto};
use crate::proto_wire::cove_v1;
use crate::proto_wire::cove_v1::marlin_gateway_service_client::MarlinGatewayServiceClient;
use crate::protocol::{MessageAttachment, RoomGovernanceEnvelope, SessionKey};

/// Typed turn dispatch request sent from Marlin Cove to Marlin Gateway (`marlin-agent-core`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MarlinGatewayTurnRequest {
    /// Stable session routing key.
    pub session_id: SessionKey,
    /// Verified Cove room/people/bot governance envelope consumed by `marlin-agent-core`.
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

/// Response returned by Marlin Gateway (`marlin-agent-core`).
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
    /// Bearer token contained non-ASCII characters invalid for HTTP/2 metadata.
    #[error("Marlin Gateway bearer token is not valid ASCII metadata")]
    InvalidBearerToken,
    /// gRPC HTTP/2 connection or RPC invocation failed.
    #[error("Marlin Gateway gRPC request failed: {message}")]
    Request {
        /// Diagnostic failure description.
        message: String,
    },
}

/// Async gRPC over HTTP/2 client targeting Marlin Gateway (`marlin-agent-core`).
#[derive(Debug, Clone)]
pub struct MarlinGatewayClient {
    endpoint: String,
    bearer_token: Option<String>,
}

impl MarlinGatewayClient {
    /// Construct a Marlin Gateway gRPC HTTP/2 client from an endpoint URI and
    /// optional bearer token.
    ///
    /// # Errors
    ///
    /// Returns [`MarlinGatewayError::InvalidEndpoint`] when the URI is blank or malformed.
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
                return Err(MarlinGatewayError::InvalidBearerToken);
            }
        }

        Ok(Self {
            endpoint: trimmed.to_string(),
            bearer_token: normalized_token,
        })
    }

    /// Return the configured Marlin Gateway gRPC HTTP/2 endpoint URI.
    #[must_use]
    pub fn grpc_endpoint(&self) -> &str {
        &self.endpoint
    }

    /// Dispatch a typed turn request carrying Cove's [`RoomGovernanceEnvelope`]
    /// over gRPC HTTP/2 (`marlin.cove.v1.MarlinGatewayService/DispatchTurn`).
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

        let mut client = MarlinGatewayServiceClient::new(channel);
        let proto_req = cove_v1::MarlinGatewayTurnRequest {
            session_id: request.session_id.to_string(),
            governance: Some(governance_envelope_to_proto(&request.governance)),
            target_agent_handle: request.target_agent_handle.clone().unwrap_or_default(),
            message: request.message.clone(),
            attachments: request
                .attachments
                .iter()
                .map(attachment_to_proto)
                .collect(),
        };
        let mut grpc_req = tonic::Request::new(proto_req);
        if let Some(token) = &self.bearer_token {
            let value = MetadataValue::try_from(format!("Bearer {token}").as_str())
                .map_err(|_| MarlinGatewayError::InvalidBearerToken)?;
            grpc_req.metadata_mut().insert("authorization", value);
        }

        let response = client
            .dispatch_turn(grpc_req)
            .await
            .map_err(|status| MarlinGatewayError::Request {
                message: status.to_string(),
            })?
            .into_inner();

        Ok(MarlinGatewayTurnResponse {
            response: response.response,
        })
    }
}
