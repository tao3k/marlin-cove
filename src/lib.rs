// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Marlin Cove — Cloud Room Relay, modular Telegram/Discord bot bridges, and
//! high-order room/people/bot governance for AI-native collaboration.
//!
//! Governance responsibilities are cleanly separated across two layers:
//!
//! - **Marlin Cove (`marlin-cove`)**: Owns room membership, scoped invites,
//!   Telegram/Discord bridge admission, anti-loop guardrails, human-only Tier-0
//!   gate routing, and Gerbil Scheme (`agentic-ai-lab`) high-order room/people/bot
//!   governance policies (`scheme/governance/`).
//! - **Marlin Gateway (`marlin-agent-core`)**: Owns `.org` context storage and
//!   agent-side atomic context projection using the verified
//!   [`protocol::RoomGovernanceEnvelope`] forwarded by Cove.
//!
//! Each runtime surface is an opt-in Cargo feature: `relay`, `telegram`, `discord`.

/// Feature-gated Telegram and Discord bot bridge adapters.
pub mod bridges;
/// Command-line entrypoint.
pub mod cli;
/// Typed TOML + environment configuration.
pub mod config;
/// Typed client boundary for Marlin Gateway (`marlin-agent-core`).
pub mod marlin_gateway;
/// Typed Cove domain protocol.
pub mod protocol;
/// Cloud Cove Room Relay server and state store.
#[cfg(feature = "relay")]
pub mod relay;

#[cfg(test)]
rust_lang_project_harness::rust_project_harness_cargo_test_gate!(
    config = {
        rust_lang_project_harness::default_rust_harness_config().with_verification_profile_hint(
            rust_lang_project_harness::RustVerificationProfileHint::new(
                "src/lib.rs",
                [rust_lang_project_harness::RustOwnerResponsibility::PublicApi],
            )
            .with_rationale("crate root owns the public package API for cargo-test verification"),
        )
    }
);
