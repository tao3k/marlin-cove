// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Integration and unit test suite for Marlin Cove across Cargo feature permutations.

#[path = "unit/config.rs"]
mod config;
#[cfg(feature = "discord")]
#[path = "unit/discord.rs"]
mod discord;
#[path = "unit/lib_policy.rs"]
mod lib_policy;
#[path = "unit/protocol.rs"]
mod protocol;
#[cfg(feature = "relay")]
#[path = "unit/relay_e2e.rs"]
mod relay_e2e;
#[cfg(feature = "telegram")]
#[path = "unit/telegram.rs"]
mod telegram;
