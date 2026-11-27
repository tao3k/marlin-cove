// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Secret-safe diagnostics, preflight evaluation, and feature-aware launch plans.

use std::fmt::{Display, Formatter};

use serde::Serialize;

use crate::config::model::CoveConfig;
use crate::config::types::{
    BindAddress, FeatureToggle, MentionPolicy, RoutePath, SecretState, ServiceUrl,
    SessionPartitionName,
};

/// Return the compile-time Cargo features enabled in this binary.
#[must_use]
pub fn compiled_features() -> Vec<&'static str> {
    let mut features = Vec::new();
    if cfg!(feature = "relay") {
        features.push("relay");
    }
    if cfg!(feature = "telegram") {
        features.push("telegram");
    }
    if cfg!(feature = "discord") {
        features.push("discord");
    }
    features
}

/// Secret-safe diagnostic summary rendered by CLI commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CoveConfigSummary {
    /// Cargo features compiled into this build.
    pub compiled_features: Vec<&'static str>,
    /// Effective relay status.
    pub relay_state: FeatureToggle,
    /// Relay bind address.
    pub relay_bind: BindAddress,
    /// Whether relay bearer token is configured.
    pub relay_auth_token: SecretState,
    /// Marlin Gateway (`marlin-agent-core`) URL.
    pub marlin_gateway_url: ServiceUrl,
    /// Whether Marlin Gateway bearer token is configured.
    pub marlin_gateway_token: SecretState,
    /// Effective Telegram bridge status.
    pub telegram_state: FeatureToggle,
    /// Whether Telegram bot token is configured.
    pub telegram_bot_token: SecretState,
    /// Whether Telegram webhook secret is configured.
    pub telegram_webhook_secret: SecretState,
    /// Effective Discord bridge status.
    pub discord_state: FeatureToggle,
    /// Whether Discord bot token is configured.
    pub discord_bot_token: SecretState,
    /// Whether Discord ingress secret is configured.
    pub discord_ingress_secret: SecretState,
}

impl CoveConfigSummary {
    /// Build a secret-safe diagnostic summary from a validated [`CoveConfig`].
    #[must_use]
    pub fn from_config(config: &CoveConfig) -> Self {
        Self {
            compiled_features: compiled_features(),
            relay_state: FeatureToggle::from_bool(
                config.relay.state.is_enabled() && cfg!(feature = "relay"),
            ),
            relay_bind: config.relay.bind_addr.clone(),
            relay_auth_token: SecretState::from_secret(config.relay.auth_token.as_ref()),
            marlin_gateway_url: config.marlin_gateway.base_url.clone(),
            marlin_gateway_token: SecretState::from_secret(
                config.marlin_gateway.bearer_token.as_ref(),
            ),
            telegram_state: FeatureToggle::from_bool(
                config.telegram.state.is_enabled() && cfg!(feature = "telegram"),
            ),
            telegram_bot_token: SecretState::from_secret(config.telegram.bot_token.as_ref()),
            telegram_webhook_secret: SecretState::from_secret(
                config.telegram.webhook_secret.as_ref(),
            ),
            discord_state: FeatureToggle::from_bool(
                config.discord.state.is_enabled() && cfg!(feature = "discord"),
            ),
            discord_bot_token: SecretState::from_secret(config.discord.bot_token.as_ref()),
            discord_ingress_secret: SecretState::from_secret(
                config.discord.ingress_secret.as_ref(),
            ),
        }
    }

    /// Render key=value diagnostic lines guaranteed to omit secret material.
    #[must_use]
    pub fn render_lines(&self) -> Vec<String> {
        vec![
            format!("compiled_features={}", self.compiled_features.join(",")),
            format!("relay_state={}", self.relay_state.as_str()),
            format!("relay_bind={}", self.relay_bind.as_str()),
            format!("relay_auth_token={}", self.relay_auth_token.as_str()),
            format!("marlin_gateway_url={}", self.marlin_gateway_url.as_str()),
            format!(
                "marlin_gateway_token={}",
                self.marlin_gateway_token.as_str()
            ),
            format!("telegram_state={}", self.telegram_state.as_str()),
            format!("telegram_bot_token={}", self.telegram_bot_token.as_str()),
            format!(
                "telegram_webhook_secret={}",
                self.telegram_webhook_secret.as_str()
            ),
            format!("discord_state={}", self.discord_state.as_str()),
            format!("discord_bot_token={}", self.discord_bot_token.as_str()),
            format!(
                "discord_ingress_secret={}",
                self.discord_ingress_secret.as_str()
            ),
        ]
    }
}

/// Preflight readiness status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreflightStatus {
    /// At least one runnable surface (relay or bridge) is ready.
    Ready,
    /// Configuration cannot satisfy runtime requirements.
    Blocked,
}

impl PreflightStatus {
    /// Return the stable status label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Blocked => "blocked",
        }
    }
}

/// Preflight issue blocking launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreflightIssue {
    /// No relay or bot bridge surface is enabled.
    NoActiveSurface,
    /// Telegram bridge was enabled in config but `telegram` Cargo feature is not compiled.
    TelegramFeatureNotCompiled,
    /// Discord bridge was enabled in config but `discord` Cargo feature is not compiled.
    DiscordFeatureNotCompiled,
    /// Relay was enabled in config but `relay` Cargo feature is not compiled.
    RelayFeatureNotCompiled,
    /// Telegram bridge is enabled without a bot token.
    MissingTelegramToken,
    /// Discord bridge is enabled without a bot token.
    MissingDiscordToken,
}

impl Display for PreflightIssue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoActiveSurface => {
                formatter.write_str("no enabled Cove relay or Telegram/Discord bridge surface")
            }
            Self::TelegramFeatureNotCompiled => {
                formatter.write_str("telegram bridge configured but `telegram` feature not enabled")
            }
            Self::DiscordFeatureNotCompiled => {
                formatter.write_str("discord bridge configured but `discord` feature not enabled")
            }
            Self::RelayFeatureNotCompiled => {
                formatter.write_str("relay server configured but `relay` feature not enabled")
            }
            Self::MissingTelegramToken => {
                formatter.write_str("telegram bridge enabled without bot_token")
            }
            Self::MissingDiscordToken => {
                formatter.write_str("discord bridge enabled without bot_token")
            }
        }
    }
}

/// Secret-safe preflight check result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightReport {
    /// Overall readiness.
    pub status: PreflightStatus,
    /// Blocking issues discovered.
    pub issues: Vec<PreflightIssue>,
}

impl PreflightReport {
    /// Evaluate a [`CoveConfig`] against the compiled Cargo feature set.
    #[must_use]
    pub fn evaluate(config: &CoveConfig) -> Self {
        let issues = collect_preflight_issues(config);
        let status = if issues.is_empty() {
            PreflightStatus::Ready
        } else {
            PreflightStatus::Blocked
        };
        Self { status, issues }
    }

    /// Return whether preflight passed with zero blocking issues.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self.status, PreflightStatus::Ready)
    }

    /// Render secret-safe diagnostic lines.
    #[must_use]
    pub fn render_lines(&self) -> Vec<String> {
        std::iter::once(format!("cove_preflight_status={}", self.status.as_str()))
            .chain(
                self.issues
                    .iter()
                    .map(|issue| format!("cove_preflight_issue={issue}")),
            )
            .collect()
    }
}

fn collect_preflight_issues(config: &CoveConfig) -> Vec<PreflightIssue> {
    let mut issues = Vec::new();
    if config.relay.state.is_enabled() && !cfg!(feature = "relay") {
        issues.push(PreflightIssue::RelayFeatureNotCompiled);
    }
    if config.telegram.state.is_enabled() {
        if !cfg!(feature = "telegram") {
            issues.push(PreflightIssue::TelegramFeatureNotCompiled);
        }
        if SecretState::from_secret(config.telegram.bot_token.as_ref()) == SecretState::Unset {
            issues.push(PreflightIssue::MissingTelegramToken);
        }
    }
    if config.discord.state.is_enabled() {
        if !cfg!(feature = "discord") {
            issues.push(PreflightIssue::DiscordFeatureNotCompiled);
        }
        if SecretState::from_secret(config.discord.bot_token.as_ref()) == SecretState::Unset {
            issues.push(PreflightIssue::MissingDiscordToken);
        }
    }
    if !has_any_compiled_active_surface(config) {
        issues.push(PreflightIssue::NoActiveSurface);
    }
    issues
}

const fn has_any_compiled_active_surface(config: &CoveConfig) -> bool {
    (config.relay.state.is_enabled() && cfg!(feature = "relay"))
        || (config.telegram.state.is_enabled() && cfg!(feature = "telegram"))
        || (config.discord.state.is_enabled() && cfg!(feature = "discord"))
}

/// Planned active endpoint in a secret-safe Cove launch plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlannedSurface {
    /// Native Cloud Cove Relay HTTP + SSE server.
    CoveRelay {
        /// Bound socket address.
        bind_addr: BindAddress,
    },
    /// Telegram webhook bridge mounted on the Cove server.
    TelegramWebhook {
        /// Mounted route path.
        path: RoutePath,
        /// Session partitioning strategy.
        session_partition: SessionPartitionName,
    },
    /// Discord ingress bridge mounted on the Cove server.
    DiscordIngress {
        /// Mounted route path.
        path: RoutePath,
        /// Session partitioning strategy.
        session_partition: SessionPartitionName,
        /// Mention requirement policy.
        mention_policy: MentionPolicy,
    },
}

/// Secret-safe live launch plan derived from [`CoveConfig`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// Planned active surfaces.
    pub surfaces: Vec<PlannedSurface>,
    /// Marlin Gateway (`marlin-agent-core`) target URL.
    pub marlin_gateway_url: ServiceUrl,
}

impl LaunchPlan {
    /// Build a launch plan from validated configuration.
    #[must_use]
    pub fn from_config(config: &CoveConfig) -> Self {
        let mut surfaces = Vec::new();
        if config.relay.state.is_enabled() && cfg!(feature = "relay") {
            surfaces.push(PlannedSurface::CoveRelay {
                bind_addr: config.relay.bind_addr.clone(),
            });
        }
        if config.telegram.state.is_enabled() && cfg!(feature = "telegram") {
            surfaces.push(PlannedSurface::TelegramWebhook {
                path: config.telegram.webhook_path.clone(),
                session_partition: config.telegram.session_partition.clone(),
            });
        }
        if config.discord.state.is_enabled() && cfg!(feature = "discord") {
            surfaces.push(PlannedSurface::DiscordIngress {
                path: config.discord.ingress_path.clone(),
                session_partition: config.discord.session_partition.clone(),
                mention_policy: config.discord.mention_policy,
            });
        }
        Self {
            surfaces,
            marlin_gateway_url: config.marlin_gateway.base_url.clone(),
        }
    }

    /// Return true if at least one surface is planned.
    #[must_use]
    pub const fn is_runnable(&self) -> bool {
        !self.surfaces.is_empty()
    }

    /// Render secret-safe launch plan lines.
    #[must_use]
    pub fn render_lines(&self) -> Vec<String> {
        let mut lines = vec![
            format!(
                "cove_plan_status={}",
                if self.is_runnable() {
                    "runnable"
                } else {
                    "empty"
                }
            ),
            format!(
                "cove_marlin_gateway_url={}",
                self.marlin_gateway_url.as_str()
            ),
            format!("cove_surface_count={}", self.surfaces.len()),
        ];
        lines.extend(self.surfaces.iter().map(render_surface_line));
        lines
    }
}

fn render_surface_line(surface: &PlannedSurface) -> String {
    match surface {
        PlannedSurface::CoveRelay { bind_addr } => {
            format!("surface=cove_relay bind={}", bind_addr.as_str())
        }
        PlannedSurface::TelegramWebhook {
            path,
            session_partition,
        } => format!(
            "surface=telegram_webhook path={} partition={}",
            path.as_str(),
            session_partition.as_str()
        ),
        PlannedSurface::DiscordIngress {
            path,
            session_partition,
            mention_policy,
        } => format!(
            "surface=discord_ingress path={} partition={} require_mention={}",
            path.as_str(),
            session_partition.as_str(),
            mention_policy.requires_mention()
        ),
    }
}
