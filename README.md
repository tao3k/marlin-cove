<!--
SPDX-FileCopyrightText: 2026 tao3k team and Contributors

SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later
-->

# Marlin Cove (`marlin-cove`)

**Marlin Cove** is the cloud room relay, modular Telegram/Discord bot bridge,
and high-order room/people/bot governance service for Marlin's AI-native
collaboration rooms.

## Clean Architectural Separation

1. **User Configuration & Deployment (`TOML`)**:
   Typed `cove.toml` envelopes (`CoveConfig`) with `MARLIN_COVE_*` environment
   overlays govern all user-facing settings (relay bind address, Marlin Gateway
   endpoint, Telegram/Discord bridge toggles & secrets).
2. **Marlin Cove Governance Plane (`scheme/` + Rust typed boundary)**:
   - `scheme/gerbil.pkg` imports `agentic-ai-lab` using the exact same Yuanxu +
     POO convention as `marlin-agent-core/scheme`.
   - `scheme/governance/` owns high-order policy combinators (`compose-policies`,
     `when-role`, `when-bridge`, `require-human-principal`) for room membership,
     scoped invites, external bot admission, bot-loop ceilings, and human-only
     Tier-0 PromptGate approvals.
   - Rust owns typed domain newtypes, receipts, Axum HTTP/SSE relay transport,
     and bridge webhooks.
3. **Marlin Gateway (`marlin-agent-core`)**:
   - `.org` context storage and atomic context projection (`org-scope`) belong
     exclusively to `marlin-agent-core`.
   - `marlin-cove` forwards a verified `RoomGovernanceEnvelope` (`room_id`,
     `actor_id`, `actor_role`, `granted_scopes`, `origin`, `tainted_external`)
     to **Marlin Gateway**, letting Agent A / Agent B enforce their own `.org`
     context boundaries without duplicating context storage in Cove.

## Modular Cargo Features

| Feature | Purpose |
| --- | --- |
| `relay` | Axum HTTP + SSE Cloud Room Relay (`/v1/rooms`, `/v1/invites`, `/v1/rooms/{id}/events`, `/v1/rooms/{id}/governance/{principal_id}`, Human-Only Tier-0 PromptGates) |
| `telegram` | Telegram Bot API webhook ingress, Markdown AST-aware 4096-char chunking (`pulldown-cmark`), outbound Bot API sender, group profile & session partition helpers |
| `discord` | Discord ingress adapter, mention gate, 2000-char chunking, outbound REST sender, and ACL validation helpers |
| `full` / `default` | Enables `relay`, `telegram`, and `discord` |

Compile only what your deployment needs:

```shell
# Minimal protocol + typed TOML config + Marlin Gateway client
cargo build --no-default-features

# Cloud Cove Relay only (no external chat bots)
cargo build --no-default-features --features relay

# Cloud Cove Relay + Telegram bot bridge only
cargo build --no-default-features --features relay,telegram

# Full Marlin Cove deployment
cargo build --all-features
```

## Quick Start

```shell
# Inspect compiled features & secret-safe configuration
cargo run -- features
cargo run -- check-config --config cove.example.toml
cargo run -- preflight --config cove.example.toml

# Run the live Cloud Cove Relay + enabled bot webhooks
cargo run -- serve --config cove.example.toml
```

## HTTP / SSE Relay Contract (for `marlin-ui` & Marlin Gateway)

- `GET /v1/health` — liveness + compiled Cargo features
- `GET /v1/rooms` / `POST /v1/rooms` — list or create Cove rooms
- `GET /v1/rooms/{room_id}/snapshot` — full room snapshot (channels, humans + AI agents, messages, Tier-0 gates)
- `GET /v1/rooms/{room_id}/governance/{principal_id}` — verified `RoomGovernanceEnvelope` handed off to Marlin Gateway
- `POST /v1/rooms/{room_id}/invites` — issue scoped invite token (`role` + `scopes` + `max_uses`)
- `POST /v1/invites/{token}/join` — redeem invite and admit human or AI agent
- `POST /v1/rooms/{room_id}/messages` — post message with `@mention` routing and bot-loop depth guardrail
- `GET /v1/rooms/{room_id}/events` — live Server-Sent Events stream (`room_event`)
- `POST /v1/rooms/{room_id}/gates` — request Tier-0 governance gate
- `POST /v1/rooms/{room_id}/gates/{gate_id}/resolve` — **Human-Only** gate resolution (AI agent actors return `403 Forbidden`)
- `POST /v1/bridges/telegram/webhook` (`telegram`) — Telegram Bot API webhook ingress
- `POST /v1/bridges/discord/ingress` (`discord`) — Discord ingress webhook

## Environment Overrides (`MARLIN_COVE_*`)

- `MARLIN_COVE_RELAY_BIND` (default `127.0.0.1:18090`)
- `MARLIN_COVE_RELAY_AUTH_TOKEN`
- `MARLIN_COVE_MARLIN_GATEWAY_URL` (default `http://127.0.0.1:18093`)
- `MARLIN_COVE_MARLIN_GATEWAY_TOKEN`
- `MARLIN_COVE_TELEGRAM_BOT_TOKEN` / `MARLIN_COVE_TELEGRAM_WEBHOOK_SECRET`
- `MARLIN_COVE_DISCORD_BOT_TOKEN` / `MARLIN_COVE_DISCORD_BOT_USER_ID` / `MARLIN_COVE_DISCORD_INGRESS_SECRET`

## Verification

```shell
just check
```
