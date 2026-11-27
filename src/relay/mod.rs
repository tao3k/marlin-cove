// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Cloud Cove Room Relay (`--features relay`).
//!
//! Coordinates rooms, scoped invites, gRPC over HTTP/2 server-streaming events,
//! human-only Tier-0 `PromptGate` approvals, and Marlin Gateway governance envelopes.

/// `tonic` gRPC HTTP/2 service for `CoveRelayService`.
pub mod grpc_service;
/// Relay request/response envelopes and typed errors.
pub mod requests;
/// Cloud Cove server runtime bootstrap.
pub mod server;
/// Shared Cloud Cove runtime state and health types.
pub mod state;
/// Thread-safe Cove state store.
pub mod store;
/// External Telegram/Discord HTTP webhook ingress.
#[cfg(any(feature = "telegram", feature = "discord"))]
pub mod webhooks;

pub use grpc_service::{CoveGrpcService, build_grpc_service};
pub use requests::{
    CreateInviteRequest, CreateRoomRequest, DispatchTurnRequest, DispatchTurnResponse,
    InviteTokenError, PostMessageRequest, RedeemInviteRequest, RelayError, RequestGateRequest,
    ResolveGateRequest, RoomSummary,
};
pub use server::serve;
pub use state::{CoveServerState, HealthResponse, ServiceHealthStatus};
pub use store::CoveRelayStore;
#[cfg(any(feature = "telegram", feature = "discord"))]
pub use webhooks::{BridgeIngressAck, BridgeIngressStatus, build_webhook_router};
