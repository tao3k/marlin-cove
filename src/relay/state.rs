// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Shared Cloud Cove runtime state and health response types.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::config::CoveConfig;
use crate::marlin_gateway::MarlinGatewayClient;
use crate::relay::store::CoveRelayStore;

/// Shared runtime state backing the Cloud Cove gRPC HTTP/2 service and optional
/// external webhook ingress.
#[derive(Clone)]
pub struct CoveServerState {
    /// Active Cove configuration.
    pub config: Arc<CoveConfig>,
    /// Thread-safe Cove room/gate/membership store.
    pub store: CoveRelayStore,
    /// Outbound gRPC HTTP/2 client targeting Marlin Gateway (`marlin-agent-core`).
    pub marlin_gateway: Option<MarlinGatewayClient>,
}

impl CoveServerState {
    /// Build server state from a [`CoveConfig`].
    #[must_use]
    pub fn from_config(config: CoveConfig) -> Self {
        let store = CoveRelayStore::from_config(&config);
        let marlin_gateway = MarlinGatewayClient::new(
            config.marlin_gateway.grpc_endpoint.as_str(),
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

/// Typed liveness state reported by `CoveRelayService::Health`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceHealthStatus {
    /// Relay server is healthy and accepting gRPC HTTP/2 traffic.
    Ok,
}

/// Health summary mirrored by `CoveRelayService::Health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    /// Typed service status (`"ok"`).
    pub status: ServiceHealthStatus,
    /// Service identifier (`"marlin-cove"`).
    pub service: String,
    /// Compile-time Cargo features enabled in this server build.
    pub features: Vec<String>,
}
