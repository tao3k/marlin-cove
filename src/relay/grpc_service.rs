// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! High-performance `tonic` gRPC over HTTP/2 implementation of
//! `marlin.cove.v1.CoveRelayService`.

use std::pin::Pin;

use tokio_stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tonic::metadata::MetadataMap;
use tonic::{Request, Response, Status};

use crate::bridges::verify_shared_secret;
use crate::config::{CoveConfig, SecretValue, compiled_features};
use crate::marlin_gateway::MarlinGatewayTurnRequest;
use crate::proto_codec::{
    ProtoCodecError, attachment_from_proto, bridge_origin_from_proto, event_to_proto,
    gate_to_proto, governance_envelope_to_proto, invite_to_proto, message_to_proto,
    optional_gate_id, optional_message_id, participant_to_proto, principal_kind_from_proto,
    principal_role_from_proto, receipt_to_proto, snapshot_to_proto,
};
use crate::proto_wire::cove_v1;
use crate::proto_wire::cove_v1::cove_relay_service_server::{
    CoveRelayService, CoveRelayServiceServer,
};
use crate::protocol::{
    BridgeOrigin, ChannelId, GateId, PrincipalId, ProtocolError, RoomId, ScopeId, SessionKey,
};
use crate::relay::requests::{
    CreateInviteRequest, CreateRoomRequest, PostMessageRequest, RedeemInviteRequest, RelayError,
    RequestGateRequest, ResolveGateRequest, RoomSummary,
};
use crate::relay::state::CoveServerState;

/// Hardened `tonic` gRPC HTTP/2 service implementing `marlin.cove.v1.CoveRelayService`.
#[derive(Clone)]
pub struct CoveGrpcService {
    state: CoveServerState,
}

impl CoveGrpcService {
    /// Construct a new gRPC service backed by shared [`CoveServerState`].
    #[must_use]
    pub const fn new(state: CoveServerState) -> Self {
        Self { state }
    }
}

/// Build a configured `tonic` [`CoveRelayServiceServer`] enforcing the TOML
/// decoding message ceiling (`relay.limits.max_grpc_message_bytes`).
#[must_use]
pub fn build_grpc_service(state: CoveServerState) -> CoveRelayServiceServer<CoveGrpcService> {
    let max_bytes = state.config.relay.limits.max_grpc_message_bytes;
    CoveRelayServiceServer::new(CoveGrpcService::new(state)).max_decoding_message_size(max_bytes)
}

type EventStream = Pin<Box<dyn Stream<Item = Result<cove_v1::RoomEvent, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl CoveRelayService for CoveGrpcService {
    type WatchRoomEventsStream = EventStream;

    async fn health(
        &self,
        _request: Request<cove_v1::HealthRequest>,
    ) -> Result<Response<cove_v1::HealthResponse>, Status> {
        Ok(Response::new(cove_v1::HealthResponse {
            status: cove_v1::ServiceHealthStatus::Ok as i32,
            service: "marlin-cove".to_string(),
            features: compiled_features()
                .into_iter()
                .map(ToString::to_string)
                .collect(),
        }))
    }

    async fn list_rooms(
        &self,
        request: Request<cove_v1::ListRoomsRequest>,
    ) -> Result<Response<cove_v1::ListRoomsResponse>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let rooms = self
            .state
            .store
            .list_rooms()
            .map_err(relay_error_to_status)?
            .into_iter()
            .map(summary_to_proto)
            .collect();
        Ok(Response::new(cove_v1::ListRoomsResponse { rooms }))
    }

    async fn create_room(
        &self,
        request: Request<cove_v1::CreateRoomRequest>,
    ) -> Result<Response<cove_v1::RoomSnapshot>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let req = request.into_inner();
        let room_id = if req.room_id.trim().is_empty() {
            None
        } else {
            Some(RoomId::new(req.room_id).map_err(protocol_error_to_status)?)
        };
        let domain_req = CreateRoomRequest {
            room_id,
            title: req.title,
            owner_id: PrincipalId::new(req.owner_id).map_err(protocol_error_to_status)?,
            owner_name: req.owner_name,
            owner_handle: req.owner_handle,
        };
        let snapshot = self
            .state
            .store
            .create_room(domain_req)
            .map_err(relay_error_to_status)?;
        Ok(Response::new(snapshot_to_proto(&snapshot)))
    }

    async fn get_room_snapshot(
        &self,
        request: Request<cove_v1::GetRoomSnapshotRequest>,
    ) -> Result<Response<cove_v1::RoomSnapshot>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let room_id =
            RoomId::new(request.into_inner().room_id).map_err(protocol_error_to_status)?;
        let snapshot = self
            .state
            .store
            .room_snapshot(&room_id)
            .map_err(relay_error_to_status)?;
        Ok(Response::new(snapshot_to_proto(&snapshot)))
    }

    async fn get_governance_envelope(
        &self,
        request: Request<cove_v1::GetGovernanceEnvelopeRequest>,
    ) -> Result<Response<cove_v1::RoomGovernanceEnvelope>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let req = request.into_inner();
        let room_id = RoomId::new(req.room_id).map_err(protocol_error_to_status)?;
        let principal_id = PrincipalId::new(req.principal_id).map_err(protocol_error_to_status)?;
        let channel_id = if req.channel_id.trim().is_empty() {
            self.state.config.relay.default_channel_id.clone()
        } else {
            ChannelId::new(req.channel_id).map_err(protocol_error_to_status)?
        };
        let envelope = self
            .state
            .store
            .governance_envelope(
                &room_id,
                channel_id,
                &principal_id,
                BridgeOrigin::CoveNative,
            )
            .map_err(relay_error_to_status)?;
        Ok(Response::new(governance_envelope_to_proto(&envelope)))
    }

    async fn create_invite(
        &self,
        request: Request<cove_v1::CreateInviteRequest>,
    ) -> Result<Response<cove_v1::RoomInvite>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let req = request.into_inner();
        let room_id = RoomId::new(req.room_id).map_err(protocol_error_to_status)?;
        let grant_scopes = req
            .grant_scopes
            .into_iter()
            .map(ScopeId::new)
            .collect::<Result<Vec<_>, _>>()
            .map_err(protocol_error_to_status)?;
        let domain_req = CreateInviteRequest {
            issued_by: PrincipalId::new(req.issued_by).map_err(protocol_error_to_status)?,
            grant_role: principal_role_from_proto(req.grant_role).map_err(codec_error_to_status)?,
            grant_scopes,
            max_uses: req.max_uses,
        };
        let invite = self
            .state
            .store
            .create_invite(&room_id, domain_req)
            .map_err(relay_error_to_status)?;
        Ok(Response::new(invite_to_proto(&invite)))
    }

    async fn redeem_invite(
        &self,
        request: Request<cove_v1::RedeemInviteRequest>,
    ) -> Result<Response<cove_v1::RoomParticipant>, Status> {
        let req = request.into_inner();
        let domain_req = RedeemInviteRequest {
            principal_id: PrincipalId::new(req.principal_id).map_err(protocol_error_to_status)?,
            display_name: req.display_name,
            handle: req.handle,
            kind: principal_kind_from_proto(req.kind).map_err(codec_error_to_status)?,
        };
        let participant = self
            .state
            .store
            .redeem_invite(&req.token, domain_req)
            .map_err(relay_error_to_status)?;
        Ok(Response::new(participant_to_proto(&participant)))
    }

    async fn post_message(
        &self,
        request: Request<cove_v1::PostMessageRequest>,
    ) -> Result<Response<cove_v1::RoomMessage>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let req = request.into_inner();
        let room_id = RoomId::new(req.room_id).map_err(protocol_error_to_status)?;
        let attachments = req
            .attachments
            .into_iter()
            .map(attachment_from_proto)
            .collect::<Result<Vec<_>, _>>()
            .map_err(codec_error_to_status)?;
        let bot_chain_depth = u8::try_from(req.bot_chain_depth)
            .map_err(|_| Status::invalid_argument("bot_chain_depth exceeds u8 ceiling"))?;
        let domain_req = PostMessageRequest {
            channel_id: ChannelId::new(req.channel_id).map_err(protocol_error_to_status)?,
            thread_root_id: optional_message_id(&req.thread_root_id)
                .map_err(protocol_error_to_status)?,
            author_id: PrincipalId::new(req.author_id).map_err(protocol_error_to_status)?,
            body: req.body,
            gate_id: optional_gate_id(&req.gate_id).map_err(protocol_error_to_status)?,
            attachments,
            origin: bridge_origin_from_proto(req.origin).map_err(codec_error_to_status)?,
            bot_chain_depth,
        };
        let message = self
            .state
            .store
            .post_message(&room_id, domain_req)
            .map_err(relay_error_to_status)?;
        Ok(Response::new(message_to_proto(&message)))
    }

    async fn dispatch_turn(
        &self,
        request: Request<cove_v1::DispatchTurnRequest>,
    ) -> Result<Response<cove_v1::DispatchTurnResponse>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let req = request.into_inner();
        let room_id = RoomId::new(req.room_id).map_err(protocol_error_to_status)?;
        let channel_id = ChannelId::new(req.channel_id).map_err(protocol_error_to_status)?;
        let author_id = PrincipalId::new(req.author_id).map_err(protocol_error_to_status)?;
        let gateway =
            self.state.marlin_gateway.as_ref().ok_or_else(|| {
                Status::unavailable("Marlin Gateway gRPC client is not configured")
            })?;

        let agent_id = self
            .state
            .store
            .find_agent_by_handle(&room_id, &req.target_agent_handle)
            .map_err(relay_error_to_status)?;
        let governance = self
            .state
            .store
            .governance_envelope(
                &room_id,
                channel_id.clone(),
                &author_id,
                BridgeOrigin::CoveNative,
            )
            .map_err(relay_error_to_status)?;

        let thread_root_id =
            optional_message_id(&req.thread_root_id).map_err(protocol_error_to_status)?;
        let user_message = self
            .state
            .store
            .post_message(
                &room_id,
                PostMessageRequest {
                    channel_id: channel_id.clone(),
                    thread_root_id: thread_root_id.clone(),
                    author_id,
                    body: req.body.clone(),
                    gate_id: None,
                    attachments: Vec::new(),
                    origin: BridgeOrigin::CoveNative,
                    bot_chain_depth: 0,
                },
            )
            .map_err(relay_error_to_status)?;

        let session_key = SessionKey::new(format!(
            "cove:{}:{}:{}",
            room_id.as_str(),
            channel_id.as_str(),
            req.target_agent_handle.trim_start_matches('@')
        ))
        .map_err(protocol_error_to_status)?;
        let gateway_req = MarlinGatewayTurnRequest {
            session_id: session_key,
            governance: governance.clone(),
            target_agent_handle: Some(req.target_agent_handle),
            message: req.body,
            attachments: Vec::new(),
        };
        let gateway_reply = gateway
            .dispatch_turn(&gateway_req)
            .await
            .map_err(|error| Status::unavailable(error.to_string()))?;

        let reply_thread_root_id = if thread_root_id.is_some() {
            thread_root_id
        } else if !channel_id.as_str().starts_with("dm:") {
            Some(user_message.id.clone())
        } else {
            None
        };

        let agent_reply = self
            .state
            .store
            .post_message(
                &room_id,
                PostMessageRequest {
                    channel_id,
                    thread_root_id: reply_thread_root_id,
                    author_id: agent_id,
                    body: gateway_reply.response,
                    gate_id: None,
                    attachments: Vec::new(),
                    origin: BridgeOrigin::MarlinGateway,
                    bot_chain_depth: 1,
                },
            )
            .map_err(relay_error_to_status)?;

        Ok(Response::new(cove_v1::DispatchTurnResponse {
            user_message: Some(message_to_proto(&user_message)),
            governance: Some(governance_envelope_to_proto(&governance)),
            agent_reply: Some(message_to_proto(&agent_reply)),
        }))
    }

    async fn request_gate(
        &self,
        request: Request<cove_v1::RequestGateRequest>,
    ) -> Result<Response<cove_v1::RoomGate>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let req = request.into_inner();
        let room_id = RoomId::new(req.room_id).map_err(protocol_error_to_status)?;
        let domain_req = RequestGateRequest {
            channel_id: ChannelId::new(req.channel_id).map_err(protocol_error_to_status)?,
            requested_by: PrincipalId::new(req.requested_by).map_err(protocol_error_to_status)?,
            summary: req.summary,
            capability: req.capability,
        };
        let gate = self
            .state
            .store
            .request_gate(&room_id, domain_req)
            .map_err(relay_error_to_status)?;
        Ok(Response::new(gate_to_proto(&gate)))
    }

    async fn resolve_gate(
        &self,
        request: Request<cove_v1::ResolveGateRequest>,
    ) -> Result<Response<cove_v1::GateApprovalReceipt>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let req = request.into_inner();
        let room_id = RoomId::new(req.room_id).map_err(protocol_error_to_status)?;
        let gate_id = GateId::new(req.gate_id).map_err(protocol_error_to_status)?;
        let domain_req = ResolveGateRequest {
            actor_id: PrincipalId::new(req.actor_id).map_err(protocol_error_to_status)?,
            approved: req.approved,
            reason: req.reason,
        };
        let receipt = self
            .state
            .store
            .resolve_gate(&room_id, &gate_id, domain_req)
            .map_err(relay_error_to_status)?;
        Ok(Response::new(receipt_to_proto(&receipt)))
    }

    async fn watch_room_events(
        &self,
        request: Request<cove_v1::WatchRoomEventsRequest>,
    ) -> Result<Response<Self::WatchRoomEventsStream>, Status> {
        check_grpc_auth(&self.state.config, request.metadata())?;
        let room_id =
            RoomId::new(request.into_inner().room_id).map_err(protocol_error_to_status)?;
        let rx = self
            .state
            .store
            .subscribe_room_events(&room_id)
            .map_err(relay_error_to_status)?;

        let stream = async_stream::try_stream! {
            use tokio_stream::StreamExt;
            let mut broadcast = BroadcastStream::new(rx);
            while let Some(item) = broadcast.next().await {
                if let Ok(event) = item {
                    yield event_to_proto(&event);
                }
            }
        };
        Ok(Response::new(Box::pin(stream)))
    }
}

fn check_grpc_auth(config: &CoveConfig, metadata: &MetadataMap) -> Result<(), Status> {
    let expected = config.relay.auth_token.as_ref().map(SecretValue::as_str);
    let provided = metadata
        .get("authorization")
        .and_then(|value| value.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "));

    if verify_shared_secret(expected, provided) {
        Ok(())
    } else {
        Err(Status::unauthenticated(
            "missing or invalid Cove relay bearer token",
        ))
    }
}

fn summary_to_proto(summary: RoomSummary) -> cove_v1::RoomSummary {
    cove_v1::RoomSummary {
        room_id: summary.room_id.to_string(),
        title: summary.title,
        channel_count: summary.channel_count as u64,
        participant_count: summary.participant_count as u64,
        message_count: summary.message_count as u64,
        pending_gates: summary.pending_gates as u64,
    }
}

#[allow(clippy::needless_pass_by_value)]
fn relay_error_to_status(error: RelayError) -> Status {
    let message = error.to_string();
    match error {
        RelayError::RoomNotFound(_)
        | RelayError::ChannelNotFound { .. }
        | RelayError::ParticipantNotFound { .. }
        | RelayError::GateNotFound { .. } => Status::not_found(message),
        RelayError::InvalidInvite(_)
        | RelayError::BotLoopLimitExceeded { .. }
        | RelayError::PrincipalKindRoleMismatch
        | RelayError::MessageTooLarge { .. }
        | RelayError::Protocol(ProtocolError::EmptyIdentifier { .. }) => {
            Status::invalid_argument(message)
        }
        RelayError::CapacityExceeded { .. } => Status::resource_exhausted(message),
        RelayError::UnauthorizedAction { .. }
        | RelayError::PrivilegeEscalation { .. }
        | RelayError::Protocol(ProtocolError::HumanApprovalRequired { .. }) => {
            Status::permission_denied(message)
        }
        RelayError::RoomAlreadyExists(_)
        | RelayError::ParticipantAlreadyExists { .. }
        | RelayError::GateAlreadyResolved { .. }
        | RelayError::DuplicateBridgeMessage { .. } => Status::already_exists(message),
        RelayError::LockPoisoned => Status::internal(message),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn protocol_error_to_status(error: ProtocolError) -> Status {
    let message = error.to_string();
    match error {
        ProtocolError::EmptyIdentifier { .. } => Status::invalid_argument(message),
        ProtocolError::HumanApprovalRequired { .. } => Status::permission_denied(message),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn codec_error_to_status(error: ProtoCodecError) -> Status {
    Status::invalid_argument(error.to_string())
}
