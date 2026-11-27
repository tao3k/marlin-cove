// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Modular external bot bridge adapters gated by Cargo features.

/// Discord bot bridge feature (`--features discord`).
#[cfg(feature = "discord")]
pub mod discord;
/// Normalized inbound/outbound bridge envelopes and shared secret verification.
pub mod envelope;
/// Telegram bot bridge feature (`--features telegram`).
#[cfg(feature = "telegram")]
pub mod telegram;

pub use envelope::{BridgeError, InboundBridgeMessage, verify_shared_secret};
