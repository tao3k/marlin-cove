// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

use std::collections::BTreeMap;

use marlin_cove::config::{
    CoveConfig, CoveConfigSummary, FeatureToggle, LaunchPlan, MentionPolicy, PreflightReport,
    SecretState,
};
use pretty_assertions::assert_eq;

#[test]
fn toml_config_loads_and_redacts_secrets_in_summary() {
    let raw = r#"
[relay]
state = "enabled"
bind_addr = "127.0.0.1:19090"
auth_token = "super-secret-relay-token"
max_bot_chain_depth = 2

[marlin_gateway]
base_url = "http://127.0.0.1:18093"
bearer_token = "super-secret-marlin-gateway-token"

[telegram]
state = "enabled"
bot_token = "tg-secret-token"
webhook_secret = "tg-webhook-secret"

[discord]
state = "enabled"
bot_token = "discord-secret-token"
ingress_secret = "discord-ingress-secret"
mention_policy = "required"

[governance]
default_guest_scope = "public"
scheme_policy_entry = "scheme/governance/interface.ss"
"#;

    let config = CoveConfig::from_toml_str(raw).unwrap();
    let summary = CoveConfigSummary::from_config(&config);
    let rendered = summary.render_lines().join("\n");

    assert_eq!(summary.relay_auth_token, SecretState::Configured);
    assert_eq!(summary.marlin_gateway_token, SecretState::Configured);
    assert_eq!(summary.telegram_bot_token, SecretState::Configured);
    assert_eq!(summary.discord_bot_token, SecretState::Configured);
    assert!(!rendered.contains("super-secret-relay-token"));
    assert!(!rendered.contains("super-secret-marlin-gateway-token"));
    assert!(!rendered.contains("tg-secret-token"));
    assert!(!rendered.contains("discord-secret-token"));
}

#[test]
fn env_overlay_enables_bridges_and_marlin_gateway() {
    let vars = BTreeMap::from([
        ("MARLIN_COVE_RELAY_BIND", "127.0.0.1:18888"),
        ("MARLIN_COVE_MARLIN_GATEWAY_URL", "http://127.0.0.1:19999"),
        ("MARLIN_COVE_TELEGRAM_BOT_TOKEN", "tg-env-token"),
        ("MARLIN_COVE_DISCORD_BOT_TOKEN", "dc-env-token"),
        ("MARLIN_COVE_DISCORD_REQUIRE_MENTION", "false"),
    ]);

    let config = CoveConfig::default()
        .with_env_lookup(|key| vars.get(key).map(|value| (*value).to_string()))
        .unwrap();

    assert_eq!(config.relay.bind_addr.as_str(), "127.0.0.1:18888");
    assert_eq!(
        config.marlin_gateway.base_url.as_str(),
        "http://127.0.0.1:19999"
    );
    assert_eq!(config.telegram.state, FeatureToggle::Enabled);
    assert_eq!(config.discord.state, FeatureToggle::Enabled);
    assert_eq!(config.discord.mention_policy, MentionPolicy::Optional);

    let report = PreflightReport::evaluate(&config);
    let plan = LaunchPlan::from_config(&config);
    if cfg!(all(
        feature = "relay",
        feature = "telegram",
        feature = "discord"
    )) {
        assert!(report.is_ready());
        assert_eq!(plan.surfaces.len(), 3);
    }
}
