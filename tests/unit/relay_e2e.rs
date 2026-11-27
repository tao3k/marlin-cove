// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

use std::pin::Pin;

use marlin_cove::config::{CoveConfig, SecretValue, ServiceUrl};
use marlin_cove::marlin_gateway::{
    COVE_META_ACTOR_ID, COVE_META_ACTOR_ROLE, COVE_META_TAINTED_EXTERNAL,
};
use marlin_cove::proto_wire::app_v1;
use marlin_cove::proto_wire::app_v1::app_server_server::{AppServer, AppServerServer};
use marlin_cove::proto_wire::cove_v1;
use marlin_cove::proto_wire::cove_v1::cove_relay_service_client::CoveRelayServiceClient;
use marlin_cove::relay::{CoveServerState, build_grpc_service};
use pretty_assertions::assert_eq;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tokio_stream::{Stream, StreamExt};
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;
use tonic::{Code, Request, Response, Status};

async fn spawn_grpc_relay(
    config: CoveConfig,
) -> (
    CoveRelayServiceClient<Channel>,
    CoveServerState,
    tokio::task::JoinHandle<()>,
) {
    let state = CoveServerState::from_config(config);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let incoming = TcpListenerStream::new(listener);
    let svc = build_grpc_service(state.clone());

    let handle = tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(svc)
            .serve_with_incoming(incoming)
            .await;
    });

    let client = CoveRelayServiceClient::connect(format!("http://{addr}"))
        .await
        .unwrap();
    (client, state, handle)
}

fn with_bearer<T>(payload: T, token: &str) -> Request<T> {
    let mut req = Request::new(payload);
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}").as_str()).unwrap(),
    );
    req
}

type TurnEventStream =
    Pin<Box<dyn Stream<Item = Result<app_v1::TurnEvent, Status>> + Send + 'static>>;
type SessionTurnEventStream =
    Pin<Box<dyn Stream<Item = Result<app_v1::SessionTurnEvent, Status>> + Send + 'static>>;
type WorkbenchTabsStream =
    Pin<Box<dyn Stream<Item = Result<app_v1::WorkbenchTabsResponse, Status>> + Send + 'static>>;

struct MockMarlinAppServerGrpc {
    expected_token: String,
}

#[tonic::async_trait]
impl AppServer for MockMarlinAppServerGrpc {
    type StreamTurnStream = TurnEventStream;
    type WatchSessionTurnStream = SessionTurnEventStream;
    type WatchWorkbenchTabsStream = WorkbenchTabsStream;

    async fn get_capabilities(
        &self,
        _request: Request<app_v1::GetCapabilitiesRequest>,
    ) -> Result<Response<app_v1::GetCapabilitiesResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn stream_turn(
        &self,
        request: Request<app_v1::StreamTurnRequest>,
    ) -> Result<Response<Self::StreamTurnStream>, Status> {
        let provided = request
            .metadata()
            .get("authorization")
            .and_then(|v| v.to_str().ok());
        if provided != Some(format!("Bearer {}", self.expected_token).as_str()) {
            return Err(Status::unauthenticated("invalid gateway token"));
        }
        let role = request
            .metadata()
            .get(COVE_META_ACTOR_ROLE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let actor = request
            .metadata()
            .get(COVE_META_ACTOR_ID)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let tainted = request
            .metadata()
            .get(COVE_META_TAINTED_EXTERNAL)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let prompt_len = request.into_inner().messages.len();
        let part1 = format!("[Marlin AppServer gRPC] role={role} actor={actor} ");
        let part2 = format!("tainted={tainted} msgs={prompt_len} reply=ok");
        let events = vec![
            Ok(app_v1::TurnEvent {
                event: Some(app_v1::turn_event::Event::Started(app_v1::TurnStarted {})),
            }),
            Ok(app_v1::TurnEvent {
                event: Some(app_v1::turn_event::Event::TextDelta(app_v1::TextDelta {
                    sequence: 1,
                    text: part1.clone(),
                })),
            }),
            Ok(app_v1::TurnEvent {
                event: Some(app_v1::turn_event::Event::TextDelta(app_v1::TextDelta {
                    sequence: 2,
                    text: part2.clone(),
                })),
            }),
            Ok(app_v1::TurnEvent {
                event: Some(app_v1::turn_event::Event::Completed(
                    app_v1::TurnCompleted {
                        text_chunks: 2,
                        text_bytes: (part1.len() + part2.len()) as u64,
                    },
                )),
            }),
        ];
        Ok(Response::new(Box::pin(tokio_stream::iter(events))))
    }

    async fn start_session_turn(
        &self,
        _request: Request<app_v1::StartSessionTurnRequest>,
    ) -> Result<Response<app_v1::StartSessionTurnResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn watch_session_turn(
        &self,
        _request: Request<app_v1::WatchSessionTurnRequest>,
    ) -> Result<Response<Self::WatchSessionTurnStream>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn cancel_session_turn(
        &self,
        _request: Request<app_v1::CancelSessionTurnRequest>,
    ) -> Result<Response<app_v1::CancelSessionTurnResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn open_session(
        &self,
        _request: Request<app_v1::OpenSessionRequest>,
    ) -> Result<Response<app_v1::OpenSessionResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn get_session_summary(
        &self,
        _request: Request<app_v1::GetSessionSummaryRequest>,
    ) -> Result<Response<app_v1::GetSessionSummaryResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn get_session_turn_status(
        &self,
        _request: Request<app_v1::GetSessionTurnStatusRequest>,
    ) -> Result<Response<app_v1::GetSessionTurnStatusResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn get_session_turn_cancellation(
        &self,
        _request: Request<app_v1::GetSessionTurnCancellationRequest>,
    ) -> Result<Response<app_v1::GetSessionTurnCancellationResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn register_task_actor(
        &self,
        _request: Request<app_v1::RegisterTaskActorRequest>,
    ) -> Result<Response<app_v1::TaskActorCursorResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn dispatch_task_actor(
        &self,
        _request: Request<app_v1::DispatchTaskActorRequest>,
    ) -> Result<Response<app_v1::DispatchTaskActorResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn reconcile_task_receipt(
        &self,
        _request: Request<app_v1::ReconcileTaskReceiptRequest>,
    ) -> Result<Response<app_v1::ReconcileTaskReceiptResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn bind_governed_context(
        &self,
        _request: Request<app_v1::BindGovernedContextRequest>,
    ) -> Result<Response<app_v1::BindGovernedContextResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn get_workbench_tabs(
        &self,
        _request: Request<app_v1::GetWorkbenchTabsRequest>,
    ) -> Result<Response<app_v1::WorkbenchTabsResponse>, Status> {
        Err(Status::unimplemented("mock"))
    }

    async fn watch_workbench_tabs(
        &self,
        _request: Request<app_v1::WatchWorkbenchTabsRequest>,
    ) -> Result<Response<Self::WatchWorkbenchTabsStream>, Status> {
        Err(Status::unimplemented("mock"))
    }
}

#[tokio::test]
async fn cove_grpc_invites_prevent_hijack_and_privilege_escalation() {
    let (mut client, _state, handle) = spawn_grpc_relay(CoveConfig::default()).await;

    let health = client
        .health(cove_v1::HealthRequest {})
        .await
        .unwrap()
        .into_inner();
    assert_eq!(health.status, cove_v1::ServiceHealthStatus::Ok as i32);
    assert_eq!(health.service, "marlin-cove");

    // Non-human agent cannot issue invites.
    let err = client
        .create_invite(cove_v1::CreateInviteRequest {
            room_id: "cove-main".to_string(),
            issued_by: "agent:architect".to_string(),
            grant_role: cove_v1::PrincipalRole::Member as i32,
            grant_scopes: vec!["public".to_string()],
            max_uses: 1,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);

    // Owner cannot escalate external_guest scopes beyond public.
    let err = client
        .create_invite(cove_v1::CreateInviteRequest {
            room_id: "cove-main".to_string(),
            issued_by: "human:owner".to_string(),
            grant_role: cove_v1::PrincipalRole::ExternalGuest as i32,
            grant_scopes: vec!["org:core".to_string()],
            max_uses: 1,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);

    // Valid guest invite issued by human owner.
    let invite = client
        .create_invite(cove_v1::CreateInviteRequest {
            room_id: "cove-main".to_string(),
            issued_by: "human:owner".to_string(),
            grant_role: cove_v1::PrincipalRole::ExternalGuest as i32,
            grant_scopes: vec!["public".to_string()],
            max_uses: 2,
        })
        .await
        .unwrap()
        .into_inner();

    // Quint-caught regression: redeeming invite MUST NOT overwrite existing human:owner.
    let err = client
        .redeem_invite(cove_v1::RedeemInviteRequest {
            token: invite.token.clone(),
            principal_id: "human:owner".to_string(),
            display_name: "Hijacker".to_string(),
            handle: "hijack".to_string(),
            kind: cove_v1::PrincipalKind::Human as i32,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::AlreadyExists);

    // Legitimate guest joins once; governance envelope carries tainted_external=true.
    let guest = client
        .redeem_invite(cove_v1::RedeemInviteRequest {
            token: invite.token,
            principal_id: "human:guest-1".to_string(),
            display_name: "Guest One".to_string(),
            handle: "guest1".to_string(),
            kind: cove_v1::PrincipalKind::Human as i32,
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(guest.role, cove_v1::PrincipalRole::ExternalGuest as i32);

    let guest_env = client
        .get_governance_envelope(cove_v1::GetGovernanceEnvelopeRequest {
            room_id: "cove-main".to_string(),
            principal_id: "human:guest-1".to_string(),
            channel_id: String::new(),
        })
        .await
        .unwrap()
        .into_inner();
    assert!(guest_env.tainted_external);

    handle.abort();
}

fn msg_req(author: &str, body: &str, depth: u32) -> cove_v1::PostMessageRequest {
    cove_v1::PostMessageRequest {
        room_id: "cove-main".to_string(),
        channel_id: "architecture".to_string(),
        thread_root_id: String::new(),
        author_id: author.to_string(),
        body: body.to_string(),
        gate_id: String::new(),
        attachments: Vec::new(),
        origin: cove_v1::BridgeOrigin::CoveNative as i32,
        bot_chain_depth: depth,
    }
}

#[tokio::test]
async fn cove_grpc_enforces_capacity_auth_loop_guard_and_streaming() {
    let mut config = CoveConfig::default();
    config.relay.auth_token = SecretValue::new("cove-secret-99");
    config.relay.limits.max_messages_per_room = 2;
    let (mut client, _state, handle) = spawn_grpc_relay(config).await;

    // Protected reads MUST reject missing bearer tokens.
    let err = client
        .get_room_snapshot(cove_v1::GetRoomSnapshotRequest {
            room_id: "cove-main".to_string(),
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::Unauthenticated);

    // Open HTTP/2 server-streaming WatchRoomEvents subscription.
    let mut stream_client = client.clone();
    let mut event_stream = stream_client
        .watch_room_events(with_bearer(
            cove_v1::WatchRoomEventsRequest {
                room_id: "cove-main".to_string(),
            },
            "cove-secret-99",
        ))
        .await
        .unwrap()
        .into_inner();

    // Bounded capacity (CapacityModel): 2 messages succeed, 3rd returns ResourceExhausted.
    for idx in 0..2 {
        client
            .post_message(with_bearer(
                msg_req("human:owner", &format!("msg-{idx}"), 0),
                "cove-secret-99",
            ))
            .await
            .unwrap();
    }
    let err = client
        .post_message(with_bearer(
            msg_req("human:owner", "overflow", 0),
            "cove-secret-99",
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::ResourceExhausted);

    // Verify the live gRPC HTTP/2 stream received our posted message event!
    let first_event = event_stream.next().await.unwrap().unwrap();
    assert_eq!(first_event.seq, 1);
    assert!(matches!(
        first_event.kind,
        Some(cove_v1::room_event::Kind::MessagePosted(_))
    ));

    // Bot-loop depth guard.
    let err = client
        .post_message(with_bearer(
            msg_req("agent:architect", "loop", 5),
            "cove-secret-99",
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::InvalidArgument);

    handle.abort();
}

#[tokio::test]
async fn cove_grpc_enforces_tier0_gate_immutability() {
    let mut config = CoveConfig::default();
    config.relay.auth_token = SecretValue::new("cove-secret-99");
    let (mut client, _state, handle) = spawn_grpc_relay(config).await;

    // Tier-0 Gate: Agent rejected (PermissionDenied), Human Owner approves (Ok), replay rejected (AlreadyExists).
    let gate = client
        .request_gate(with_bearer(
            cove_v1::RequestGateRequest {
                room_id: "cove-main".to_string(),
                channel_id: "security".to_string(),
                requested_by: "agent:architect".to_string(),
                summary: "Deploy relay config".to_string(),
                capability: "shell:deploy".to_string(),
            },
            "cove-secret-99",
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(gate.decision, cove_v1::GateDecision::Pending as i32);

    let err = client
        .resolve_gate(with_bearer(
            cove_v1::ResolveGateRequest {
                room_id: "cove-main".to_string(),
                gate_id: gate.id.clone(),
                actor_id: "agent:architect".to_string(),
                approved: true,
                reason: "self".to_string(),
            },
            "cove-secret-99",
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::PermissionDenied);

    let receipt = client
        .resolve_gate(with_bearer(
            cove_v1::ResolveGateRequest {
                room_id: "cove-main".to_string(),
                gate_id: gate.id.clone(),
                actor_id: "human:owner".to_string(),
                approved: true,
                reason: "ok".to_string(),
            },
            "cove-secret-99",
        ))
        .await
        .unwrap()
        .into_inner();
    assert_eq!(receipt.decision, cove_v1::GateDecision::Approved as i32);

    let err = client
        .resolve_gate(with_bearer(
            cove_v1::ResolveGateRequest {
                room_id: "cove-main".to_string(),
                gate_id: gate.id,
                actor_id: "human:owner".to_string(),
                approved: false,
                reason: "replay".to_string(),
            },
            "cove-secret-99",
        ))
        .await
        .unwrap_err();
    assert_eq!(err.code(), Code::AlreadyExists);

    handle.abort();
}

#[tokio::test]
async fn cove_grpc_live_dispatches_turn_to_marlin_gateway_over_http2() {
    let gw_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gw_addr = gw_listener.local_addr().unwrap();
    let gw_incoming = TcpListenerStream::new(gw_listener);
    let gw_handle = tokio::spawn(async move {
        let _ = tonic::transport::Server::builder()
            .add_service(AppServerServer::new(MockMarlinAppServerGrpc {
                expected_token: "gw-grpc-secret".to_string(),
            }))
            .serve_with_incoming(gw_incoming)
            .await;
    });

    let mut config = CoveConfig::default();
    config.marlin_gateway.grpc_endpoint =
        ServiceUrl::new("marlin_gateway.grpc_endpoint", format!("http://{gw_addr}")).unwrap();
    config.marlin_gateway.bearer_token = SecretValue::new("gw-grpc-secret");

    let (mut client, _state, cove_handle) = spawn_grpc_relay(config).await;

    let dispatched = client
        .dispatch_turn(cove_v1::DispatchTurnRequest {
            room_id: "cove-main".to_string(),
            channel_id: "architecture".to_string(),
            author_id: "human:owner".to_string(),
            target_agent_handle: "architect".to_string(),
            body: "@architect verify Marlin AppServer gRPC handoff".to_string(),
            thread_root_id: String::new(),
        })
        .await
        .unwrap()
        .into_inner();

    let gov = dispatched.governance.unwrap();
    let user_msg = dispatched.user_message.unwrap();
    let reply = dispatched.agent_reply.unwrap();
    assert_eq!(gov.actor_role, cove_v1::PrincipalRole::Owner as i32);
    assert!(!gov.tainted_external);
    assert_eq!(reply.origin, cove_v1::BridgeOrigin::MarlinGateway as i32);
    assert_eq!(reply.author_kind, cove_v1::PrincipalKind::Agent as i32);
    assert_eq!(reply.bot_chain_depth, 1);
    assert_eq!(
        reply.thread_root_id, user_msg.id,
        "group channel @mention replies must thread under the triggering user message"
    );
    assert!(reply.body.contains(
        "[Marlin AppServer gRPC] role=owner actor=human:owner tainted=false msgs=1 reply=ok"
    ));

    let snapshot = client
        .get_room_snapshot(cove_v1::GetRoomSnapshotRequest {
            room_id: "cove-main".to_string(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(snapshot.messages.len(), 2);

    cove_handle.abort();
    gw_handle.abort();
}

#[cfg(any(feature = "telegram", feature = "discord"))]
#[tokio::test]
async fn external_webhooks_fail_closed_and_deduplicate_replays() {
    use axum::body::{Body, to_bytes};
    use axum::http::{Request as HttpRequest, StatusCode};
    use marlin_cove::config::{ExternalBotId, FeatureToggle, MentionPolicy};
    use marlin_cove::relay::{BridgeIngressAck, BridgeIngressStatus, build_webhook_router};
    use serde_json::json;
    use tower::ServiceExt;

    let mut config = CoveConfig::default();
    config.telegram.state = FeatureToggle::Enabled;
    config.telegram.webhook_secret = SecretValue::new("tg-sec");
    config.discord.state = FeatureToggle::Enabled;
    config.discord.ingress_secret = SecretValue::new("dc-sec");
    config.discord.bot_user_id = ExternalBotId::new("bot-77");
    config.discord.mention_policy = MentionPolicy::Required;
    let state = CoveServerState::from_config(config);
    let app = build_webhook_router(state.clone());

    #[cfg(feature = "telegram")]
    {
        let payload = json!({
            "update_id": 100,
            "message": {
                "message_id": 55,
                "from": { "id": 888, "username": "tg_alice", "first_name": "Alice" },
                "chat": { "id": -1001, "type": "supergroup", "title": "Marlin TG" },
                "date": 1_700_000_500,
                "text": "@reviewer check mobile swipe"
            }
        });
        let req = HttpRequest::builder()
            .method("POST")
            .uri("/v1/bridges/telegram/webhook")
            .header("content-type", "application/json")
            .header("x-telegram-bot-api-secret-token", "tg-sec")
            .body(Body::from(payload.to_string()))
            .unwrap();
        let resp = app.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::ACCEPTED);

        // Replay must deduplicate cleanly.
        let replay_req = HttpRequest::builder()
            .method("POST")
            .uri("/v1/bridges/telegram/webhook")
            .header("content-type", "application/json")
            .header("x-telegram-bot-api-secret-token", "tg-sec")
            .body(Body::from(payload.to_string()))
            .unwrap();
        let replay_resp = app.clone().oneshot(replay_req).await.unwrap();
        assert_eq!(replay_resp.status(), StatusCode::OK);
        let bytes = to_bytes(replay_resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let ack: BridgeIngressAck = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(ack.status, BridgeIngressStatus::Duplicate);

        // Unconfigured secret fails closed.
        let unconfigured =
            build_webhook_router(CoveServerState::from_config(CoveConfig::default()));
        let bad_req = HttpRequest::builder()
            .method("POST")
            .uri("/v1/bridges/telegram/webhook")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "update_id": 1 }).to_string()))
            .unwrap();
        let bad_resp = unconfigured.oneshot(bad_req).await.unwrap();
        assert_eq!(bad_resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[cfg(feature = "discord")]
    {
        let dc_req = HttpRequest::builder()
            .method("POST")
            .uri("/v1/bridges/discord/ingress")
            .header("content-type", "application/json")
            .header("x-marlin-cove-ingress-secret", "dc-sec")
            .body(Body::from(
                json!({
                    "message_id": "dc-msg-1",
                    "guild_id": "guild-1",
                    "channel_id": "chan-1",
                    "author_id": "user-9",
                    "author_name": "dc_bob",
                    "author_is_bot": false,
                    "content": "<@bot-77> status update please",
                    "mentioned_user_ids": ["bot-77"],
                    "timestamp_unix": 1_700_000_600
                })
                .to_string(),
            ))
            .unwrap();
        let dc_resp = app.clone().oneshot(dc_req).await.unwrap();
        assert_eq!(dc_resp.status(), StatusCode::ACCEPTED);
    }
}
