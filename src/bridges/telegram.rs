// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Telegram bot bridge feature (`--features telegram`).
//!
//! Supports Markdown AST-aware 4096-char chunking, webhook translation,
//! outbound Bot API delivery, and group-profile session planning.

use std::collections::BTreeMap;
use std::fmt::{Display, Formatter};
use std::str::FromStr;

use pulldown_cmark::{Event, Options, Parser, TagEnd};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::bridges::{BridgeError, InboundBridgeMessage, verify_shared_secret};
use crate::protocol::{BridgeOrigin, ChannelId, PrincipalId, RoomId, SessionKey};

/// Maximum message length accepted by Telegram `sendMessage`.
pub const TELEGRAM_MAX_MESSAGE_LENGTH: usize = 4096;

const CHUNK_CONTINUED_PREFIX: &str = "[continued]\n";
const CHUNK_CONTINUES_SUFFIX: &str = "\n[continues]";

/// Telegram chat identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TelegramChatId(i64);

impl TelegramChatId {
    /// Construct a Telegram chat id.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Parse a decimal chat id string.
    #[must_use]
    pub fn parse(value: impl AsRef<str>) -> Option<Self> {
        value.as_ref().trim().parse().ok().map(Self)
    }

    /// Return the raw 64-bit Telegram chat id.
    #[must_use]
    pub const fn into_raw(self) -> i64 {
        self.0
    }

    /// Return true if this chat id refers to a Telegram group or supergroup.
    #[must_use]
    pub const fn is_group_chat(self) -> bool {
        self.0 < 0
    }
}

/// Telegram user identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TelegramUserId(i64);

impl TelegramUserId {
    /// Construct a Telegram user id.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Return the raw Telegram user id.
    #[must_use]
    pub const fn into_raw(self) -> i64 {
        self.0
    }
}

/// Telegram forum-topic thread identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TelegramThreadId(i64);

impl TelegramThreadId {
    /// Construct a Telegram thread id.
    #[must_use]
    pub const fn new(value: i64) -> Self {
        Self(value)
    }

    /// Return the raw Telegram thread id.
    #[must_use]
    pub const fn into_raw(self) -> i64 {
        self.0
    }
}

/// How incoming Telegram messages partition into logical conversation sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramSessionPartition {
    /// Share one session for everyone in the same chat.
    #[default]
    ChatOnly,
    /// Isolate by chat and user.
    ChatUser,
    /// Share one session for the same user across all chats.
    UserOnly,
    /// Isolate by chat, forum topic thread, and user.
    ChatThreadUser,
}

/// Failure returned when parsing an unknown session partition name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid Telegram session partition")]
pub struct TelegramSessionPartitionError;

impl TelegramSessionPartition {
    /// Build a deterministic [`SessionKey`] for a Telegram message.
    #[must_use]
    pub fn build_session_key(
        self,
        chat_id: TelegramChatId,
        user_id: TelegramUserId,
        thread_id: Option<TelegramThreadId>,
    ) -> SessionKey {
        let chat = chat_id.into_raw();
        let user = user_id.into_raw();
        let raw = match self {
            Self::ChatOnly => format!("telegram:{chat}"),
            Self::ChatUser => format!("telegram:{chat}:{user}"),
            Self::UserOnly => format!("telegram:{user}"),
            Self::ChatThreadUser => {
                let thread = thread_id.map_or(0, TelegramThreadId::into_raw);
                format!("telegram:{chat}:{thread}:{user}")
            }
        };
        SessionKey::from_trusted(raw)
    }
}

impl Display for TelegramSessionPartition {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::ChatOnly => "chat",
            Self::ChatUser => "chat_user",
            Self::UserOnly => "user",
            Self::ChatThreadUser => "chat_thread_user",
        };
        formatter.write_str(label)
    }
}

impl FromStr for TelegramSessionPartition {
    type Err = TelegramSessionPartitionError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chat" | "chat_only" | "chat-only" => Ok(Self::ChatOnly),
            "chat_user" | "chat-user" => Ok(Self::ChatUser),
            "user" | "user_only" | "user-only" => Ok(Self::UserOnly),
            "chat_thread_user" | "chat-thread-user" | "topic_user" | "topic-user" => {
                Ok(Self::ChatThreadUser)
            }
            _ => Err(TelegramSessionPartitionError),
        }
    }
}

/// Telegram chat type reported by the Bot API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TelegramChatType {
    /// Direct 1:1 chat.
    Private,
    /// Legacy basic group.
    Group,
    /// Supergroup or forum group.
    Supergroup,
    /// Broadcast channel.
    Channel,
}

impl TelegramChatType {
    fn parse_str(value: &str) -> Option<Self> {
        match value {
            "private" => Some(Self::Private),
            "group" => Some(Self::Group),
            "supergroup" => Some(Self::Supergroup),
            "channel" => Some(Self::Channel),
            _ => None,
        }
    }
}

/// Telegram Bot API webhook update payload (subset required by Cove).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramWebhookUpdate {
    /// Monotonic update id.
    pub update_id: i64,
    /// Inbound message object, if present.
    pub message: Option<TelegramInboundMessage>,
}

/// Telegram Bot API `Message` object subset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramInboundMessage {
    /// Unique message id inside the chat.
    pub message_id: i64,
    /// Optional forum topic thread id.
    #[serde(default)]
    pub message_thread_id: Option<TelegramThreadId>,
    /// Message author.
    pub from: Option<TelegramUser>,
    /// Target chat metadata.
    pub chat: TelegramChat,
    /// Unix epoch timestamp in seconds.
    pub date: u64,
    /// UTF-8 message text.
    #[serde(default)]
    pub text: Option<String>,
}

/// Telegram Bot API `User` object subset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramUser {
    /// Unique Telegram user id.
    pub id: TelegramUserId,
    /// Optional `@username`.
    #[serde(default)]
    pub username: Option<String>,
    /// User first name.
    pub first_name: String,
}

/// Telegram Bot API `Chat` object subset.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TelegramChat {
    /// Unique Telegram chat id.
    pub id: TelegramChatId,
    /// Chat kind (`private`, `group`, `supergroup`, `channel`).
    #[serde(rename = "type")]
    pub kind: TelegramChatType,
    /// Optional group title.
    #[serde(default)]
    pub title: Option<String>,
}

/// Verify the `X-Telegram-Bot-Api-Secret-Token` header against configured secret.
#[must_use]
pub fn verify_telegram_webhook_secret(expected: Option<&str>, provided: Option<&str>) -> bool {
    verify_shared_secret(expected, provided)
}

/// Typed routing context for translating a Telegram webhook update into Cove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramWebhookContext {
    /// Session partition policy.
    pub partition: TelegramSessionPartition,
    /// Target Cove room identifier.
    pub target_room_id: RoomId,
    /// Target Cove channel identifier.
    pub target_channel_id: ChannelId,
}

/// Translate a Telegram webhook update into a normalized [`InboundBridgeMessage`].
///
/// # Errors
///
/// Returns [`BridgeError::Ignored`] when the update has no text message, or
/// [`BridgeError::Protocol`] when room/channel identifiers are invalid.
pub fn translate_webhook_update(
    update: &TelegramWebhookUpdate,
    context: TelegramWebhookContext,
) -> Result<InboundBridgeMessage, BridgeError> {
    let Some(message) = &update.message else {
        return Err(BridgeError::Ignored {
            reason: "telegram update contains no message",
        });
    };
    let text = message
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(BridgeError::Ignored {
            reason: "telegram message contains no text",
        })?;

    let (user_id, sender_name) = message.from.as_ref().map_or_else(
        || (TelegramUserId::new(0), "telegram-anonymous".to_string()),
        |user| {
            let name = user.username.as_deref().map_or_else(
                || user.first_name.clone(),
                |username| format!("@{username}"),
            );
            (user.id, name)
        },
    );

    let session_key =
        context
            .partition
            .build_session_key(message.chat.id, user_id, message.message_thread_id);
    let sender_id = PrincipalId::new(format!("tg:{}", user_id.into_raw()))?;

    Ok(InboundBridgeMessage {
        origin: BridgeOrigin::Telegram,
        external_message_id: message.message_id.to_string(),
        external_channel_id: message.chat.id.into_raw().to_string(),
        sender_id,
        sender_name,
        target_room_id: context.target_room_id,
        target_channel_id: context.target_channel_id,
        session_key,
        content: text.to_string(),
        attachments: Vec::new(),
        timestamp_unix: message.date,
    })
}

/// Outbound Telegram Bot API delivery client.
#[derive(Debug, Clone)]
pub struct TelegramBotClient {
    api_base_url: String,
    bot_token: String,
    http: reqwest::Client,
}

#[derive(Debug, Serialize)]
struct TelegramSendMessageBody<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_thread_id: Option<i64>,
}

impl TelegramBotClient {
    /// Create a Telegram Bot API client.
    #[must_use]
    pub fn new(api_base_url: impl Into<String>, bot_token: impl Into<String>) -> Self {
        Self {
            api_base_url: api_base_url.into().trim_end_matches('/').to_string(),
            bot_token: bot_token.into().trim().to_string(),
            http: reqwest::Client::new(),
        }
    }

    /// Return the `sendMessage` endpoint URL for this bot.
    #[must_use]
    pub fn send_message_endpoint(&self) -> String {
        format!("{}/bot{}/sendMessage", self.api_base_url, self.bot_token)
    }

    /// Split a Markdown response into Telegram-safe chunks and send each chunk in order.
    ///
    /// # Errors
    ///
    /// Returns [`BridgeError::Delivery`] when any Telegram Bot API call fails.
    pub async fn send_chunked_message(
        &self,
        chat_id: TelegramChatId,
        thread_id: Option<TelegramThreadId>,
        content: &str,
    ) -> Result<usize, BridgeError> {
        let chunks = split_message_for_telegram(content);
        let total = chunks.len();
        for (index, chunk) in chunks.iter().enumerate() {
            let text = decorate_chunk_for_telegram(chunk, index, total);
            let body = TelegramSendMessageBody {
                chat_id: chat_id.into_raw(),
                text: &text,
                message_thread_id: thread_id.map(TelegramThreadId::into_raw),
            };
            self.http
                .post(self.send_message_endpoint())
                .json(&body)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
                .map_err(|source| BridgeError::Delivery {
                    message: source.to_string(),
                })?;
        }
        Ok(total)
    }
}

/// Reserve character count for continuation markers when chunking Telegram messages.
#[must_use]
pub fn chunk_marker_reserve_chars() -> usize {
    CHUNK_CONTINUED_PREFIX.chars().count() + CHUNK_CONTINUES_SUFFIX.chars().count()
}

/// Decorate one Telegram chunk with `[continued]` / `[continues]` markers when multi-part.
#[must_use]
pub fn decorate_chunk_for_telegram(chunk: &str, index: usize, total_chunks: usize) -> String {
    if total_chunks <= 1 {
        return chunk.to_string();
    }
    if index == 0 {
        format!("{chunk}{CHUNK_CONTINUES_SUFFIX}")
    } else if index + 1 == total_chunks {
        format!("{CHUNK_CONTINUED_PREFIX}{chunk}")
    } else {
        format!("{CHUNK_CONTINUED_PREFIX}{chunk}{CHUNK_CONTINUES_SUFFIX}")
    }
}

/// Split Markdown text into Telegram-safe chunks respecting AST block boundaries.
#[must_use]
pub fn split_message_for_telegram(message: &str) -> Vec<String> {
    let max_chunk_chars = TELEGRAM_MAX_MESSAGE_LENGTH.saturating_sub(chunk_marker_reserve_chars());
    if max_chunk_chars == 0
        || byte_index_after_n_chars(message, TELEGRAM_MAX_MESSAGE_LENGTH) == message.len()
    {
        return vec![message.to_string()];
    }

    if let Some(ast_chunks) = split_with_markdown_block_boundaries(message, max_chunk_chars) {
        return ast_chunks;
    }

    let mut chunks = Vec::new();
    let mut remaining = message;
    while !remaining.is_empty() {
        let max_boundary = byte_index_after_n_chars(remaining, max_chunk_chars);
        if max_boundary == remaining.len() {
            chunks.push(remaining.to_string());
            break;
        }
        let search_area = &remaining[..max_boundary];
        let chunk_end = choose_split_boundary(search_area, max_boundary, max_chunk_chars);
        chunks.push(remaining[..chunk_end].to_string());
        remaining = &remaining[chunk_end..];
    }
    chunks
}

fn split_with_markdown_block_boundaries(
    message: &str,
    max_chunk_chars: usize,
) -> Option<Vec<String>> {
    if !looks_like_markdown(message) {
        return None;
    }

    let mut boundaries: Vec<usize> = Parser::new_ext(message, Options::all())
        .into_offset_iter()
        .filter_map(|(event, range)| is_markdown_block_boundary(&event).then_some(range.end))
        .filter(|index| *index > 0 && *index < message.len())
        .collect();
    boundaries.sort_unstable();
    boundaries.dedup();
    if boundaries.is_empty() {
        return None;
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < message.len() {
        let relative_max = byte_index_after_n_chars(&message[start..], max_chunk_chars);
        let max_boundary = start + relative_max;
        if max_boundary >= message.len() {
            chunks.push(message[start..].to_string());
            break;
        }

        let chunk_end = boundaries
            .iter()
            .copied()
            .take_while(|boundary| *boundary <= max_boundary)
            .last()
            .filter(|boundary| *boundary > start)
            .unwrap_or_else(|| {
                let search_area = &message[start..max_boundary];
                start + choose_split_boundary(search_area, relative_max, max_chunk_chars)
            });
        if chunk_end <= start {
            return None;
        }
        chunks.push(message[start..chunk_end].to_string());
        start = chunk_end;
    }
    Some(chunks)
}

fn looks_like_markdown(message: &str) -> bool {
    message.contains('\n')
        || message.contains("```")
        || message.contains("# ")
        || message.contains("](")
        || message.contains("* ")
}

const fn is_markdown_block_boundary(event: &Event<'_>) -> bool {
    matches!(
        event,
        Event::End(
            TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_)
                | TagEnd::List(_)
                | TagEnd::Item
        ) | Event::Rule
    )
}

fn byte_index_after_n_chars(text: &str, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    text.char_indices()
        .nth(count)
        .map_or(text.len(), |(idx, _)| idx)
}

fn choose_split_boundary(search_area: &str, max_boundary: usize, max_chars: usize) -> usize {
    if let Some(pos) = search_area.rfind('\n')
        && search_area[..pos].chars().count() >= max_chars / 2
    {
        return pos + 1;
    }
    if let Some(pos) = search_area.rfind(' ') {
        return pos + 1;
    }
    max_boundary
}

/// Telegram group evidence extracted from bridge runtime logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramGroupRecord {
    /// Telegram chat id.
    pub chat_id: TelegramChatId,
    /// Telegram group title.
    pub title: String,
    /// Telegram chat type.
    pub chat_type: TelegramChatType,
    /// Session key emitted by runtime.
    pub session_key: SessionKey,
    /// Optional forum topic thread id.
    pub thread_id: Option<TelegramThreadId>,
}

/// Profile extraction error when expected Telegram groups are missing.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TelegramGroupProfileError {
    /// Required group titles were absent from the log stream.
    #[error("missing Telegram group titles in log: {}", .0.join(", "))]
    MissingTitles(Vec<String>),
}

/// Parsed Telegram group profile keyed by exact title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramGroupProfile {
    groups_by_title: BTreeMap<String, TelegramGroupRecord>,
}

impl TelegramGroupProfile {
    /// Parse a group profile from structured bridge logs and assert required titles exist.
    ///
    /// # Errors
    ///
    /// Returns [`TelegramGroupProfileError::MissingTitles`] when any title is absent.
    pub fn parse_required(
        log_text: &str,
        required_titles: &[&str],
    ) -> Result<Self, TelegramGroupProfileError> {
        let groups_by_title: BTreeMap<String, TelegramGroupRecord> = log_text
            .lines()
            .filter_map(parse_group_line)
            .map(|group| (group.title.clone(), group))
            .collect();

        let missing: Vec<String> = required_titles
            .iter()
            .filter(|title| !groups_by_title.contains_key(**title))
            .map(ToString::to_string)
            .collect();

        if missing.is_empty() {
            Ok(Self { groups_by_title })
        } else {
            Err(TelegramGroupProfileError::MissingTitles(missing))
        }
    }

    /// Look up one Telegram group record by title.
    #[must_use]
    pub fn group(&self, title: &str) -> Option<&TelegramGroupRecord> {
        self.groups_by_title.get(title)
    }

    /// Select unique group chats preserving first-seen order (explicit -> primary -> allowlist).
    #[must_use]
    pub fn select_admin_group_chats(
        explicit: Vec<TelegramChatId>,
        primary: Option<TelegramChatId>,
        allow_list: Vec<TelegramChatId>,
    ) -> Vec<TelegramChatId> {
        explicit
            .into_iter()
            .chain(primary)
            .chain(allow_list.into_iter().filter(|id| id.is_group_chat()))
            .fold(Vec::new(), |mut selected, id| {
                if !selected.contains(&id) {
                    selected.push(id);
                }
                selected
            })
    }
}

fn parse_group_line(line: &str) -> Option<TelegramGroupRecord> {
    let chat_id = TelegramChatId::new(parse_i64_after(line, "chat_id=Some(")?);
    let title = parse_quoted_after(line, "chat_title=Some(\"")?;
    let chat_type = TelegramChatType::parse_str(&parse_quoted_after(line, "chat_type=Some(\"")?)?;
    let session_key = SessionKey::new(parse_token_after(line, "session_key=")?).ok()?;
    let thread_id = parse_i64_after(line, "message_thread_id=Some(").map(TelegramThreadId::new);

    Some(TelegramGroupRecord {
        chat_id,
        title,
        chat_type,
        session_key,
        thread_id,
    })
}

fn parse_i64_after(line: &str, marker: &str) -> Option<i64> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find(')')?;
    rest[..end].parse().ok()
}

fn parse_quoted_after(line: &str, marker: &str) -> Option<String> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn parse_token_after(line: &str, marker: &str) -> Option<String> {
    let start = line.find(marker)? + marker.len();
    let rest = &line[start..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    Some(rest[..end].to_string())
}
