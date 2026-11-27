// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! External third-party HTTP webhook ingress for Telegram and Discord.
//!
//! All native Marlin Cove client (`marlin-ui`) and Marlin Gateway
//! (`marlin-agent-core`) communication runs exclusively over gRPC HTTP/2.

use axum::extract::{DefaultBodyLimit, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

#[cfg(feature = "discord")]
use crate::bridges::discord::{
    DiscordIngressContext, DiscordIngressPayload, DiscordMentionGate, DiscordSessionPartition,
    DiscordUserId, translate_discord_ingress, verify_discord_ingress_secret,
};
#[cfg(feature = "telegram")]
use crate::bridges::telegram::{
    TelegramSessionPartition, TelegramWebhookContext, TelegramWebhookUpdate,
    translate_webhook_update, verify_telegram_webhook_secret,
};
use crate::config::SecretValue;
use crate::protocol::ProtocolError;
use crate::relay::requests::RelayError;
use crate::relay::state::CoveServerState;
use crate::relay::store::CoveRelayStore;

/// Typed ingress outcome for external Telegram/Discord HTTP webhook callbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeIngressStatus {
    /// Message passed bridge policy and was posted into a Cove room.
    Ingested,
    /// Message was intentionally ignored by bridge policy.
    Ignored,
    /// Replayed external message id was deduplicated idempotently.
    Duplicate,
}

/// Bridge webhook acknowledgement returned for external Telegram/Discord calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeIngressAck {
    /// Typed outcome (`"ingested"`, `"ignored"`, or `"duplicate"`).
    pub status: BridgeIngressStatus,
    /// Cove message id when ingested into a room.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Diagnostic reason when ignored or deduplicated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct WebhookErrorEnvelope {
    error: String,
}

struct WebhookApiError {
    status: StatusCode,
    message: String,
}

impl WebhookApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for WebhookApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(WebhookErrorEnvelope {
                error: self.message,
            }),
        )
            .into_response()
    }
}

impl From<RelayError> for WebhookApiError {
    fn from(error: RelayError) -> Self {
        let status = match &error {
            RelayError::RoomNotFound(_)
            | RelayError::ChannelNotFound { .. }
            | RelayError::ParticipantNotFound { .. }
            | RelayError::GateNotFound { .. } => StatusCode::NOT_FOUND,
            RelayError::InvalidInvite(_)
            | RelayError::BotLoopLimitExceeded { .. }
            | RelayError::PrincipalKindRoleMismatch
            | RelayError::Protocol(ProtocolError::EmptyIdentifier { .. }) => {
                StatusCode::BAD_REQUEST
            }
            RelayError::MessageTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
            RelayError::CapacityExceeded { .. } => StatusCode::TOO_MANY_REQUESTS,
            RelayError::UnauthorizedAction { .. }
            | RelayError::PrivilegeEscalation { .. }
            | RelayError::Protocol(ProtocolError::HumanApprovalRequired { .. }) => {
                StatusCode::FORBIDDEN
            }
            RelayError::RoomAlreadyExists(_)
            | RelayError::ParticipantAlreadyExists { .. }
            | RelayError::GateAlreadyResolved { .. }
            | RelayError::DuplicateBridgeMessage { .. } => StatusCode::CONFLICT,
            RelayError::LockPoisoned => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.to_string())
    }
}

/// Build the thin HTTP webhook router strictly for compiled third-party bridges.
pub fn build_webhook_router(state: CoveServerState) -> Router {
    let max_body_bytes = state.config.relay.limits.max_grpc_message_bytes;
    let mut router = Router::new();

    #[cfg(feature = "telegram")]
    {
        let webhook_path = state.config.telegram.webhook_path.as_str().to_string();
        router = router.route(&webhook_path, post(telegram_webhook_handler));
    }

    #[cfg(feature = "discord")]
    {
        let ingress_path = state.config.discord.ingress_path.as_str().to_string();
        router = router.route(&ingress_path, post(discord_ingress_handler));
    }

    router
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .with_state(state)
}

#[cfg(feature = "telegram")]
async fn telegram_webhook_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Json(update): Json<TelegramWebhookUpdate>,
) -> Result<impl IntoResponse, WebhookApiError> {
    let Some(expected) = state
        .config
        .telegram
        .webhook_secret
        .as_ref()
        .map(SecretValue::as_str)
        .filter(|secret| !secret.trim().is_empty())
    else {
        return Err(WebhookApiError::new(
            StatusCode::UNAUTHORIZED,
            "telegram webhook secret is not configured (fail-closed)",
        ));
    };
    let provided = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok());
    if !verify_telegram_webhook_secret(Some(expected), provided) {
        return Err(WebhookApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid Telegram webhook secret token",
        ));
    }

    let context = TelegramWebhookContext {
        partition: state
            .config
            .telegram
            .session_partition
            .as_str()
            .parse::<TelegramSessionPartition>()
            .unwrap_or_default(),
        target_room_id: state.config.telegram.default_room_id.clone(),
        target_channel_id: state.config.telegram.default_channel_id.clone(),
    };

    match translate_webhook_update(&update, context) {
        Ok(inbound) => ingest_bridge_to_response(&state.store, inbound),
        Err(crate::bridges::BridgeError::Ignored { reason }) => Ok((
            StatusCode::OK,
            Json(BridgeIngressAck {
                status: BridgeIngressStatus::Ignored,
                message_id: None,
                reason: Some(reason.to_string()),
            }),
        )),
        Err(error) => Err(WebhookApiError::new(
            StatusCode::BAD_REQUEST,
            error.to_string(),
        )),
    }
}

#[cfg(feature = "discord")]
async fn discord_ingress_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Json(payload): Json<DiscordIngressPayload>,
) -> Result<impl IntoResponse, WebhookApiError> {
    let Some(expected) = state
        .config
        .discord
        .ingress_secret
        .as_ref()
        .map(SecretValue::as_str)
        .filter(|secret| !secret.trim().is_empty())
    else {
        return Err(WebhookApiError::new(
            StatusCode::UNAUTHORIZED,
            "discord ingress secret is not configured (fail-closed)",
        ));
    };
    let provided = headers
        .get("x-marlin-cove-ingress-secret")
        .and_then(|value| value.to_str().ok());
    if !verify_discord_ingress_secret(Some(expected), provided) {
        return Err(WebhookApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid Discord ingress secret",
        ));
    }

    let context = DiscordIngressContext {
        partition: state
            .config
            .discord
            .session_partition
            .as_str()
            .parse::<DiscordSessionPartition>()
            .unwrap_or_default(),
        mention_gate: DiscordMentionGate {
            require_mention: state.config.discord.mention_policy.requires_mention(),
            bot_user_id: state
                .config
                .discord
                .bot_user_id
                .as_ref()
                .and_then(|id| DiscordUserId::new(id.as_str()).ok()),
        },
        target_room_id: state.config.discord.default_room_id.clone(),
        target_channel_id: state.config.discord.default_channel_id.clone(),
    };

    match translate_discord_ingress(&payload, &context) {
        Ok(inbound) => ingest_bridge_to_response(&state.store, inbound),
        Err(crate::bridges::BridgeError::Ignored { reason }) => Ok((
            StatusCode::OK,
            Json(BridgeIngressAck {
                status: BridgeIngressStatus::Ignored,
                message_id: None,
                reason: Some(reason.to_string()),
            }),
        )),
        Err(error) => Err(WebhookApiError::new(
            StatusCode::BAD_REQUEST,
            error.to_string(),
        )),
    }
}

fn ingest_bridge_to_response(
    store: &CoveRelayStore,
    inbound: crate::bridges::InboundBridgeMessage,
) -> Result<(StatusCode, Json<BridgeIngressAck>), WebhookApiError> {
    match store.ingest_bridge_message(inbound) {
        Ok(message) => Ok((
            StatusCode::ACCEPTED,
            Json(BridgeIngressAck {
                status: BridgeIngressStatus::Ingested,
                message_id: Some(message.id.to_string()),
                reason: None,
            }),
        )),
        Err(RelayError::DuplicateBridgeMessage {
            external_message_id,
        }) => Ok((
            StatusCode::OK,
            Json(BridgeIngressAck {
                status: BridgeIngressStatus::Duplicate,
                message_id: None,
                reason: Some(format!("duplicate external message {external_message_id}")),
            }),
        )),
        Err(error) => Err(WebhookApiError::from(error)),
    }
}
