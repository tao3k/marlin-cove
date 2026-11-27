// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use axum::routing::post;
use marlin_cove::config::{
    CoveConfig, ExternalBotId, FeatureToggle, MentionPolicy, SecretValue, ServiceUrl,
};
use marlin_cove::marlin_gateway::{MarlinGatewayTurnRequest, MarlinGatewayTurnResponse};
use marlin_cove::protocol::{
    BridgeOrigin, GateDecision, PrincipalKind, PrincipalRole, RoomGate, RoomGovernanceEnvelope,
    RoomInvite, RoomParticipant, RoomSnapshot,
};
use marlin_cove::relay::{
    BridgeIngressAck, BridgeIngressStatus, CoveServerState, DispatchTurnResponse, HealthResponse,
    ServiceHealthStatus, build_router,
};
use pretty_assertions::assert_eq;
use serde_json::json;
use tokio::net::TcpListener;
use tower::ServiceExt;

fn test_router() -> Router {
    let mut config = CoveConfig::default();
    config.telegram.state = FeatureToggle::Enabled;
    config.telegram.webhook_secret = SecretValue::new("tg-sec");
    config.discord.state = FeatureToggle::Enabled;
    config.discord.ingress_secret = SecretValue::new("dc-sec");
    config.discord.bot_user_id = ExternalBotId::new("bot-77");
    config.discord.mention_policy = MentionPolicy::Required;
    build_router(CoveServerState::from_config(config))
}

async fn json_request(
    app: &Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
    payload: Option<serde_json::Value>,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (key, value) in headers {
        builder = builder.header(*key, *value);
    }
    let request = if let Some(value) = payload {
        builder
            .header("content-type", "application/json")
            .body(Body::from(value.to_string()))
            .unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    };
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, bytes.to_vec())
}

#[tokio::test]
async fn cove_relay_invites_prevent_hijack_and_privilege_escalation() {
    let app = test_router();

    let (status, body) = json_request(&app, "GET", "/v1/health", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    let health: HealthResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(health.status, ServiceHealthStatus::Ok);

    // Non-human agent cannot issue invites.
    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/invites",
        &[],
        Some(json!({
            "issued_by": "agent:architect",
            "grant_role": "member",
            "grant_scopes": ["public"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Owner cannot grant owner role or escalate external_guest scopes beyond public.
    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/invites",
        &[],
        Some(json!({
            "issued_by": "human:owner",
            "grant_role": "external_guest",
            "grant_scopes": ["org:core"]
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    // Valid guest invite issued by human owner.
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/invites",
        &[],
        Some(json!({
            "issued_by": "human:owner",
            "grant_role": "external_guest",
            "grant_scopes": ["public"],
            "max_uses": 2
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let invite: RoomInvite = serde_json::from_slice(&body).unwrap();
    let join_uri = format!("/v1/invites/{}/join", invite.token.as_str());

    // Quint-caught regression: redeeming invite MUST NOT overwrite existing human:owner.
    let (status, _) = json_request(
        &app,
        "POST",
        &join_uri,
        &[],
        Some(json!({
            "principal_id": "human:owner",
            "display_name": "Hijacker",
            "handle": "hijack",
            "kind": "human"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    // Legitimate guest joins once; governance envelope carries tainted_external=true.
    let (status, body) = json_request(
        &app,
        "POST",
        &join_uri,
        &[],
        Some(json!({
            "principal_id": "human:guest-1",
            "display_name": "Guest One",
            "handle": "guest1",
            "kind": "human"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let guest: RoomParticipant = serde_json::from_slice(&body).unwrap();
    assert_eq!(guest.role, PrincipalRole::ExternalGuest);

    let (status, env_bytes) = json_request(
        &app,
        "GET",
        "/v1/rooms/cove-main/governance/human:guest-1",
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let guest_env: RoomGovernanceEnvelope = serde_json::from_slice(&env_bytes).unwrap();
    assert!(guest_env.tainted_external);
}

#[tokio::test]
async fn cove_relay_bridges_fail_closed_without_secret_and_deduplicate_replays() {
    let app = test_router();

    #[cfg(feature = "telegram")]
    {
        let tg_payload = json!({
            "update_id": 100,
            "message": {
                "message_id": 55,
                "from": { "id": 888, "username": "tg_alice", "first_name": "Alice" },
                "chat": { "id": -1001, "type": "supergroup", "title": "Marlin TG" },
                "date": 1_700_000_500,
                "text": "@reviewer check mobile swipe"
            }
        });
        let (status, _) = json_request(
            &app,
            "POST",
            "/v1/bridges/telegram/webhook",
            &[("x-telegram-bot-api-secret-token", "tg-sec")],
            Some(tg_payload.clone()),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);

        // Replaying the exact same Telegram message id must deduplicate cleanly.
        let (status, replay_bytes) = json_request(
            &app,
            "POST",
            "/v1/bridges/telegram/webhook",
            &[("x-telegram-bot-api-secret-token", "tg-sec")],
            Some(tg_payload),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let ack: BridgeIngressAck = serde_json::from_slice(&replay_bytes).unwrap();
        assert_eq!(ack.status, BridgeIngressStatus::Duplicate);

        // Unconfigured webhook secret MUST fail closed (401 Unauthorized).
        let unconfigured = build_router(CoveServerState::from_config(CoveConfig::default()));
        let (status, _) = json_request(
            &unconfigured,
            "POST",
            "/v1/bridges/telegram/webhook",
            &[],
            Some(json!({ "update_id": 1 })),
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }

    #[cfg(feature = "discord")]
    {
        let (status, _) = json_request(
            &app,
            "POST",
            "/v1/bridges/discord/ingress",
            &[("x-marlin-cove-ingress-secret", "dc-sec")],
            Some(json!({
                "message_id": "dc-msg-1",
                "guild_id": "guild-1",
                "channel_id": "chan-1",
                "author_id": "user-9",
                "author_name": "dc_bob",
                "author_is_bot": false,
                "content": "<@bot-77> status update please",
                "mentioned_user_ids": ["bot-77"],
                "timestamp_unix": 1_700_000_600
            })),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
    }

    let (status, body) = json_request(&app, "GET", "/v1/rooms/cove-main/snapshot", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    let snapshot: RoomSnapshot = serde_json::from_slice(&body).unwrap();
    #[cfg(feature = "telegram")]
    assert_eq!(
        snapshot
            .messages
            .iter()
            .filter(|msg| msg.origin == BridgeOrigin::Telegram)
            .count(),
        1
    );
    #[cfg(feature = "discord")]
    assert!(
        snapshot
            .messages
            .iter()
            .any(|msg| msg.origin == BridgeOrigin::Discord)
    );
}

#[tokio::test]
async fn cove_relay_enforces_tier0_gate_immutability_capacity_and_auth() {
    let mut config = CoveConfig::default();
    config.relay.auth_token = SecretValue::new("cove-secret-99");
    config.relay.limits.max_messages_per_room = 2;
    let app = build_router(CoveServerState::from_config(config));
    let auth = [("authorization", "Bearer cove-secret-99")];

    // Protected reads MUST reject missing bearer tokens.
    let (status, _) = json_request(&app, "GET", "/v1/rooms/cove-main/snapshot", &[], None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // Bounded capacity (CapacityModel): 2 messages succeed, 3rd returns 429.
    for idx in 0..2 {
        let (status, _) = json_request(
            &app,
            "POST",
            "/v1/rooms/cove-main/messages",
            &auth,
            Some(json!({
                "channel_id": "architecture",
                "author_id": "human:owner",
                "body": format!("msg-{idx}")
            })),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED);
    }
    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/messages",
        &auth,
        Some(json!({
            "channel_id": "architecture",
            "author_id": "human:owner",
            "body": "overflow"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);

    // Bot-loop depth guard.
    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/messages",
        &auth,
        Some(json!({
            "channel_id": "architecture",
            "author_id": "agent:architect",
            "body": "loop",
            "bot_chain_depth": 5
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Tier-0 Gate: Agent rejected (403), Human Owner approves (200), replay rejected (409).
    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/gates",
        &auth,
        Some(json!({
            "channel_id": "security",
            "requested_by": "agent:architect",
            "summary": "Deploy relay config",
            "capability": "shell:deploy"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let gate: RoomGate = serde_json::from_slice(&body).unwrap();
    assert_eq!(gate.decision, GateDecision::Pending);

    let resolve_uri = format!("/v1/rooms/cove-main/gates/{}/resolve", gate.id.as_str());
    let (status, _) = json_request(
        &app,
        "POST",
        &resolve_uri,
        &auth,
        Some(json!({ "actor_id": "agent:architect", "approved": true })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = json_request(
        &app,
        "POST",
        &resolve_uri,
        &auth,
        Some(json!({ "actor_id": "human:owner", "approved": true, "reason": "ok" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = json_request(
        &app,
        "POST",
        &resolve_uri,
        &auth,
        Some(json!({ "actor_id": "human:owner", "approved": false, "reason": "replay" })),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn cove_relay_live_dispatches_turn_to_marlin_gateway_with_governance_envelope() {
    // Spawn a real local TCP mock Marlin Gateway (`marlin-agent-core`) endpoint.
    let gateway_app = Router::new().route(
        "/message",
        post(
            |axum::Json(req): axum::Json<MarlinGatewayTurnRequest>| async move {
                let reply = format!(
                    "[Marlin Gateway] role={:?} actor={} tainted={} reply=ok",
                    req.governance.actor_role,
                    req.governance.actor_id.as_str(),
                    req.governance.tainted_external
                );
                axum::Json(MarlinGatewayTurnResponse { response: reply })
            },
        ),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway_addr = listener.local_addr().unwrap();
    let server_handle = tokio::spawn(async move {
        let _ = axum::serve(listener, gateway_app).await;
    });

    let mut config = CoveConfig::default();
    config.marlin_gateway.base_url =
        ServiceUrl::new("marlin_gateway.base_url", format!("http://{gateway_addr}")).unwrap();
    let app = build_router(CoveServerState::from_config(config));

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/dispatch",
        &[],
        Some(json!({
            "channel_id": "architecture",
            "author_id": "human:owner",
            "target_agent_handle": "architect",
            "body": "@architect verify Marlin Gateway handoff"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let dispatched: DispatchTurnResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(dispatched.governance.actor_role, PrincipalRole::Owner);
    assert!(!dispatched.governance.tainted_external);
    assert_eq!(dispatched.agent_reply.origin, BridgeOrigin::MarlinGateway);
    assert_eq!(dispatched.agent_reply.author_kind, PrincipalKind::Agent);
    assert_eq!(dispatched.agent_reply.bot_chain_depth, 1);
    assert!(
        dispatched
            .agent_reply
            .body
            .contains("[Marlin Gateway] role=Owner actor=human:owner tainted=false")
    );

    let (status, snap_bytes) =
        json_request(&app, "GET", "/v1/rooms/cove-main/snapshot", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    let snapshot: RoomSnapshot = serde_json::from_slice(&snap_bytes).unwrap();
    assert_eq!(snapshot.messages.len(), 2);

    server_handle.abort();
}
