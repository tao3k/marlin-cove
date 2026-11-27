<!--
SPDX-FileCopyrightText: 2026 tao3k team and Contributors

SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later
-->

# Marlin Cove (`marlin-cove`)

**Marlin Cove** is the **gRPC over HTTP/2** Cloud Room Relay, modular
Telegram/Discord bot bridge, and high-order room/people/bot governance service
for Marlin's AI-native collaboration rooms.

## Clean Architectural Separation

1. **User Configuration & Deployment (`TOML`)**:
   Typed `cove.toml` envelopes (`CoveConfig`) with `MARLIN_COVE_*` environment
   overlays govern all user-facing settings (gRPC bind address, optional
   external webhook bind address, capacity limits, Marlin Gateway gRPC endpoint,
   and Telegram/Discord bridge toggles & secrets).
2. **Marlin Cove Governance Plane (`scheme/` + `formal/` + Rust typed boundary)**:
   - `scheme/gerbil.pkg` imports `agentic-ai-lab` using the exact same Yuanxu +
     POO convention as `marlin-agent-core/scheme`.
   - `scheme/governance/` builds on `agentic-ai-lab`'s generic higher-order rule
     combinators for room membership, scoped invites, external bot admission,
     bot-loop ceilings, and human-only Tier-0 `PromptGate` approvals.
   - `formal/cove_security.qnt` is formally verified with Quint
     (`AllSecurityInvariants`) alongside `agentic-ai-lab` executable refinement
     and `deterministic-engineering`'s `CapacityModel`.
   - Rust owns typed domain newtypes, receipts, **Protobuf v3 + `tonic` gRPC over
     HTTP/2** transport (`proto/marlin/cove/v1/cove.proto`), and optional
     third-party HTTP webhook ingress strictly for Telegram/Discord.
3. **Marlin Gateway (`marlin-agent-core`)**:
   - `.org` context storage and atomic context projection (`org-scope`) belong
     exclusively to `marlin-agent-core`.
   - `marlin-cove` forwards a verified `RoomGovernanceEnvelope` (`room_id`,
     `channel_id`, `actor_id`, `actor_kind`, `actor_role`, `granted_scopes`,
     `origin`, `tainted_external`) over **gRPC HTTP/2** (`MarlinGatewayService`)
     so each agent enforces its own `.org` context boundaries without
     duplicating context storage in Cove.

## Modular Cargo Features

| Feature | Purpose |
| --- | --- |
| `relay` | Pure `tonic` gRPC over HTTP/2 Cloud Room Relay (`marlin.cove.v1.CoveRelayService` + server-streaming `WatchRoomEvents`, Human-Only Tier-0 `PromptGate`, `CapacityModel` bounds) |
| `telegram` | Telegram Bot API HTTP webhook ingress, Markdown AST-aware 4096-char chunking (`pulldown-cmark`), outbound Bot API sender, group profile & session partition helpers |
| `discord` | Discord HTTP ingress adapter, mention gate, 2000-char chunking, outbound REST sender, and ACL validation helpers |
| `full` / `default` | Enables `relay`, `telegram`, and `discord` |

Compile only what your deployment needs:

```shell
# Minimal protocol + Protobuf codec + typed TOML config + Marlin Gateway gRPC HTTP/2 client
cargo build --no-default-features

# Pure gRPC HTTP/2 Cloud Cove Relay only (no external chat bots, no HTTP listener)
cargo build --no-default-features --features relay

# Cloud Cove gRPC Relay + Telegram bot bridge only
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
cargo run -- plan-live --config cove.example.toml

# Run the live gRPC HTTP/2 Cloud Cove Relay (+ optional external bot webhooks)
cargo run -- serve --config cove.example.toml
```

## Protobuf v3 / gRPC over HTTP/2 Contract (`proto/marlin/cove/v1/cove.proto`)

- **`marlin.cove.v1.CoveRelayService`** (bound to `relay.grpc_bind_addr`, default `127.0.0.1:18090`):
  - `Health`, `ListRooms`, `CreateRoom`, `GetRoomSnapshot`, `GetGovernanceEnvelope`
  - `CreateInvite`, `RedeemInvite` (Quint-verified anti-hijack & scope-subset protection)
  - `PostMessage`, `DispatchTurn` (live gRPC HTTP/2 handoff to `MarlinGatewayService`)
  - `RequestGate`, `ResolveGate` (**Human-Only** Owner/Maintainer, immutable once resolved)
  - `WatchRoomEvents` (native HTTP/2 server-streaming `stream RoomEvent`)
- **`marlin.app.v1.AppServer`** (native `marlin-agent-core` gRPC HTTP/2 endpoint at `marlin_gateway.grpc_endpoint`, default `http://127.0.0.1:18093` — **zero** `marlin-cove` crate dependency in `marlin-agent-core`):
  - `StreamTurn(StreamTurnRequest) -> stream TurnEvent` carrying Cove's verified `RoomGovernanceEnvelope` in `x-marlin-cove-*` HTTP/2 gRPC metadata headers
- **External Third-Party HTTP Webhooks** (mounted on `relay.webhook_bind_addr`, default `127.0.0.1:18091`, **only** when `telegram` or `discord` is enabled):
  - `POST /v1/bridges/telegram/webhook` (`telegram`)
  - `POST /v1/bridges/discord/ingress` (`discord`)

## Environment Overrides (`MARLIN_COVE_*`)

- `MARLIN_COVE_RELAY_GRPC_BIND` (default `127.0.0.1:18090`)
- `MARLIN_COVE_RELAY_WEBHOOK_BIND` (default `127.0.0.1:18091`)
- `MARLIN_COVE_RELAY_AUTH_TOKEN`
- `MARLIN_COVE_MARLIN_GATEWAY_ENDPOINT` (default `http://127.0.0.1:18093`)
- `MARLIN_COVE_MARLIN_GATEWAY_TOKEN`
- `MARLIN_COVE_TELEGRAM_BOT_TOKEN` / `MARLIN_COVE_TELEGRAM_WEBHOOK_SECRET`
- `MARLIN_COVE_DISCORD_BOT_TOKEN` / `MARLIN_COVE_DISCORD_BOT_USER_ID` / `MARLIN_COVE_DISCORD_INGRESS_SECRET`

## Verification

```shell
just check
```
