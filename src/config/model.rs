// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Root TOML configuration envelope and `MARLIN_COVE_*` environment overlays.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::types::{
    BindAddress, CoveConfigError, DEFAULT_COVE_CHANNEL_ID, DEFAULT_COVE_ROOM_ID,
    DEFAULT_COVE_ROOM_TITLE, DEFAULT_DISCORD_API_BASE_URL, DEFAULT_DISCORD_INGRESS_PATH,
    DEFAULT_DISCORD_MAX_MESSAGE_CHARS, DEFAULT_EVENT_BUS_CAPACITY, DEFAULT_MARLIN_GATEWAY_ENDPOINT,
    DEFAULT_MAX_BOT_CHAIN_DEPTH, DEFAULT_MAX_BRIDGE_DEDUP_ENTRIES, DEFAULT_MAX_GATES_PER_ROOM,
    DEFAULT_MAX_GRPC_MESSAGE_BYTES, DEFAULT_MAX_INVITES, DEFAULT_MAX_MESSAGE_BYTES,
    DEFAULT_MAX_MESSAGES_PER_ROOM, DEFAULT_MAX_ROOMS, DEFAULT_RELAY_GRPC_BIND,
    DEFAULT_RELAY_WEBHOOK_BIND, DEFAULT_TELEGRAM_API_BASE_URL, DEFAULT_TELEGRAM_MAX_MESSAGE_CHARS,
    DEFAULT_TELEGRAM_WEBHOOK_PATH, ExternalBotId, FeatureToggle, MentionPolicy, RoutePath,
    SecretValue, ServiceUrl, SessionPartitionName,
};
use crate::protocol::{ChannelId, RoomId};

/// Bounded capacity limits instantiating `deterministic-engineering`'s `CapacityModel`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayCapacityLimits {
    /// gRPC HTTP/2 event stream broadcast channel buffer size per room.
    pub event_bus_capacity: usize,
    /// Maximum rooms allowed in the relay store.
    pub max_rooms: usize,
    /// Maximum active invite tokens allowed.
    pub max_invites: usize,
    /// Maximum timeline messages retained per room.
    pub max_messages_per_room: usize,
    /// Maximum Tier-0 governance gates retained per room.
    pub max_gates_per_room: usize,
    /// Maximum external bridge ids retained per room for replay deduplication.
    pub max_bridge_dedup_entries: usize,
    /// Maximum gRPC message decoding / webhook payload size in bytes.
    pub max_grpc_message_bytes: usize,
    /// Maximum single message body size in bytes.
    pub max_message_bytes: usize,
}

impl Default for RelayCapacityLimits {
    fn default() -> Self {
        Self {
            event_bus_capacity: DEFAULT_EVENT_BUS_CAPACITY,
            max_rooms: DEFAULT_MAX_ROOMS,
            max_invites: DEFAULT_MAX_INVITES,
            max_messages_per_room: DEFAULT_MAX_MESSAGES_PER_ROOM,
            max_gates_per_room: DEFAULT_MAX_GATES_PER_ROOM,
            max_bridge_dedup_entries: DEFAULT_MAX_BRIDGE_DEDUP_ENTRIES,
            max_grpc_message_bytes: DEFAULT_MAX_GRPC_MESSAGE_BYTES,
            max_message_bytes: DEFAULT_MAX_MESSAGE_BYTES,
        }
    }
}

/// Cloud Cove Relay gRPC over HTTP/2 server settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RelayConfig {
    /// Whether the gRPC HTTP/2 relay server is enabled.
    pub state: FeatureToggle,
    /// Socket bind address for `marlin.cove.v1.CoveRelayService` over HTTP/2 (`host:port`).
    pub grpc_bind_addr: BindAddress,
    /// Optional HTTP webhook listener bind address mounted ONLY when Telegram or Discord
    /// bridge is enabled in config and compiled.
    pub webhook_bind_addr: Option<BindAddress>,
    /// Optional bearer token required in gRPC `authorization` metadata for all RPCs
    /// (except `Health` and `RedeemInvite`).
    pub auth_token: Option<SecretValue>,
    /// Default seeded room identifier.
    pub default_room_id: RoomId,
    /// Default seeded room display title.
    pub default_room_title: String,
    /// Default channel identifier for room queries and bridges.
    pub default_channel_id: ChannelId,
    /// Maximum bot-to-bot reply depth before suppressing auto-triggers.
    pub max_bot_chain_depth: u8,
    /// Bounded retention & payload limits (overflow refuses admission).
    pub limits: RelayCapacityLimits,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            state: FeatureToggle::from_bool(cfg!(feature = "relay")),
            grpc_bind_addr: BindAddress::new(DEFAULT_RELAY_GRPC_BIND),
            webhook_bind_addr: Some(BindAddress::new(DEFAULT_RELAY_WEBHOOK_BIND)),
            auth_token: None,
            default_room_id: RoomId::from_trusted(DEFAULT_COVE_ROOM_ID.to_string()),
            default_room_title: DEFAULT_COVE_ROOM_TITLE.to_string(),
            default_channel_id: ChannelId::from_trusted(DEFAULT_COVE_CHANNEL_ID.to_string()),
            max_bot_chain_depth: DEFAULT_MAX_BOT_CHAIN_DEPTH,
            limits: RelayCapacityLimits::default(),
        }
    }
}

/// Marlin Gateway (`marlin-agent-core`) gRPC over HTTP/2 settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MarlinGatewayConfig {
    /// gRPC over HTTP/2 endpoint URI for Marlin Gateway (`marlin-agent-core`).
    pub grpc_endpoint: ServiceUrl,
    /// Optional bearer token forwarded in gRPC `authorization` metadata.
    pub bearer_token: Option<SecretValue>,
}

impl Default for MarlinGatewayConfig {
    fn default() -> Self {
        Self {
            grpc_endpoint: ServiceUrl::new(
                "marlin_gateway.grpc_endpoint",
                DEFAULT_MARLIN_GATEWAY_ENDPOINT,
            )
            .expect("default marlin gateway endpoint is valid"),
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
    /// Webhook mount path on the optional external webhook listener.
    pub webhook_path: RoutePath,
    /// Telegram Bot API base URL.
    pub api_base_url: ServiceUrl,
    /// Session partition policy (`chat`, `chat_user`, `user`, `chat_thread_user`).
    pub session_partition: SessionPartitionName,
    /// Default Cove room id mapped to unmapped Telegram chats.
    pub default_room_id: RoomId,
    /// Default Cove channel id mapped to unmapped Telegram chats.
    pub default_channel_id: ChannelId,
    /// Maximum outbound message chunk size in characters.
    pub max_message_chars: usize,
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
            default_room_id: RoomId::from_trusted(DEFAULT_COVE_ROOM_ID.to_string()),
            default_channel_id: ChannelId::from_trusted(DEFAULT_COVE_CHANNEL_ID.to_string()),
            max_message_chars: DEFAULT_TELEGRAM_MAX_MESSAGE_CHARS,
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
    /// Ingress mount path on the optional external webhook listener.
    pub ingress_path: RoutePath,
    /// Discord REST API base URL.
    pub api_base_url: ServiceUrl,
    /// Whether foreground channel messages require mentioning the bot.
    pub mention_policy: MentionPolicy,
    /// Session partition policy (`guild_channel_user`, `channel`, `user`, `guild_user`).
    pub session_partition: SessionPartitionName,
    /// Default Cove room id mapped to unmapped Discord channels.
    pub default_room_id: RoomId,
    /// Default Cove channel id mapped to unmapped Discord channels.
    pub default_channel_id: ChannelId,
    /// Maximum outbound message chunk size in characters.
    pub max_message_chars: usize,
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
            default_room_id: RoomId::from_trusted(DEFAULT_COVE_ROOM_ID.to_string()),
            default_channel_id: ChannelId::from_trusted(DEFAULT_COVE_CHANNEL_ID.to_string()),
            max_message_chars: DEFAULT_DISCORD_MAX_MESSAGE_CHARS,
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
    /// Cloud Cove gRPC HTTP/2 Relay settings.
    pub relay: RelayConfig,
    /// Marlin Gateway (`marlin-agent-core`) gRPC HTTP/2 settings.
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
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_RELAY_GRPC_BIND") {
            self.relay.grpc_bind_addr = BindAddress::new(value);
        }
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_RELAY_WEBHOOK_BIND") {
            self.relay.webhook_bind_addr = Some(BindAddress::new(value));
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
        if let Some(value) = non_empty(lookup, "MARLIN_COVE_MARLIN_GATEWAY_ENDPOINT") {
            self.marlin_gateway.grpc_endpoint =
                ServiceUrl::new("marlin_gateway.grpc_endpoint", value)?;
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
            "marlin_gateway.grpc_endpoint",
            self.marlin_gateway.grpc_endpoint.as_str(),
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
