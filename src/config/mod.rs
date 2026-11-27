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
    CoveConfig, DiscordBridgeConfig, GovernanceConfig, MarlinGatewayConfig, RelayCapacityLimits,
    RelayConfig, TelegramBridgeConfig,
};
pub use preflight::{
    CoveConfigSummary, LaunchPlan, PlannedSurface, PreflightIssue, PreflightReport,
    PreflightStatus, compiled_features,
};
pub use types::{
    BindAddress, CoveConfigError, DEFAULT_COVE_CHANNEL_ID, DEFAULT_COVE_ROOM_ID,
    DEFAULT_COVE_ROOM_TITLE, DEFAULT_DISCORD_API_BASE_URL, DEFAULT_DISCORD_INGRESS_PATH,
    DEFAULT_DISCORD_MAX_MESSAGE_CHARS, DEFAULT_EVENT_BUS_CAPACITY, DEFAULT_MARLIN_GATEWAY_ENDPOINT,
    DEFAULT_MAX_BOT_CHAIN_DEPTH, DEFAULT_MAX_BRIDGE_DEDUP_ENTRIES, DEFAULT_MAX_GATES_PER_ROOM,
    DEFAULT_MAX_GRPC_MESSAGE_BYTES, DEFAULT_MAX_INVITES, DEFAULT_MAX_MESSAGE_BYTES,
    DEFAULT_MAX_MESSAGES_PER_ROOM, DEFAULT_MAX_ROOMS, DEFAULT_RELAY_GRPC_BIND,
    DEFAULT_RELAY_WEBHOOK_BIND, DEFAULT_TELEGRAM_API_BASE_URL, DEFAULT_TELEGRAM_MAX_MESSAGE_CHARS,
    DEFAULT_TELEGRAM_WEBHOOK_PATH, ExternalBotId, FeatureToggle, MentionPolicy, RoutePath,
    SecretState, SecretValue, ServiceUrl, SessionPartitionName,
};
