// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use marlin_cove::config::{CoveConfig, ExternalBotId, FeatureToggle, MentionPolicy, SecretValue};
use marlin_cove::protocol::{
    BridgeOrigin, GateDecision, PrincipalKind, PrincipalRole, RoomGate, RoomGovernanceEnvelope,
    RoomInvite, RoomMessage, RoomParticipant, RoomSnapshot,
};
use marlin_cove::relay::{CoveServerState, HealthResponse, ServiceHealthStatus, build_router};
use pretty_assertions::assert_eq;
use serde_json::json;
use tower::ServiceExt;

fn test_router() -> axum::Router {
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
    app: &axum::Router,
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
async fn cove_relay_invites_and_governance_envelopes() {
    let app = test_router();

    let (status, body) = json_request(&app, "GET", "/v1/health", &[], None).await;
    assert_eq!(status, StatusCode::OK);
    let health: HealthResponse = serde_json::from_slice(&body).unwrap();
    assert_eq!(health.status, ServiceHealthStatus::Ok);
    assert_eq!(health.service, "marlin-cove");
    assert!(health.features.contains(&"relay".to_string()));

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/invites",
        &[],
        Some(json!({
            "issued_by": "human:owner",
            "grant_role": "external_guest",
            "grant_scopes": ["public"],
            "max_uses": 1
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let invite: RoomInvite = serde_json::from_slice(&body).unwrap();

    let join_uri = format!("/v1/invites/{}/join", invite.token.as_str());
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

    let (status, _) = json_request(
        &app,
        "POST",
        &join_uri,
        &[],
        Some(json!({
            "principal_id": "human:guest-2",
            "display_name": "Guest Two",
            "handle": "guest2",
            "kind": "human"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, guest_env_bytes) = json_request(
        &app,
        "GET",
        "/v1/rooms/cove-main/governance/human:guest-1",
        &[],
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let guest_env: RoomGovernanceEnvelope = serde_json::from_slice(&guest_env_bytes).unwrap();
    assert!(guest_env.tainted_external);
    assert_eq!(guest_env.actor_role, PrincipalRole::ExternalGuest);
}

#[tokio::test]
async fn cove_relay_ingests_telegram_and_discord_bridges() {
    let app = test_router();

    #[cfg(feature = "telegram")]
    {
        let (status, _) = json_request(
            &app,
            "POST",
            "/v1/bridges/telegram/webhook",
            &[("x-telegram-bot-api-secret-token", "tg-sec")],
            Some(json!({
                "update_id": 100,
                "message": {
                    "message_id": 55,
                    "from": { "id": 888, "username": "tg_alice", "first_name": "Alice" },
                    "chat": { "id": -1001, "type": "supergroup", "title": "Marlin TG" },
                    "date": 1_700_000_500,
                    "text": "@reviewer check mobile swipe"
                }
            })),
        )
        .await;
        assert_eq!(status, StatusCode::ACCEPTED);
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
    assert!(
        snapshot
            .participants
            .iter()
            .any(|p| p.id.as_str() == "agent:architect" && p.kind == PrincipalKind::Agent)
    );
    #[cfg(feature = "telegram")]
    assert!(
        snapshot
            .messages
            .iter()
            .any(|msg| msg.origin == BridgeOrigin::Telegram)
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
async fn cove_relay_enforces_bot_loop_limit_and_human_only_tier0_gate() {
    let app = test_router();

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/messages",
        &[],
        Some(json!({
            "channel_id": "architecture",
            "author_id": "human:owner",
            "body": "@architect please inspect the Cove protocol"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let posted: RoomMessage = serde_json::from_slice(&body).unwrap();
    assert_eq!(posted.mentions, vec!["architect"]);

    let (status, _) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/messages",
        &[],
        Some(json!({
            "channel_id": "architecture",
            "author_id": "agent:architect",
            "body": "infinite loop attempt",
            "bot_chain_depth": 5
        })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, body) = json_request(
        &app,
        "POST",
        "/v1/rooms/cove-main/gates",
        &[],
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
        &[],
        Some(json!({
            "actor_id": "agent:architect",
            "approved": true,
            "reason": "bot trying to self-approve"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = json_request(
        &app,
        "POST",
        &resolve_uri,
        &[],
        Some(json!({
            "actor_id": "human:owner",
            "approved": true,
            "reason": "human reviewed and approved"
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}
