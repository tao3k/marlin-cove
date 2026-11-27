// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Cloud Cove HTTP + SSE router and feature-gated bot webhooks.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::CorsLayer;

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
use crate::config::{CoveConfig, compiled_features};
use crate::protocol::{BridgeOrigin, ChannelId, GateId, PrincipalId, ProtocolError, RoomId};
use crate::relay::store::{
    CoveRelayStore, CreateInviteRequest, CreateRoomRequest, PostMessageRequest,
    RedeemInviteRequest, RelayError, RequestGateRequest, ResolveGateRequest,
};

/// Shared runtime state mounted on the Axum router.
#[derive(Clone)]
pub struct CoveServerState {
    /// Active Cove configuration.
    pub config: Arc<CoveConfig>,
    /// Thread-safe Cove room/gate/membership store.
    pub store: CoveRelayStore,
}

impl CoveServerState {
    /// Build server state from a [`CoveConfig`].
    #[must_use]
    pub fn from_config(config: CoveConfig) -> Self {
        let store = CoveRelayStore::with_default_room(
            config.relay.max_bot_chain_depth,
            &config.governance.default_guest_scope,
        );
        Self {
            config: Arc::new(config),
            store,
        }
    }
}

/// Typed liveness state reported by `GET /v1/health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceHealthStatus {
    /// Relay server is healthy and accepting traffic.
    Ok,
}

/// Health response returned by `GET /v1/health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Typed service status (`"ok"`).
    pub status: ServiceHealthStatus,
    /// Service identifier (`"marlin-cove"`).
    pub service: String,
    /// Compile-time Cargo features enabled in this server build.
    pub features: Vec<String>,
}

/// Typed ingress outcome for Telegram/Discord webhook calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BridgeIngressStatus {
    /// Message passed bridge policy and was posted into a Cove room.
    Ingested,
    /// Message was intentionally ignored by bridge policy (e.g. unmentioned or bot-authored).
    Ignored,
}

/// Bridge webhook acknowledgement returned for Telegram/Discord ingress calls.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeIngressAck {
    /// Typed outcome (`"ingested"` or `"ignored"`).
    pub status: BridgeIngressStatus,
    /// Cove message id when ingested into a room.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    /// Diagnostic reason when ignored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: String,
}

struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorEnvelope {
                error: self.message,
            }),
        )
            .into_response()
    }
}

impl From<RelayError> for ApiError {
    fn from(error: RelayError) -> Self {
        let status = match &error {
            RelayError::RoomNotFound(_)
            | RelayError::ChannelNotFound { .. }
            | RelayError::ParticipantNotFound { .. }
            | RelayError::GateNotFound { .. } => StatusCode::NOT_FOUND,
            RelayError::InvalidInvite(_)
            | RelayError::BotLoopLimitExceeded { .. }
            | RelayError::Protocol(ProtocolError::EmptyIdentifier { .. }) => {
                StatusCode::BAD_REQUEST
            }
            RelayError::Protocol(ProtocolError::HumanApprovalRequired { .. }) => {
                StatusCode::FORBIDDEN
            }
            RelayError::RoomAlreadyExists(_) => StatusCode::CONFLICT,
            RelayError::LockPoisoned => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, error.to_string())
    }
}

impl From<ProtocolError> for ApiError {
    fn from(error: ProtocolError) -> Self {
        let status = match error {
            ProtocolError::EmptyIdentifier { .. } => StatusCode::BAD_REQUEST,
            ProtocolError::HumanApprovalRequired { .. } => StatusCode::FORBIDDEN,
        };
        Self::new(status, error.to_string())
    }
}

/// Build the complete Axum router with all compiled Cargo feature routes mounted.
pub fn build_router(state: CoveServerState) -> Router {
    let mut router = Router::new()
        .route("/v1/health", get(health_handler))
        .route(
            "/v1/rooms",
            get(list_rooms_handler).post(create_room_handler),
        )
        .route("/v1/rooms/{room_id}/snapshot", get(room_snapshot_handler))
        .route(
            "/v1/rooms/{room_id}/governance/{principal_id}",
            get(governance_envelope_handler),
        )
        .route("/v1/rooms/{room_id}/invites", post(create_invite_handler))
        .route("/v1/invites/{token}/join", post(redeem_invite_handler))
        .route("/v1/rooms/{room_id}/messages", post(post_message_handler))
        .route("/v1/rooms/{room_id}/events", get(room_events_sse_handler))
        .route("/v1/rooms/{room_id}/gates", post(request_gate_handler))
        .route(
            "/v1/rooms/{room_id}/gates/{gate_id}/resolve",
            post(resolve_gate_handler),
        );

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

    router.layer(CorsLayer::permissive()).with_state(state)
}

/// Bind and run the Cloud Cove Relay server until terminated.
///
/// # Errors
///
/// Returns an I/O error if binding the TCP listener or serving fails.
pub async fn serve(config: CoveConfig) -> std::io::Result<()> {
    let bind_addr = config.relay.bind_addr.as_str().to_string();
    let state = CoveServerState::from_config(config);
    let app = build_router(state);
    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!(%bind_addr, features = ?compiled_features(), "marlin-cove relay listening");
    axum::serve(listener, app).await
}

fn check_relay_auth(config: &CoveConfig, headers: &HeaderMap) -> Result<(), ApiError> {
    let Some(expected) = config
        .relay
        .auth_token
        .as_ref()
        .map(|secret| secret.as_str().trim())
        .filter(|token| !token.is_empty())
    else {
        return Ok(());
    };

    let provided = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(str::trim);

    if provided == Some(expected) {
        Ok(())
    } else {
        Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "missing or invalid Cove relay bearer token",
        ))
    }
}

async fn health_handler() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: ServiceHealthStatus::Ok,
        service: "marlin-cove".to_string(),
        features: compiled_features()
            .into_iter()
            .map(ToString::to_string)
            .collect(),
    })
}

async fn list_rooms_handler(
    State(state): State<CoveServerState>,
) -> Result<impl IntoResponse, ApiError> {
    Ok(Json(state.store.list_rooms()?))
}

async fn create_room_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Json(request): Json<CreateRoomRequest>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let snapshot = state.store.create_room(request)?;
    Ok((StatusCode::CREATED, Json(snapshot)))
}

async fn room_snapshot_handler(
    State(state): State<CoveServerState>,
    Path(room_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    let room_id = RoomId::new(room_id)?;
    Ok(Json(state.store.room_snapshot(&room_id)?))
}

async fn governance_envelope_handler(
    State(state): State<CoveServerState>,
    Path((room_id, principal_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let room_id = RoomId::new(room_id)?;
    let principal_id = PrincipalId::new(principal_id)?;
    let channel_id = ChannelId::new("general")?;
    let envelope = state.store.governance_envelope(
        &room_id,
        channel_id,
        &principal_id,
        BridgeOrigin::CoveNative,
    )?;
    Ok(Json(envelope))
}

async fn create_invite_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
    Json(request): Json<CreateInviteRequest>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let room_id = RoomId::new(room_id)?;
    let invite = state.store.create_invite(&room_id, request)?;
    Ok((StatusCode::CREATED, Json(invite)))
}

async fn redeem_invite_handler(
    State(state): State<CoveServerState>,
    Path(token): Path<String>,
    Json(request): Json<RedeemInviteRequest>,
) -> Result<impl IntoResponse, ApiError> {
    let participant = state.store.redeem_invite(&token, request)?;
    Ok((StatusCode::OK, Json(participant)))
}

async fn post_message_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
    Json(request): Json<PostMessageRequest>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let room_id = RoomId::new(room_id)?;
    let message = state.store.post_message(&room_id, request)?;
    Ok((StatusCode::CREATED, Json(message)))
}

async fn room_events_sse_handler(
    State(state): State<CoveServerState>,
    Path(room_id): Path<String>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    let room_id = RoomId::new(room_id)?;
    let rx = state.store.subscribe_room_events(&room_id)?;
    let stream = BroadcastStream::new(rx).filter_map(|item| {
        let event = item.ok()?;
        let data = serde_json::to_string(&event).ok()?;
        Some(Ok(SseEvent::default()
            .id(event.seq.to_string())
            .event("room_event")
            .data(data)))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn request_gate_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
    Json(request): Json<RequestGateRequest>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let room_id = RoomId::new(room_id)?;
    let gate = state.store.request_gate(&room_id, request)?;
    Ok((StatusCode::CREATED, Json(gate)))
}

async fn resolve_gate_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Path((room_id, gate_id)): Path<(String, String)>,
    Json(request): Json<ResolveGateRequest>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let room_id = RoomId::new(room_id)?;
    let gate_id = GateId::new(gate_id)?;
    let receipt = state.store.resolve_gate(&room_id, &gate_id, request)?;
    Ok((StatusCode::OK, Json(receipt)))
}

#[cfg(feature = "telegram")]
async fn telegram_webhook_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Json(update): Json<TelegramWebhookUpdate>,
) -> Result<impl IntoResponse, ApiError> {
    let provided = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok());
    let expected = state
        .config
        .telegram
        .webhook_secret
        .as_ref()
        .map(crate::config::SecretValue::as_str);
    if !verify_telegram_webhook_secret(expected, provided) {
        return Err(ApiError::new(
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
        target_channel_id: ChannelId::new("general")?,
    };

    match translate_webhook_update(&update, context) {
        Ok(inbound) => {
            let message = state.store.ingest_bridge_message(inbound)?;
            Ok((
                StatusCode::ACCEPTED,
                Json(BridgeIngressAck {
                    status: BridgeIngressStatus::Ingested,
                    message_id: Some(message.id.to_string()),
                    reason: None,
                }),
            ))
        }
        Err(crate::bridges::BridgeError::Ignored { reason }) => Ok((
            StatusCode::OK,
            Json(BridgeIngressAck {
                status: BridgeIngressStatus::Ignored,
                message_id: None,
                reason: Some(reason.to_string()),
            }),
        )),
        Err(error) => Err(ApiError::new(StatusCode::BAD_REQUEST, error.to_string())),
    }
}

#[cfg(feature = "discord")]
async fn discord_ingress_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Json(payload): Json<DiscordIngressPayload>,
) -> Result<impl IntoResponse, ApiError> {
    let provided = headers
        .get("x-marlin-cove-ingress-secret")
        .and_then(|value| value.to_str().ok());
    let expected = state
        .config
        .discord
        .ingress_secret
        .as_ref()
        .map(crate::config::SecretValue::as_str);
    if !verify_discord_ingress_secret(expected, provided) {
        return Err(ApiError::new(
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
        target_channel_id: ChannelId::new("general")?,
    };

    match translate_discord_ingress(&payload, &context) {
        Ok(inbound) => {
            let message = state.store.ingest_bridge_message(inbound)?;
            Ok((
                StatusCode::ACCEPTED,
                Json(BridgeIngressAck {
                    status: BridgeIngressStatus::Ingested,
                    message_id: Some(message.id.to_string()),
                    reason: None,
                }),
            ))
        }
        Err(crate::bridges::BridgeError::Ignored { reason }) => Ok((
            StatusCode::OK,
            Json(BridgeIngressAck {
                status: BridgeIngressStatus::Ignored,
                message_id: None,
                reason: Some(reason.to_string()),
            }),
        )),
        Err(error) => Err(ApiError::new(StatusCode::BAD_REQUEST, error.to_string())),
    }
}
