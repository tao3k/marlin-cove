// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

use marlin_cove::bridges::telegram::{
    TELEGRAM_MAX_MESSAGE_LENGTH, TelegramChat, TelegramChatId, TelegramChatType,
    TelegramGroupProfile, TelegramInboundMessage, TelegramSessionPartition, TelegramThreadId,
    TelegramUser, TelegramUserId, TelegramWebhookContext, TelegramWebhookUpdate,
    decorate_chunk_for_telegram, split_message_for_telegram, translate_webhook_update,
    verify_telegram_webhook_secret,
};
use marlin_cove::protocol::{BridgeOrigin, ChannelId, PrincipalRole, RoomId, ScopeId};
use pretty_assertions::assert_eq;

#[test]
fn telegram_markdown_chunker_preserves_size_limits() {
    let block = "## Section\n\nThis is a markdown paragraph with **bold** text.\n\n";
    let long_message = block.repeat(95);
    let chunks = split_message_for_telegram(&long_message);
    assert!(chunks.len() >= 2);
    for (index, chunk) in chunks.iter().enumerate() {
        let decorated = decorate_chunk_for_telegram(chunk, index, chunks.len());
        assert!(decorated.chars().count() <= TELEGRAM_MAX_MESSAGE_LENGTH);
    }
}

#[test]
fn telegram_webhook_update_translates_into_cove_and_marlin_gateway_envelope() {
    let update = TelegramWebhookUpdate {
        update_id: 9001,
        message: Some(TelegramInboundMessage {
            message_id: 77,
            message_thread_id: Some(TelegramThreadId::new(12)),
            from: Some(TelegramUser {
                id: TelegramUserId::new(501),
                username: Some("alice".to_string()),
                first_name: "Alice".to_string(),
            }),
            chat: TelegramChat {
                id: TelegramChatId::new(-100_123_456),
                kind: TelegramChatType::Supergroup,
                title: Some("Marlin Eng".to_string()),
            },
            date: 1_700_000_100,
            text: Some("@architect review the Cove relay PR".to_string()),
        }),
    };

    let inbound = translate_webhook_update(
        &update,
        TelegramWebhookContext {
            partition: TelegramSessionPartition::ChatThreadUser,
            target_room_id: RoomId::new("cove-main").unwrap(),
            target_channel_id: ChannelId::new("general").unwrap(),
        },
    )
    .unwrap();

    assert_eq!(inbound.origin, BridgeOrigin::Telegram);
    assert_eq!(inbound.sender_id.as_str(), "tg:501");
    assert_eq!(inbound.sender_name, "@alice");
    assert_eq!(inbound.session_key.as_str(), "telegram:-100123456:12:501");

    let gateway_req = inbound.into_marlin_gateway_request(
        PrincipalRole::ExternalGuest,
        vec![ScopeId::new("public").unwrap()],
        Some("architect".to_string()),
    );
    assert!(gateway_req.governance.tainted_external);
    assert_eq!(gateway_req.governance.origin, BridgeOrigin::Telegram);
    assert!(verify_telegram_webhook_secret(
        Some("secret-1"),
        Some("secret-1")
    ));
    assert!(!verify_telegram_webhook_secret(
        Some("secret-1"),
        Some("wrong")
    ));
}

#[test]
fn telegram_group_profile_extracts_required_groups() {
    let log = r#"info chat_id=Some(-10088) chat_title=Some("Marlin Cove") chat_type=Some("supergroup") session_key=telegram:-10088 message_thread_id=Some(5)"#;
    let profile = TelegramGroupProfile::parse_required(log, &["Marlin Cove"]).unwrap();
    let group = profile.group("Marlin Cove").unwrap();
    assert_eq!(group.chat_id, TelegramChatId::new(-10088));
    assert_eq!(group.thread_id, Some(TelegramThreadId::new(5)));
}
