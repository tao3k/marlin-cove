// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Cloud Cove HTTP + SSE router, hardened security boundaries, and live Marlin
//! Gateway dispatch.

use std::convert::Infallible;
use std::sync::Arc;

use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::{AllowOrigin, CorsLayer};

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
use crate::bridges::verify_shared_secret;
use crate::config::{CoveConfig, SecretValue, compiled_features};
use crate::marlin_gateway::{MarlinGatewayClient, MarlinGatewayTurnRequest};
use crate::protocol::{BridgeOrigin, GateId, PrincipalId, ProtocolError, RoomId, SessionKey};
use crate::relay::requests::{
    CreateInviteRequest, CreateRoomRequest, DispatchTurnRequest, DispatchTurnResponse,
    PostMessageRequest, RedeemInviteRequest, RelayError, RequestGateRequest, ResolveGateRequest,
};
use crate::relay::store::CoveRelayStore;

/// Shared runtime state mounted on the Axum router.
#[derive(Clone)]
pub struct CoveServerState {
    /// Active Cove configuration.
    pub config: Arc<CoveConfig>,
    /// Thread-safe Cove room/gate/membership store.
    pub store: CoveRelayStore,
    /// Outbound typed client targeting Marlin Gateway (`marlin-agent-core`).
    pub marlin_gateway: Option<MarlinGatewayClient>,
}

impl CoveServerState {
    /// Build server state from a [`CoveConfig`].
    #[must_use]
    pub fn from_config(config: CoveConfig) -> Self {
        let store = CoveRelayStore::from_config(&config);
        let marlin_gateway = MarlinGatewayClient::new(
            config.marlin_gateway.base_url.as_str(),
            config
                .marlin_gateway
                .bearer_token
                .as_ref()
                .map(|secret| secret.as_str().to_string()),
        )
        .ok();
        Self {
            config: Arc::new(config),
            store,
            marlin_gateway,
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
    /// Message was intentionally ignored by bridge policy.
    Ignored,
    /// Replayed external message id was deduplicated idempotently.
    Duplicate,
}

/// Bridge webhook acknowledgement returned for Telegram/Discord ingress calls.
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

impl From<ProtocolError> for ApiError {
    fn from(error: ProtocolError) -> Self {
        let status = match error {
            ProtocolError::EmptyIdentifier { .. } => StatusCode::BAD_REQUEST,
            ProtocolError::HumanApprovalRequired { .. } => StatusCode::FORBIDDEN,
        };
        Self::new(status, error.to_string())
    }
}

/// Build the hardened Axum router with all compiled Cargo feature routes mounted.
pub fn build_router(state: CoveServerState) -> Router {
    let max_body_bytes = state.config.relay.limits.max_body_bytes;
    let allowed_origins = state.config.relay.allowed_origins.clone();

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
        .route("/v1/rooms/{room_id}/dispatch", post(dispatch_turn_handler))
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

    let router = router
        .layer(DefaultBodyLimit::max(max_body_bytes))
        .with_state(state);

    if allowed_origins.is_empty() {
        router
    } else {
        let origins: Vec<HeaderValue> = allowed_origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin.trim()).ok())
            .collect();
        let cors = CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([Method::GET, Method::POST]);
        router.layer(cors)
    }
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
    let expected = config.relay.auth_token.as_ref().map(SecretValue::as_str);
    let provided = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "));

    if verify_shared_secret(expected, provided) {
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
    headers: HeaderMap,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
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
    headers: HeaderMap,
    Path(room_id): Path<String>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let room_id = RoomId::new(room_id)?;
    Ok(Json(state.store.room_snapshot(&room_id)?))
}

async fn governance_envelope_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Path((room_id, principal_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let room_id = RoomId::new(room_id)?;
    let principal_id = PrincipalId::new(principal_id)?;
    let channel_id = state.config.relay.default_channel_id.clone();
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

async fn dispatch_turn_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
    Json(request): Json<DispatchTurnRequest>,
) -> Result<impl IntoResponse, ApiError> {
    check_relay_auth(&state.config, &headers)?;
    let room_id = RoomId::new(room_id)?;
    let gateway = state.marlin_gateway.as_ref().ok_or_else(|| {
        ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "Marlin Gateway client is not configured",
        )
    })?;
    let agent_id = state
        .store
        .find_agent_by_handle(&room_id, &request.target_agent_handle)?;
    let governance = state.store.governance_envelope(
        &room_id,
        request.channel_id.clone(),
        &request.author_id,
        BridgeOrigin::CoveNative,
    )?;

    let user_message = state.store.post_message(
        &room_id,
        PostMessageRequest {
            channel_id: request.channel_id.clone(),
            thread_root_id: None,
            author_id: request.author_id.clone(),
            body: request.body.clone(),
            gate_id: None,
            attachments: Vec::new(),
            origin: BridgeOrigin::CoveNative,
            bot_chain_depth: 0,
        },
    )?;

    let session_key = SessionKey::new(format!(
        "cove:{}:{}:{}",
        room_id.as_str(),
        request.channel_id.as_str(),
        request.target_agent_handle.trim_start_matches('@')
    ))?;
    let gateway_req = MarlinGatewayTurnRequest {
        session_id: session_key,
        governance: governance.clone(),
        target_agent_handle: Some(request.target_agent_handle),
        message: request.body,
        attachments: Vec::new(),
    };
    let gateway_reply = gateway
        .dispatch_turn(&gateway_req)
        .await
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;

    let agent_reply = state.store.post_message(
        &room_id,
        PostMessageRequest {
            channel_id: request.channel_id,
            thread_root_id: Some(user_message.id.clone()),
            author_id: agent_id,
            body: gateway_reply.response,
            gate_id: None,
            attachments: Vec::new(),
            origin: BridgeOrigin::MarlinGateway,
            bot_chain_depth: 1,
        },
    )?;

    Ok((
        StatusCode::CREATED,
        Json(DispatchTurnResponse {
            user_message,
            governance,
            agent_reply,
        }),
    ))
}

async fn room_events_sse_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Path(room_id): Path<String>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    check_relay_auth(&state.config, &headers)?;
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
    let Some(expected) = state
        .config
        .telegram
        .webhook_secret
        .as_ref()
        .map(SecretValue::as_str)
        .filter(|secret| !secret.trim().is_empty())
    else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "telegram webhook secret is not configured (fail-closed)",
        ));
    };
    let provided = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|value| value.to_str().ok());
    if !verify_telegram_webhook_secret(Some(expected), provided) {
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
        Err(error) => Err(ApiError::new(StatusCode::BAD_REQUEST, error.to_string())),
    }
}

#[cfg(feature = "discord")]
async fn discord_ingress_handler(
    State(state): State<CoveServerState>,
    headers: HeaderMap,
    Json(payload): Json<DiscordIngressPayload>,
) -> Result<impl IntoResponse, ApiError> {
    let Some(expected) = state
        .config
        .discord
        .ingress_secret
        .as_ref()
        .map(SecretValue::as_str)
        .filter(|secret| !secret.trim().is_empty())
    else {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "discord ingress secret is not configured (fail-closed)",
        ));
    };
    let provided = headers
        .get("x-marlin-cove-ingress-secret")
        .and_then(|value| value.to_str().ok());
    if !verify_discord_ingress_secret(Some(expected), provided) {
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
        Err(error) => Err(ApiError::new(StatusCode::BAD_REQUEST, error.to_string())),
    }
}

#[cfg(any(feature = "telegram", feature = "discord"))]
fn ingest_bridge_to_response(
    store: &CoveRelayStore,
    inbound: crate::bridges::InboundBridgeMessage,
) -> Result<(StatusCode, Json<BridgeIngressAck>), ApiError> {
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
        Err(error) => Err(ApiError::from(error)),
    }
}
