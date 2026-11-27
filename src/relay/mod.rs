// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Cloud Cove Room Relay (`--features relay`).
//!
//! Coordinates rooms, scoped invites, SSE streams, human-only Tier-0
//! `PromptGate` approvals, and Marlin Gateway governance envelopes.

/// Axum HTTP + SSE server.
pub mod server;
/// Thread-safe Cove state store.
pub mod store;

pub use server::{
    BridgeIngressAck, BridgeIngressStatus, CoveServerState, HealthResponse, ServiceHealthStatus,
    build_router, serve,
};
pub use store::{
    CoveRelayStore, CreateInviteRequest, CreateRoomRequest, PostMessageRequest,
    RedeemInviteRequest, RelayError, RequestGateRequest, ResolveGateRequest, RoomSummary,
};
