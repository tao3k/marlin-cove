// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Typed client boundary for dispatching Cove room and bridge turns to
//! **Marlin Gateway** (`marlin-agent-core`).
//!
//! Cove forwards a verified [`RoomGovernanceEnvelope`] so `marlin-agent-core`
//! can enforce its own atomic `.org` context governance cleanly on the agent side.

use serde::{Deserialize, Serialize};
use thiserror::Error;

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
    /// Marlin Gateway base URL was empty or malformed.
    #[error("Marlin Gateway base URL is empty or invalid")]
    InvalidBaseUrl,
    /// HTTP request or JSON decoding failed.
    #[error("Marlin Gateway request failed: {message}")]
    Request {
        /// Diagnostic failure description.
        message: String,
    },
}

/// Async HTTP client targeting Marlin Gateway (`marlin-agent-core`).
#[derive(Debug, Clone)]
pub struct MarlinGatewayClient {
    base_url: String,
    bearer_token: Option<String>,
    http: reqwest::Client,
}

impl MarlinGatewayClient {
    /// Construct a Marlin Gateway client from a base URL and optional bearer token.
    ///
    /// # Errors
    ///
    /// Returns [`MarlinGatewayError::InvalidBaseUrl`] when the URL is blank.
    pub fn new(
        base_url: impl Into<String>,
        bearer_token: Option<String>,
    ) -> Result<Self, MarlinGatewayError> {
        let raw = base_url.into();
        let trimmed = raw.trim().trim_end_matches('/');
        if trimmed.is_empty() {
            return Err(MarlinGatewayError::InvalidBaseUrl);
        }

        Ok(Self {
            base_url: trimmed.to_string(),
            bearer_token: bearer_token
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty()),
            http: reqwest::Client::new(),
        })
    }

    /// Return the concrete `/message` endpoint URL on Marlin Gateway.
    #[must_use]
    pub fn message_endpoint(&self) -> String {
        format!("{}/message", self.base_url)
    }

    /// Dispatch a typed turn request carrying Cove's [`RoomGovernanceEnvelope`].
    ///
    /// # Errors
    ///
    /// Returns [`MarlinGatewayError::Request`] when transport, status, or JSON decoding fails.
    pub async fn dispatch_turn(
        &self,
        request: &MarlinGatewayTurnRequest,
    ) -> Result<MarlinGatewayTurnResponse, MarlinGatewayError> {
        let mut builder = self.http.post(self.message_endpoint()).json(request);
        if let Some(token) = &self.bearer_token {
            builder = builder.bearer_auth(token);
        }

        let response = builder
            .send()
            .await
            .map_err(|source| MarlinGatewayError::Request {
                message: source.to_string(),
            })?;

        let response =
            response
                .error_for_status()
                .map_err(|source| MarlinGatewayError::Request {
                    message: source.to_string(),
                })?;

        response
            .json::<MarlinGatewayTurnResponse>()
            .await
            .map_err(|source| MarlinGatewayError::Request {
                message: source.to_string(),
            })
    }
}
