// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

//! Binary entrypoint for Marlin Cove.

use std::process::ExitCode;

fn main() -> ExitCode {
    marlin_cove::cli::run(std::env::args())
}
