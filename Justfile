# SPDX-FileCopyrightText: 2026 tao3k team and Contributors
#
# SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

set dotenv-load := true

fmt:
    cargo fmt --check

test:
    cargo test --all-features

test-feature-matrix:
    cargo test --no-default-features
    cargo test --no-default-features --features telegram
    cargo test --no-default-features --features discord
    cargo test --no-default-features --features relay
    cargo test --no-default-features --features relay,telegram
    cargo test --no-default-features --features relay,discord
    cargo test --all-features

clippy:
    cargo clippy --all-targets --all-features -- -D warnings

check-config:
    cargo run -- check-config

preflight:
    cargo run -- preflight

plan-live:
    cargo run -- plan-live

serve:
    cargo run -- serve

check: fmt test-feature-matrix clippy
