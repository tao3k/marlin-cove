// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Discord bot bridge feature (`--features discord`).
//!
//! Supports Discord 2000-char chunking, mention gating, ingress translation,
//! outbound REST delivery, and deterministic ACL verification.

use std::fmt::{Display, Formatter};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bridges::{BridgeError, InboundBridgeMessage, verify_shared_secret};
use crate::protocol::{BridgeOrigin, ChannelId, PrincipalId, ProtocolError, RoomId, SessionKey};

/// Maximum message length accepted by Discord `Create Message`.
pub const DISCORD_MAX_MESSAGE_LENGTH: usize = 2000;

macro_rules! define_discord_id {
    ($name:ident, $doc:literal, $field:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Construct a validated non-empty Discord snowflake identifier.
            ///
            /// # Errors
            ///
            /// Returns [`ProtocolError::EmptyIdentifier`] when blank.
            pub fn new(value: impl AsRef<str>) -> Result<Self, ProtocolError> {
                let trimmed = value.as_ref().trim();
                if trimmed.is_empty() {
                    return Err(ProtocolError::EmptyIdentifier { field: $field });
                }
                Ok(Self(trimmed.to_string()))
            }

            /// Return the raw Discord snowflake string.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

define_discord_id!(
    DiscordGuildId,
    "Discord guild (server) snowflake id.",
    "discord_guild_id"
);
define_discord_id!(
    DiscordChannelId,
    "Discord channel snowflake id.",
    "discord_channel_id"
);
define_discord_id!(
    DiscordUserId,
    "Discord user snowflake id.",
    "discord_user_id"
);
define_discord_id!(
    DiscordRoleId,
    "Discord role snowflake id.",
    "discord_role_id"
);

/// How incoming Discord messages map to a logical conversation session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscordSessionPartition {
    /// Isolate by guild (or `dm`), channel, and user.
    #[default]
    GuildChannelUser,
    /// Share one session per guild/channel.
    ChannelOnly,
    /// Share one session per user across all channels.
    UserOnly,
    /// Share one session per user inside one guild.
    GuildUser,
}

/// Failure returned when parsing an unknown Discord session partition name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid Discord session partition")]
pub struct DiscordSessionPartitionError;

impl DiscordSessionPartition {
    /// Build a deterministic [`SessionKey`] for a Discord message.
    #[must_use]
    pub fn build_session_key(
        self,
        guild_id: Option<&DiscordGuildId>,
        channel_id: &DiscordChannelId,
        user_id: &DiscordUserId,
    ) -> SessionKey {
        let scope = guild_id.map_or("dm", DiscordGuildId::as_str);
        let channel = channel_id.as_str();
        let user = user_id.as_str();
        let raw = match self {
            Self::GuildChannelUser => format!("discord:{scope}:{channel}:{user}"),
            Self::ChannelOnly => format!("discord:{scope}:{channel}"),
            Self::UserOnly => format!("discord:{user}"),
            Self::GuildUser => format!("discord:{scope}:{user}"),
        };
        SessionKey::from_trusted(raw)
    }
}

impl Display for DiscordSessionPartition {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::GuildChannelUser => "guild_channel_user",
            Self::ChannelOnly => "channel",
            Self::UserOnly => "user",
            Self::GuildUser => "guild_user",
        };
        formatter.write_str(label)
    }
}

impl FromStr for DiscordSessionPartition {
    type Err = DiscordSessionPartitionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "guild_channel_user" | "guild-channel-user" | "channel_user" => {
                Ok(Self::GuildChannelUser)
            }
            "channel" | "channel_only" | "channel-only" => Ok(Self::ChannelOnly),
            "user" | "user_only" | "user-only" => Ok(Self::UserOnly),
            "guild_user" | "guild-user" => Ok(Self::GuildUser),
            _ => Err(DiscordSessionPartitionError),
        }
    }
}

/// Mention-gating policy preventing bots from replying to every unmentioned channel message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordMentionGate {
    /// Whether an explicit bot mention is required in guild channels.
    pub require_mention: bool,
    /// Bot's own Discord user id.
    pub bot_user_id: Option<DiscordUserId>,
}

impl DiscordMentionGate {
    /// Return true when the Discord message should enter the Cove room pipeline.
    #[must_use]
    pub fn accepts_message(
        &self,
        is_dm: bool,
        content: &str,
        mentioned_user_ids: &[DiscordUserId],
    ) -> bool {
        if !self.require_mention || is_dm {
            return true;
        }
        let Some(bot_id) = self.bot_user_id.as_ref() else {
            return false;
        };
        let raw = bot_id.as_str();
        mentioned_user_ids.iter().any(|id| id == bot_id)
            || content.contains(&format!("<@{raw}>"))
            || content.contains(&format!("<@!{raw}>"))
    }
}

/// Inbound Discord bridge payload delivered by a Gateway relay or Discord interaction hook.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscordIngressPayload {
    /// Discord message snowflake id.
    pub message_id: String,
    /// Optional guild id (`None` indicates a direct message).
    #[serde(default)]
    pub guild_id: Option<DiscordGuildId>,
    /// Discord channel id.
    pub channel_id: DiscordChannelId,
    /// Author Discord user id.
    pub author_id: DiscordUserId,
    /// Author display username.
    pub author_name: String,
    /// Whether the author is a bot account (ignored to prevent bridge loops).
    #[serde(default)]
    pub author_is_bot: bool,
    /// Raw Discord message text.
    pub content: String,
    /// User ids explicitly mentioned by Discord metadata.
    #[serde(default)]
    pub mentioned_user_ids: Vec<DiscordUserId>,
    /// Author role ids for ACL checks.
    #[serde(default)]
    pub role_ids: Vec<DiscordRoleId>,
    /// Unix epoch timestamp in seconds.
    pub timestamp_unix: u64,
}

/// Verify the `X-Marlin-Cove-Ingress-Secret` header against configured secret.
#[must_use]
pub fn verify_discord_ingress_secret(expected: Option<&str>, provided: Option<&str>) -> bool {
    verify_shared_secret(expected, provided)
}

/// Typed routing context for translating a Discord ingress payload into Cove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordIngressContext {
    /// Session partition policy.
    pub partition: DiscordSessionPartition,
    /// Mention gate controlling guild channel admission.
    pub mention_gate: DiscordMentionGate,
    /// Target Cove room identifier.
    pub target_room_id: RoomId,
    /// Target Cove channel identifier.
    pub target_channel_id: ChannelId,
}

/// Translate a Discord ingress payload into a normalized [`InboundBridgeMessage`].
///
/// # Errors
///
/// Returns [`BridgeError::Ignored`] when the message is from a bot, blank, or
/// fails [`DiscordMentionGate`], or [`BridgeError::Protocol`] when identifiers
/// fail validation.
pub fn translate_discord_ingress(
    payload: &DiscordIngressPayload,
    context: &DiscordIngressContext,
) -> Result<InboundBridgeMessage, BridgeError> {
    if payload.author_is_bot {
        return Err(BridgeError::Ignored {
            reason: "discord bot-authored message ignored to prevent loops",
        });
    }
    let text = payload.content.trim();
    if text.is_empty() {
        return Err(BridgeError::Ignored {
            reason: "discord message content is empty",
        });
    }
    let is_dm = payload.guild_id.is_none();
    if !context
        .mention_gate
        .accepts_message(is_dm, text, &payload.mentioned_user_ids)
    {
        return Err(BridgeError::Ignored {
            reason: "discord message did not mention configured bot",
        });
    }

    let session_key = context.partition.build_session_key(
        payload.guild_id.as_ref(),
        &payload.channel_id,
        &payload.author_id,
    );
    let sender_id = PrincipalId::new(format!("discord:{}", payload.author_id.as_str()))?;

    Ok(InboundBridgeMessage {
        origin: BridgeOrigin::Discord,
        external_message_id: payload.message_id.clone(),
        external_channel_id: payload.channel_id.as_str().to_string(),
        sender_id,
        sender_name: payload.author_name.clone(),
        target_room_id: context.target_room_id.clone(),
        target_channel_id: context.target_channel_id.clone(),
        session_key,
        content: text.to_string(),
        attachments: Vec::new(),
        timestamp_unix: payload.timestamp_unix,
    })
}

/// Outbound Discord REST delivery client.
#[derive(Debug, Clone)]
pub struct DiscordBotClient {
    api_base_url: String,
    bot_token: String,
    http: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct DiscordCreateMessageBody<'a> {
    content: &'a str,
}

impl DiscordBotClient {
    /// Construct a Discord REST delivery client.
    #[must_use]
    pub fn new(api_base_url: impl Into<String>, bot_token: impl Into<String>) -> Self {
        Self {
            api_base_url: api_base_url.into().trim_end_matches('/').to_string(),
            bot_token: bot_token.into().trim().to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Return the `Create Message` endpoint URL for a channel.
    #[must_use]
    pub fn channel_messages_endpoint(&self, channel_id: &DiscordChannelId) -> String {
        format!(
            "{}/channels/{}/messages",
            self.api_base_url,
            channel_id.as_str()
        )
    }

    /// Split a message into Discord-safe 2000-character chunks and deliver sequentially.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::Delivery`] when any Discord REST call fails.
    pub async fn send_chunked_message(
        &self,
        channel_id: &DiscordChannelId,
        content: &str,
    ) -> Result<usize, BridgeError> {
        let chunks = split_message_for_discord(content);
        let endpoint = self.channel_messages_endpoint(channel_id);
        for chunk in &chunks {
            let body = DiscordCreateMessageBody { content: chunk };
            self.http
                .post(&endpoint)
                .header("Authorization", format!("Bot {}", self.bot_token))
                .json(&body)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|source| BridgeError::Delivery {
                    message: source.to_string(),
                })?;
        }
        Ok(chunks.len())
    }
}

/// Split text into Discord-safe 2000-character chunks.
#[must_use]
pub fn split_message_for_discord(message: &str) -> Vec<String> {
    split_message_for_discord_with_limit(message, DISCORD_MAX_MESSAGE_LENGTH)
}

/// Split text into Discord-safe chunks using an explicit character limit.
#[must_use]
pub fn split_message_for_discord_with_limit(message: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 || message.is_empty() {
        return Vec::new();
    }
    let chars: Vec<char> = message.chars().collect();
    chars
        .chunks(max_chars)
        .map(|chunk| chunk.iter().collect::<String>())
        .collect()
}

/// Deterministic ACL command-reply observation for Discord governance verification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordAclObservation {
    /// Emitted runtime event name.
    pub event: String,
    /// Target recipient channel/user id.
    pub recipient: String,
    /// Observed runtime session key.
    pub session_key: SessionKey,
}

/// Validation failure returned by [`validate_discord_acl_observation`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum DiscordAclValidationError {
    /// Target event/recipient pair was never observed.
    #[error("missing expected Discord ACL reply event")]
    MissingReply,
    /// Observed session key did not belong to the authorized session set.
    #[error("Discord ACL reply session mismatch: {observed}")]
    SessionMismatch {
        /// Unexpected session key.
        observed: SessionKey,
    },
}

/// Verify that an ACL denial reply was emitted on an expected session key.
///
/// # Errors
///
/// Returns [`DiscordAclValidationError`] when missing or scoped to the wrong session.
pub fn validate_discord_acl_observation<'a>(
    expected_event: &str,
    expected_recipient: &str,
    allowed_sessions: &[SessionKey],
    observations: &'a [DiscordAclObservation],
) -> Result<&'a DiscordAclObservation, DiscordAclValidationError> {
    let target = observations
        .iter()
        .find(|obs| obs.event == expected_event && obs.recipient == expected_recipient)
        .ok_or(DiscordAclValidationError::MissingReply)?;

    if !allowed_sessions.contains(&target.session_key) {
        return Err(DiscordAclValidationError::SessionMismatch {
            observed: target.session_key.clone(),
        });
    }
    Ok(target)
}
