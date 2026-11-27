// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Cloud Cove gRPC over HTTP/2 (+ optional third-party webhook) server bootstrap.

use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

use crate::config::{CoveConfig, compiled_features};
use crate::relay::grpc_service::build_grpc_service;
use crate::relay::state::CoveServerState;

/// Bind and run the Cloud Cove gRPC over HTTP/2 server (and, when enabled and
/// compiled, the external Telegram/Discord HTTP webhook listener) until terminated.
///
/// # Errors
///
/// Returns an I/O error if binding a TCP listener or serving fails.
pub async fn serve(config: CoveConfig) -> std::io::Result<()> {
    let grpc_bind_addr = config.relay.grpc_bind_addr.as_str().to_string();
    let state = CoveServerState::from_config(config);
    let grpc_listener = TcpListener::bind(&grpc_bind_addr).await?;
    let grpc_incoming = TcpListenerStream::new(grpc_listener);
    let grpc_svc = build_grpc_service(state.clone());

    tracing::info!(
        %grpc_bind_addr,
        features = ?compiled_features(),
        "marlin-cove gRPC HTTP/2 relay listening"
    );

    let grpc_future = async move {
        tonic::transport::Server::builder()
            .add_service(grpc_svc)
            .serve_with_incoming(grpc_incoming)
            .await
            .map_err(std::io::Error::other)
    };

    #[cfg(any(feature = "telegram", feature = "discord"))]
    {
        let webhooks_enabled = (cfg!(feature = "telegram")
            && state.config.telegram.state.is_enabled())
            || (cfg!(feature = "discord") && state.config.discord.state.is_enabled());
        if webhooks_enabled
            && let Some(webhook_addr) = state.config.relay.webhook_bind_addr.as_ref()
        {
            let bind_str = webhook_addr.as_str().to_string();
            let webhook_listener = TcpListener::bind(&bind_str).await?;
            let webhook_app = crate::relay::webhooks::build_webhook_router(state);
            tracing::info!(
                webhook_bind_addr = %bind_str,
                "marlin-cove external bridge HTTP webhook listener active"
            );
            let webhook_future = async move { axum::serve(webhook_listener, webhook_app).await };
            tokio::try_join!(grpc_future, webhook_future)?;
            return Ok(());
        }
    }

    grpc_future.await
}
