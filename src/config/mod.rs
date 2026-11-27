// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Configuration, diagnostics, preflight, and launch planning.

/// Root TOML configuration model and `MARLIN_COVE_*` environment overlay.
pub mod model;
/// Secret-safe configuration summary, preflight checks, and launch plans.
pub mod preflight;
/// Typed domain primitives for Cove configuration fields.
pub mod types;

pub use model::{
    CoveConfig, DiscordBridgeConfig, GovernanceConfig, MarlinGatewayConfig, RelayConfig,
    TelegramBridgeConfig,
};
pub use preflight::{
    CoveConfigSummary, LaunchPlan, PlannedSurface, PreflightIssue, PreflightReport,
    PreflightStatus, compiled_features,
};
pub use types::{
    BindAddress, CoveConfigError, DEFAULT_COVE_ROOM_ID, DEFAULT_DISCORD_API_BASE_URL,
    DEFAULT_DISCORD_INGRESS_PATH, DEFAULT_MARLIN_GATEWAY_URL, DEFAULT_MAX_BOT_CHAIN_DEPTH,
    DEFAULT_RELAY_BIND, DEFAULT_TELEGRAM_API_BASE_URL, DEFAULT_TELEGRAM_WEBHOOK_PATH,
    ExternalBotId, FeatureToggle, MentionPolicy, RoutePath, SecretState, SecretValue, ServiceUrl,
    SessionPartitionName,
};
