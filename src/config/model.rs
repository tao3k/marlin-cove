// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Root TOML configuration envelope and `MARLIN_COVE_*` environment overlays.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::types::{
    BindAddress, CoveConfigError, DEFAULT_COVE_ROOM_ID, DEFAULT_DISCORD_API_BASE_URL,
    DEFAULT_DISCORD_INGRESS_PATH, DEFAULT_MARLIN_GATEWAY_URL, DEFAULT_MAX_BOT_CHAIN_DEPTH,
    DEFAULT_RELAY_BIND, DEFAULT_TELEGRAM_API_BASE_URL, DEFAULT_TELEGRAM_WEBHOOK_PATH,
    ExternalBotId, FeatureToggle, MentionPolicy, RoutePath, SecretValue, ServiceUrl,
    SessionPartitionName,
};
use crate::protocol::RoomId;

/// Cloud Cove Relay HTTP/SSE server settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayConfig {
    /// Whether the HTTP/SSE relay server is enabled.
    pub state: FeatureToggle,
    /// Socket bind address (`host:port`).
    pub bind_addr: BindAddress,
    /// Optional bearer token required for native Cove API writes.
    pub auth_token: Option<SecretValue>,
    /// Maximum bot-to-bot reply depth before suppressing auto-triggers.
    pub max_bot_chain_depth: u8,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            state: FeatureToggle::from_bool(cfg!(feature = "relay")),
            bind_addr: BindAddress::new(DEFAULT_RELAY_BIND),
            auth_token: None,
            max_bot_chain_depth: DEFAULT_MAX_BOT_CHAIN_DEPTH,
        }
    }
}

/// Marlin Gateway (`marlin-agent-core`) settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarlinGatewayConfig {
    /// Base URL for the Marlin Gateway (`marlin-agent-core`) service.
    pub base_url: ServiceUrl,
    /// Optional bearer token forwarded to Marlin Gateway.
    pub bearer_token: Option<SecretValue>,
}

impl Default for MarlinGatewayConfig {
    fn default() -> Self {
        Self {
            base_url: ServiceUrl::new("marlin_gateway.base_url", DEFAULT_MARLIN_GATEWAY_URL)
                .expect("default marlin gateway url is valid"),
            bearer_token: None,
        }
    }
}

/// Telegram bot bridge settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TelegramBridgeConfig {
    /// Whether the Telegram bridge is active.
    pub state: FeatureToggle,
    /// Bot token issued by `BotFather`.
    pub bot_token: Option<SecretValue>,
    /// Secret verified against `X-Telegram-Bot-Api-Secret-Token`.
    pub webhook_secret: Option<SecretValue>,
    /// Webhook mount path on the Cove Relay server.
    pub webhook_path: RoutePath,
    /// Telegram Bot API base URL.
    pub api_base_url: ServiceUrl,
    /// Session partition policy (`chat`, `chat_user`, `user`, `chat_thread_user`).
    pub session_partition: SessionPartitionName,
    /// Default Cove room id mapped to unmapped Telegram chats.
    pub default_room_id: RoomId,
}

impl Default for TelegramBridgeConfig {
    fn default() -> Self {
        Self {
            state: FeatureToggle::Disabled,
            bot_token: None,
            webhook_secret: None,
            webhook_path: RoutePath::new("telegram.webhook_path", DEFAULT_TELEGRAM_WEBHOOK_PATH)
                .expect("default telegram webhook path is valid"),
            api_base_url: ServiceUrl::new("telegram.api_base_url", DEFAULT_TELEGRAM_API_BASE_URL)
                .expect("default telegram api url is valid"),
            session_partition: SessionPartitionName::new("chat"),
            default_room_id: RoomId::new(DEFAULT_COVE_ROOM_ID).expect("static room id is valid"),
        }
    }
}

/// Discord bot bridge settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DiscordBridgeConfig {
    /// Whether the Discord bridge is active.
    pub state: FeatureToggle,
    /// Discord bot token.
    pub bot_token: Option<SecretValue>,
    /// Discord bot user id used by the mention gate.
    pub bot_user_id: Option<ExternalBotId>,
    /// Shared secret verified against `X-Marlin-Cove-Ingress-Secret`.
    pub ingress_secret: Option<SecretValue>,
    /// Ingress mount path on the Cove Relay server.
    pub ingress_path: RoutePath,
    /// Discord REST API base URL.
    pub api_base_url: ServiceUrl,
    /// Whether foreground channel messages require mentioning the bot.
    pub mention_policy: MentionPolicy,
    /// Session partition policy (`guild_channel_user`, `channel`, `user`, `guild_user`).
    pub session_partition: SessionPartitionName,
    /// Default Cove room id mapped to unmapped Discord channels.
    pub default_room_id: RoomId,
}

impl Default for DiscordBridgeConfig {
    fn default() -> Self {
        Self {
            state: FeatureToggle::Disabled,
            bot_token: None,
            bot_user_id: None,
            ingress_secret: None,
            ingress_path: RoutePath::new("discord.ingress_path", DEFAULT_DISCORD_INGRESS_PATH)
                .expect("default discord ingress path is valid"),
            api_base_url: ServiceUrl::new("discord.api_base_url", DEFAULT_DISCORD_API_BASE_URL)
                .expect("default discord api url is valid"),
            mention_policy: MentionPolicy::Required,
            session_partition: SessionPartitionName::new("guild_channel_user"),
            default_room_id: RoomId::new(DEFAULT_COVE_ROOM_ID).expect("static room id is valid"),
        }
    }
}

/// Cove high-order room/people/bot governance settings (TOML envelope pointing
/// to the Gerbil Scheme policy entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GovernanceConfig {
    /// Default scope tag assigned to newly admitted external guests or bridge senders.
    pub default_guest_scope: String,
    /// Optional path to a Gerbil Scheme policy entry (`agentic-ai-lab` Governance).
    pub scheme_policy_entry: Option<String>,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            default_guest_scope: "public".to_string(),
            scheme_policy_entry: Some("scheme/governance/interface.ss".to_string()),
        }
    }
}

/// Root typed TOML configuration envelope for `marlin-cove`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CoveConfig {
    /// Cloud Cove Relay settings.
    pub relay: RelayConfig,
    /// Marlin Gateway (`marlin-agent-core`) settings.
    pub marlin_gateway: MarlinGatewayConfig,
    /// Telegram bot bridge settings.
    pub telegram: TelegramBridgeConfig,
    /// Discord bot bridge settings.
    pub discord: DiscordBridgeConfig,
    /// High-order Cove room/people/bot governance settings.
    pub governance: GovernanceConfig,
}

impl CoveConfig {
    /// Parse a typed TOML configuration string and validate URLs/paths.
    ///
    /// # Errors
    ///
    /// Returns [`CoveConfigError`] when TOML parsing or field validation fails.
    pub fn from_toml_str(source: &str) -> Result<Self, CoveConfigError> {
        let config: Self = toml::from_str(source)?;
        config.validate()?;
        Ok(config)
    }

    /// Load a typed TOML configuration file from disk.
    ///
    /// # Errors
    ///
    /// Returns [`CoveConfigError`] when reading, parsing, or validation fails.
    pub fn from_toml_file(path: &Path) -> Result<Self, CoveConfigError> {
        let content =
            std::fs::read_to_string(path).map_err(|source| CoveConfigError::ReadFile {
                path: path.display().to_string(),
                source,
            })?;
        Self::from_toml_str(&content)
    }

    /// Build configuration from `MARLIN_COVE_*` environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`CoveConfigError`] when any variable is malformed.
    pub fn from_env() -> Result<Self, CoveConfigError> {
        Self::default().with_env_lookup(|key| std::env::var(key).ok())
    }

    /// Overlay `MARLIN_COVE_*` environment variables onto an existing config.
    ///
    /// # Errors
    ///
    /// Returns [`CoveConfigError`] when any override is invalid.
    pub fn with_env_lookup<F>(mut self, lookup: F) -> Result<Self, CoveConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        self.apply_relay_env(&lookup)?;
        self.apply_marlin_gateway_env(&lookup)?;
        self.apply_telegram_env(&lookup)?;
        self.apply_discord_env(&lookup)?;
        self.validate()?;
        Ok(self)
    }

    fn apply_relay_env<F>(&mut self, lookup: &F) -> Result<(), CoveConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_RELAY_BIND") {
            self.relay.bind_addr = BindAddress::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_RELAY_ENABLED") {
            self.relay.state =
                FeatureToggle::from_bool(parse_bool("MARLIN_COVE_RELAY_ENABLED", &value)?);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_RELAY_AUTH_TOKEN") {
            self.relay.auth_token = SecretValue::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_MAX_BOT_CHAIN_DEPTH") {
            self.relay.max_bot_chain_depth =
                value
                    .parse::<u8>()
                    .map_err(|_| CoveConfigError::InvalidInteger {
                        key: "MARLIN_COVE_MAX_BOT_CHAIN_DEPTH",
                        value,
                    })?;
        }
        Ok(())
    }

    fn apply_marlin_gateway_env<F>(&mut self, lookup: &F) -> Result<(), CoveConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_MARLIN_GATEWAY_URL") {
            self.marlin_gateway.base_url = ServiceUrl::new("marlin_gateway.base_url", value)?;
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_MARLIN_GATEWAY_TOKEN") {
            self.marlin_gateway.bearer_token = SecretValue::new(value);
        }
        Ok(())
    }

    fn apply_telegram_env<F>(&mut self, lookup: &F) -> Result<(), CoveConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_TELEGRAM_BOT_TOKEN") {
            self.telegram.bot_token = SecretValue::new(value);
            self.telegram.state = FeatureToggle::Enabled;
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_TELEGRAM_WEBHOOK_SECRET") {
            self.telegram.webhook_secret = SecretValue::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_TELEGRAM_WEBHOOK_PATH") {
            self.telegram.webhook_path = RoutePath::new("telegram.webhook_path", value)?;
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_TELEGRAM_API_BASE_URL") {
            self.telegram.api_base_url = ServiceUrl::new("telegram.api_base_url", value)?;
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_TELEGRAM_SESSION_PARTITION") {
            self.telegram.session_partition = SessionPartitionName::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_TELEGRAM_DEFAULT_ROOM_ID") {
            self.telegram.default_room_id =
                RoomId::new(value).map_err(|_| CoveConfigError::InvalidRoomId {
                    field: "telegram.default_room_id",
                })?;
        }
        Ok(())
    }

    fn apply_discord_env<F>(&mut self, lookup: &F) -> Result<(), CoveConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_BOT_TOKEN") {
            self.discord.bot_token = SecretValue::new(value);
            self.discord.state = FeatureToggle::Enabled;
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_BOT_USER_ID") {
            self.discord.bot_user_id = ExternalBotId::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_INGRESS_SECRET") {
            self.discord.ingress_secret = SecretValue::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_INGRESS_PATH") {
            self.discord.ingress_path = RoutePath::new("discord.ingress_path", value)?;
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_API_BASE_URL") {
            self.discord.api_base_url = ServiceUrl::new("discord.api_base_url", value)?;
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_REQUIRE_MENTION") {
            let required = parse_bool("MARLIN_COVE_DISCORD_REQUIRE_MENTION", &value)?;
            self.discord.mention_policy = if required {
                MentionPolicy::Required
            } else {
                MentionPolicy::Optional
            };
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_SESSION_PARTITION") {
            self.discord.session_partition = SessionPartitionName::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_DISCORD_DEFAULT_ROOM_ID") {
            self.discord.default_room_id =
                RoomId::new(value).map_err(|_| CoveConfigError::InvalidRoomId {
                    field: "discord.default_room_id",
                })?;
        }
        Ok(())
    }

    /// Validate all configured URLs and route paths.
    ///
    /// # Errors
    ///
    /// Returns [`CoveConfigError`] when any URL or path is invalid.
    pub fn validate(&self) -> Result<(), CoveConfigError> {
        let _ = ServiceUrl::new(
            "marlin_gateway.base_url",
            self.marlin_gateway.base_url.as_str(),
        )?;
        let _ = ServiceUrl::new("telegram.api_base_url", self.telegram.api_base_url.as_str())?;
        let _ = ServiceUrl::new("discord.api_base_url", self.discord.api_base_url.as_str())?;
        let _ = RoutePath::new("telegram.webhook_path", self.telegram.webhook_path.as_str())?;
        let _ = RoutePath::new("discord.ingress_path", self.discord.ingress_path.as_str())?;
        Ok(())
    }
}

fn non_empty<F>(lookup: &F, key: &str) -> Option<String>
where
    F: Fn(&str) -> Option<String>,
{
    lookup(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn parse_bool(key: &'static str, value: &str) -> Result<bool, CoveConfigError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(CoveConfigError::InvalidBoolean {
            key,
            value: value.to_string(),
        }),
    }
}
