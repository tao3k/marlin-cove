// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

use marlin_cove::bridges::BridgeError;
use marlin_cove::bridges::discord::{
    DISCORD_MAX_MESSAGE_LENGTH, DiscordAclObservation, DiscordChannelId, DiscordGuildId,
    DiscordIngressContext, DiscordIngressPayload, DiscordMentionGate, DiscordSessionPartition,
    DiscordUserId, split_message_for_discord, translate_discord_ingress,
    validate_discord_acl_observation, verify_discord_ingress_secret,
};
use marlin_cove::protocol::{BridgeOrigin, ChannelId, RoomId, SessionKey};
use pretty_assertions::assert_eq;

#[test]
fn discord_chunker_splits_at_2000_char_limit() {
    let long = "a".repeat(4500);
    let chunks = split_message_for_discord(&long);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].chars().count(), DISCORD_MAX_MESSAGE_LENGTH);
    assert_eq!(chunks[1].chars().count(), DISCORD_MAX_MESSAGE_LENGTH);
    assert_eq!(chunks[2].chars().count(), 500);
}

#[test]
fn discord_mention_gate_filters_unmentioned_guild_messages_and_accepts_mentions() {
    let bot_id = DiscordUserId::new("999000").unwrap();
    let context = DiscordIngressContext {
        partition: DiscordSessionPartition::GuildChannelUser,
        mention_gate: DiscordMentionGate {
            require_mention: true,
            bot_user_id: Some(bot_id.clone()),
        },
        target_room_id: RoomId::new("cove-main").unwrap(),
        target_channel_id: ChannelId::new("general").unwrap(),
    };

    let unmentioned = DiscordIngressPayload {
        message_id: "m1".to_string(),
        guild_id: Some(DiscordGuildId::new("g1").unwrap()),
        channel_id: DiscordChannelId::new("c1").unwrap(),
        author_id: DiscordUserId::new("u42").unwrap(),
        author_name: "charlie".to_string(),
        author_is_bot: false,
        content: "random chatter".to_string(),
        mentioned_user_ids: Vec::new(),
        role_ids: Vec::new(),
        timestamp_unix: 1_700_000_200,
    };

    let ignored = translate_discord_ingress(&unmentioned, &context);
    assert!(matches!(ignored, Err(BridgeError::Ignored { .. })));

    let mut mentioned = unmentioned;
    mentioned.content = "<@999000> @security audit this".to_string();
    mentioned.mentioned_user_ids = vec![bot_id];

    let inbound = translate_discord_ingress(&mentioned, &context).unwrap();

    assert_eq!(inbound.origin, BridgeOrigin::Discord);
    assert_eq!(inbound.sender_id.as_str(), "discord:u42");
    assert_eq!(inbound.session_key.as_str(), "discord:g1:c1:u42");
    assert!(verify_discord_ingress_secret(Some("s1"), Some("s1")));
    assert!(!verify_discord_ingress_secret(Some("s1"), None));
}

#[test]
fn discord_acl_observation_validates_session_scope() {
    let expected_session = SessionKey::new("discord:g1:c1:u42").unwrap();
    let observations = vec![DiscordAclObservation {
        event: "discord.command.control_admin_required.replied".to_string(),
        recipient: "c1".to_string(),
        session_key: expected_session.clone(),
    }];

    let matched = validate_discord_acl_observation(
        "discord.command.control_admin_required.replied",
        "c1",
        &[expected_session],
        &observations,
    )
    .unwrap();
    assert_eq!(matched.recipient, "c1");
}
