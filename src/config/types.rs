// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Typed domain primitives and errors for Marlin Cove TOML configuration.

use std::fmt::Formatter;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default local bind address for the Cloud Cove gRPC over HTTP/2 server.
pub const DEFAULT_RELAY_GRPC_BIND: &str = "127.0.0.1:18090";
/// Default local bind address for the optional third-party HTTP webhook listener.
pub const DEFAULT_RELAY_WEBHOOK_BIND: &str = "127.0.0.1:18091";
/// Default Marlin Gateway (`marlin-agent-core`) gRPC over HTTP/2 endpoint URI.
pub const DEFAULT_MARLIN_GATEWAY_ENDPOINT: &str = "http://127.0.0.1:18093";
/// Default Telegram Bot API base URL.
pub const DEFAULT_TELEGRAM_API_BASE_URL: &str = "https://api.telegram.org";
/// Default Telegram webhook route path.
pub const DEFAULT_TELEGRAM_WEBHOOK_PATH: &str = "/v1/bridges/telegram/webhook";
/// Default Discord API base URL.
pub const DEFAULT_DISCORD_API_BASE_URL: &str = "https://discord.com/api/v10";
/// Default Discord ingress route path.
pub const DEFAULT_DISCORD_INGRESS_PATH: &str = "/v1/bridges/discord/ingress";
/// Default Cove room id for bridged external chats.
pub const DEFAULT_COVE_ROOM_ID: &str = "cove-main";
/// Default Cove room title.
pub const DEFAULT_COVE_ROOM_TITLE: &str = "Marlin Cove Main";
/// Default Cove channel id for room and bridge ingress.
pub const DEFAULT_COVE_CHANNEL_ID: &str = "general";
/// Maximum bot-to-bot reply chain depth allowed before fail-closed suppression.
pub const DEFAULT_MAX_BOT_CHAIN_DEPTH: u8 = 2;
/// Default gRPC event stream broadcast channel capacity per room.
pub const DEFAULT_EVENT_BUS_CAPACITY: usize = 256;
/// Default maximum number of rooms retained in memory (`CapacityModel` bound).
pub const DEFAULT_MAX_ROOMS: usize = 64;
/// Default maximum active invite tokens retained in memory.
pub const DEFAULT_MAX_INVITES: usize = 256;
/// Default maximum messages retained per room.
pub const DEFAULT_MAX_MESSAGES_PER_ROOM: usize = 2048;
/// Default maximum Tier-0 gates retained per room.
pub const DEFAULT_MAX_GATES_PER_ROOM: usize = 256;
/// Default maximum external webhook ids retained per room for replay deduplication.
pub const DEFAULT_MAX_BRIDGE_DEDUP_ENTRIES: usize = 4096;
/// Default maximum gRPC / webhook payload size in bytes (64 KiB).
pub const DEFAULT_MAX_GRPC_MESSAGE_BYTES: usize = 65_536;
/// Default maximum single message body size in bytes (16 KiB).
pub const DEFAULT_MAX_MESSAGE_BYTES: usize = 16_384;
/// Default Telegram outbound chunk character ceiling.
pub const DEFAULT_TELEGRAM_MAX_MESSAGE_CHARS: usize = 4096;
/// Default Discord outbound chunk character ceiling.
pub const DEFAULT_DISCORD_MAX_MESSAGE_CHARS: usize = 2000;

/// Secret presence indicator safe to print in logs and CLI summaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SecretState {
    /// Secret value is configured.
    Configured,
    /// Secret value is absent.
    Unset,
}

impl SecretState {
    /// Build a secret presence flag from an optional secret wrapper.
    #[must_use]
    pub fn from_secret(value: Option<&SecretValue>) -> Self {
        match value.map(|secret| secret.as_str().trim()) {
            Some(trimmed) if !trimmed.is_empty() => Self::Configured,
            _ => Self::Unset,
        }
    }

    /// Return the stable diagnostic representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Unset => "unset",
        }
    }
}

/// Redacted secret wrapper preventing accidental secret leakage via `Debug`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SecretValue(String);

impl SecretValue {
    /// Wrap a non-empty secret string.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let trimmed = value.into().trim().to_string();
        (!trimmed.is_empty()).then_some(Self(trimmed))
    }

    /// Expose the raw secret strictly at authenticated transport boundaries.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("SecretValue([REDACTED])")
    }
}

/// Enablement state for a Cove subsystem or bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FeatureToggle {
    /// Subsystem is active.
    Enabled,
    /// Subsystem is disabled.
    #[default]
    Disabled,
}

impl FeatureToggle {
    /// Build a toggle from a boolean.
    #[must_use]
    pub const fn from_bool(enabled: bool) -> Self {
        if enabled {
            Self::Enabled
        } else {
            Self::Disabled
        }
    }

    /// Return true when enabled.
    #[must_use]
    pub const fn is_enabled(self) -> bool {
        matches!(self, Self::Enabled)
    }

    /// Return the stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Enabled => "enabled",
            Self::Disabled => "disabled",
        }
    }
}

/// Typed socket bind address (`host:port`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BindAddress(String);

impl BindAddress {
    /// Construct a bind address string.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    /// Return the bind address slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed session partition selector string validated by each bridge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionPartitionName(String);

impl SessionPartitionName {
    /// Construct a session partition selector.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into().trim().to_string())
    }

    /// Return the raw selector slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Typed external bot identity (e.g. Discord bot user snowflake id).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExternalBotId(String);

impl ExternalBotId {
    /// Construct an external bot id when non-empty.
    #[must_use]
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let trimmed = value.into().trim().to_string();
        (!trimmed.is_empty()).then_some(Self(trimmed))
    }

    /// Return the underlying bot id slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Mention-requirement policy for shared external channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MentionPolicy {
    /// Require explicit `@bot` mention in guild/group channels.
    #[default]
    Required,
    /// Accept all non-bot channel messages without explicit mention.
    Optional,
}

impl MentionPolicy {
    /// Return true when an explicit mention is required.
    #[must_use]
    pub const fn requires_mention(self) -> bool {
        matches!(self, Self::Required)
    }
}

/// Validated HTTP(S) endpoint URL newtype.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServiceUrl(String);

impl ServiceUrl {
    /// Validate and normalize an HTTP(S) base URL.
    ///
    /// # Errors
    ///
    /// Returns [`CoveConfigError::InvalidUrl`] when blank or not `http://`/`https://`.
    pub fn new(field: &'static str, value: impl AsRef<str>) -> Result<Self, CoveConfigError> {
        let trimmed = value.as_ref().trim().trim_end_matches('/');
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            Ok(Self(trimmed.to_string()))
        } else {
            Err(CoveConfigError::InvalidUrl {
                field,
                value: value.as_ref().to_string(),
            })
        }
    }

    /// Return the normalized base URL without trailing slash.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validated route path starting with `/`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoutePath(String);

impl RoutePath {
    /// Normalize a route path so it always starts with `/`.
    ///
    /// # Errors
    ///
    /// Returns [`CoveConfigError::InvalidPath`] when the path is empty.
    pub fn new(field: &'static str, value: impl AsRef<str>) -> Result<Self, CoveConfigError> {
        let trimmed = value.as_ref().trim();
        if trimmed.is_empty() {
            return Err(CoveConfigError::InvalidPath { field });
        }
        if trimmed.starts_with('/') {
            Ok(Self(trimmed.to_string()))
        } else {
            Ok(Self(format!("/{trimmed}")))
        }
    }

    /// Return the normalized route path.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Configuration loading and validation errors.
#[derive(Debug, Error)]
pub enum CoveConfigError {
    /// Failed to read a TOML configuration file.
    #[error("failed to read config file `{path}`: {source}")]
    ReadFile {
        /// File path that could not be read.
        path: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Failed to parse TOML configuration.
    #[error("failed to parse TOML config: {0}")]
    ParseToml(#[from] toml::de::Error),
    /// Invalid HTTP(S) URL.
    #[error("invalid URL for `{field}`: `{value}` (expected http:// or https://)")]
    InvalidUrl {
        /// Field name.
        field: &'static str,
        /// Rejected value.
        value: String,
    },
    /// Blank route path.
    #[error("route path for `{field}` must not be empty")]
    InvalidPath {
        /// Field name.
        field: &'static str,
    },
    /// Invalid room id.
    #[error("room id for `{field}` must not be empty")]
    InvalidRoomId {
        /// Field name.
        field: &'static str,
    },
    /// Invalid boolean override in environment variables.
    #[error("invalid boolean for `{key}`: `{value}`")]
    InvalidBoolean {
        /// Environment variable key.
        key: &'static str,
        /// Rejected value.
        value: String,
    },
    /// Invalid integer override in environment variables.
    #[error("invalid integer for `{key}`: `{value}`")]
    InvalidInteger {
        /// Environment variable key.
        key: &'static str,
        /// Rejected value.
        value: String,
    },
}
