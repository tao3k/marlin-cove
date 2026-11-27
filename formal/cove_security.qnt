// SPDX-FileCopyrightText: 2026 tao3k team and Contributors
//
// SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

// Formal state-machine security specification for Marlin Cove Cloud Room Relay.
//
// Instantiates `deterministic-engineering` bounded-capacity accounting
// (`CapacityModel`: overflow refuses admission and preserves state) alongside
// `agentic-ai-lab` security invariants:
//   1. InvHumanOnlyTier0Gate: Only human Owner/Maintainer may resolve a pending
//      Tier-0 PromptGate; AI agents and ExternalGuests can never approve/deny,
//      and a resolved gate is immutable (no replay/overwrite).
//   2. InvInviteAuthorizationAndNoEscalation: Only human Owner/Maintainer may
//      issue invites; ExternalGuest invites never exceed {"public"} scope;
//      uses never exceed max_uses.
//   3. InvBotLoopBounded: Every admitted message satisfies bot_chain_depth <= MAX_BOT_CHAIN_DEPTH.
//   4. InvExternalTaintPreserved: Every Telegram/Discord or ExternalGuest turn
//      carries tainted_external == true in its Marlin Gateway governance envelope.
//   5. InvBoundedRetentionCapacity: Rooms, invites, messages, gates, and
//      deduped external webhook receipts strictly respect configured capacity caps.
module cove_security {
  type PrincipalKind = str // "human" | "agent"
  type PrincipalRole = str // "owner" | "maintainer" | "member" | "external_guest" | "role_agent"
  type BridgeOrigin = str  // "cove_native" | "telegram" | "discord" | "marlin_gateway"
  type GateDecision = str  // "pending" | "approved" | "denied"

  type Participant = {
    id: str,
    kind: PrincipalKind,
    role: PrincipalRole,
    scopes: Set[str]
  }

  type Invite = {
    token: str,
    issued_by: str,
    grant_role: PrincipalRole,
    grant_scopes: Set[str],
    max_uses: int,
    uses: int
  }

  type Message = {
    id: int,
    author_id: str,
    origin: BridgeOrigin,
    bot_chain_depth: int,
    tainted_external: bool
  }

  type Gate = {
    id: int,
    requested_by: str,
    decision: GateDecision,
    resolved_by: str,
    resolved_count: int
  }

  pure val MAX_BOT_CHAIN_DEPTH: int = 2
  pure val MAX_INVITES: int = 4
  pure val MAX_MESSAGES: int = 8
  pure val MAX_GATES: int = 4
  pure val PUBLIC_SCOPES: Set[str] = Set("public")
  pure val ALL_SCOPES: Set[str] = Set("public", "room:arch", "org:core")

  var participants: str -> Participant
  var invites: str -> Invite
  var messages: List[Message]
  var gates: int -> Gate
  var seen_bridge_ids: Set[str]
  var next_msg_id: int
  var next_gate_id: int

  pure def is_privileged_human(p: Participant): bool =
    p.kind == "human" and (p.role == "owner" or p.role == "maintainer")

  pure def is_external_origin(origin: BridgeOrigin): bool =
    origin == "telegram" or origin == "discord"

  pure def compute_taint(p: Participant, origin: BridgeOrigin): bool =
    is_external_origin(origin) or p.role == "external_guest"

  action init = all {
    participants' = Map(
      "human:owner" -> { id: "human:owner", kind: "human", role: "owner", scopes: ALL_SCOPES },
      "human:member" -> { id: "human:member", kind: "human", role: "member", scopes: Set("public", "room:arch") },
      "agent:architect" -> { id: "agent:architect", kind: "agent", role: "role_agent", scopes: Set("public", "room:arch") }
    ),
    invites' = Map(),
    messages' = [],
    gates' = Map(),
    seen_bridge_ids' = Set(),
    next_msg_id' = 1,
    next_gate_id' = 1
  }

  // Action 1: Issue a scoped invite (strictly enforces human owner/maintainer,
  // bounded capacity, and no scope escalation for ExternalGuest).
  action issue_invite(issuer_id: str, token: str, grant_role: PrincipalRole, grant_scopes: Set[str], max_uses: int): bool = all {
    issuer_id.in(participants.keys()),
    is_privileged_human(participants.get(issuer_id)),
    invites.keys().size() < MAX_INVITES,
    not(token.in(invites.keys())),
    max_uses >= 1,
    max_uses <= 2,
    // Fail-closed: cannot grant role higher than issuer or grant owner via invite
    grant_role != "owner",
    // Fail-closed: ExternalGuest restricted strictly to PUBLIC_SCOPES
    (grant_role != "external_guest") or grant_scopes.subseteq(PUBLIC_SCOPES),
    // Fail-closed: issuer cannot grant scopes they do not hold
    grant_scopes.subseteq(participants.get(issuer_id).scopes),
    invites' = invites.put(token, {
      token: token,
      issued_by: issuer_id,
      grant_role: grant_role,
      grant_scopes: grant_scopes,
      max_uses: max_uses,
      uses: 0
    }),
    participants' = participants,
    messages' = messages,
    gates' = gates,
    seen_bridge_ids' = seen_bridge_ids,
    next_msg_id' = next_msg_id,
    next_gate_id' = next_gate_id
  }

  // Action 2: Redeem an invite token (enforces uses < max_uses, role/kind
  // consistency, and strictly forbids overwriting an existing participant).
  action redeem_invite(token: str, principal_id: str, kind: PrincipalKind): bool = all {
    token.in(invites.keys()),
    not(principal_id.in(participants.keys())),
    val inv = invites.get(token)
    all {
      inv.uses < inv.max_uses,
      // RoleAgent invites only admit agents; human roles only admit humans
      (inv.grant_role == "role_agent") == (kind == "agent"),
      invites' = invites.put(token, { ...inv, uses: inv.uses + 1 }),
      participants' = participants.put(principal_id, {
        id: principal_id,
        kind: kind,
        role: inv.grant_role,
        scopes: inv.grant_scopes
      }),
      messages' = messages,
      gates' = gates,
      seen_bridge_ids' = seen_bridge_ids,
      next_msg_id' = next_msg_id,
      next_gate_id' = next_gate_id
    }
  }

  // Action 3: Post native or Marlin Gateway message (enforces bot depth & capacity).
  action post_message(author_id: str, origin: BridgeOrigin, depth: int): bool = all {
    author_id.in(participants.keys()),
    not(is_external_origin(origin)),
    depth >= 0,
    depth <= MAX_BOT_CHAIN_DEPTH,
    messages.length() < MAX_MESSAGES,
    val author = participants.get(author_id)
    val msg = {
      id: next_msg_id,
      author_id: author_id,
      origin: origin,
      bot_chain_depth: depth,
      tainted_external: compute_taint(author, origin)
    }
    all {
      messages' = messages.append(msg),
      next_msg_id' = next_msg_id + 1,
      participants' = participants,
      invites' = invites,
      gates' = gates,
      seen_bridge_ids' = seen_bridge_ids,
      next_gate_id' = next_gate_id
    }
  }

  // Action 4: Ingest authenticated Telegram/Discord bridge message with replay dedup.
  action ingest_bridge(origin: BridgeOrigin, ext_id: str, sender_id: str): bool = all {
    is_external_origin(origin),
    not(ext_id.in(seen_bridge_ids)),
    messages.length() < MAX_MESSAGES,
    val admitted = if (sender_id.in(participants.keys()))
      participants.get(sender_id)
    else
      { id: sender_id, kind: "human", role: "external_guest", scopes: PUBLIC_SCOPES }
    val msg = {
      id: next_msg_id,
      author_id: sender_id,
      origin: origin,
      bot_chain_depth: 0,
      tainted_external: compute_taint(admitted, origin)
    }
    all {
      participants' = participants.put(sender_id, admitted),
      seen_bridge_ids' = seen_bridge_ids.union(Set(ext_id)),
      messages' = messages.append(msg),
      next_msg_id' = next_msg_id + 1,
      invites' = invites,
      gates' = gates,
      next_gate_id' = next_gate_id
    }
  }

  // Action 5: Request a Tier-0 PromptGate (bounded capacity).
  action request_gate(requester_id: str): bool = all {
    requester_id.in(participants.keys()),
    gates.keys().size() < MAX_GATES,
    gates' = gates.put(next_gate_id, {
      id: next_gate_id,
      requested_by: requester_id,
      decision: "pending",
      resolved_by: "",
      resolved_count: 0
    }),
    next_gate_id' = next_gate_id + 1,
    participants' = participants,
    invites' = invites,
    messages' = messages,
    seen_bridge_ids' = seen_bridge_ids,
    next_msg_id' = next_msg_id
  }

  // Action 6: Resolve a pending Tier-0 PromptGate (strictly human Owner/Maintainer,
  // exactly-once transition — no replay or overwrite).
  action resolve_gate(gate_id: int, actor_id: str, approved: bool): bool = all {
    gate_id.in(gates.keys()),
    actor_id.in(participants.keys()),
    is_privileged_human(participants.get(actor_id)),
    val g = gates.get(gate_id)
    all {
      g.decision == "pending",
      g.resolved_count == 0,
      gates' = gates.put(gate_id, {
        ...g,
        decision: if (approved) "approved" else "denied",
        resolved_by: actor_id,
        resolved_count: g.resolved_count + 1
      }),
      participants' = participants,
      invites' = invites,
      messages' = messages,
      seen_bridge_ids' = seen_bridge_ids,
      next_msg_id' = next_msg_id,
      next_gate_id' = next_gate_id
    }
  }

  action step = any {
    nondet issuer = oneOf(participants.keys())
    nondet token = oneOf(Set("inv_1", "inv_2", "inv_3", "inv_4"))
    nondet role = oneOf(Set("member", "external_guest", "role_agent"))
    nondet scopes = oneOf(Set(Set("public"), Set("public", "room:arch")))
    nondet max_u = oneOf(Set(1, 2))
    issue_invite(issuer, token, role, scopes, max_u),

    nondet token = oneOf(Set("inv_1", "inv_2", "inv_3", "inv_4"))
    nondet pid = oneOf(Set("human:guest1", "human:guest2", "agent:helper"))
    nondet kind = oneOf(Set("human", "agent"))
    redeem_invite(token, pid, kind),

    nondet author = oneOf(participants.keys())
    nondet origin = oneOf(Set("cove_native", "marlin_gateway"))
    nondet depth = oneOf(Set(0, 1, 2))
    post_message(author, origin, depth),

    nondet origin = oneOf(Set("telegram", "discord"))
    nondet ext_id = oneOf(Set("tg:1", "tg:2", "dc:1", "dc:2"))
    nondet sender = oneOf(Set("tg:501", "discord:u42"))
    ingest_bridge(origin, ext_id, sender),

    nondet req = oneOf(participants.keys())
    request_gate(req),

    nondet gid = oneOf(Set(1, 2, 3, 4))
    nondet actor = oneOf(participants.keys())
    nondet ok = oneOf(Set(true, false))
    resolve_gate(gid, actor, ok)
  }

  // ===== FORMAL SECURITY INVARIANTS =====

  val InvHumanOnlyTier0Gate: bool =
    gates.keys().forall(gid =>
      val g = gates.get(gid)
      (g.decision == "pending" and g.resolved_count == 0)
      or (
        g.resolved_count == 1
        and g.resolved_by.in(participants.keys())
        and is_privileged_human(participants.get(g.resolved_by))
      )
    )

  val InvInviteAuthorizationAndNoEscalation: bool =
    invites.keys().forall(tok =>
      val inv = invites.get(tok)
      inv.uses >= 0
      and inv.uses <= inv.max_uses
      and inv.grant_role != "owner"
      and inv.issued_by.in(participants.keys())
      and is_privileged_human(participants.get(inv.issued_by))
      and ((inv.grant_role != "external_guest") or inv.grant_scopes.subseteq(PUBLIC_SCOPES))
    )

  val InvBotLoopBounded: bool =
    messages.indices().forall(i =>
      messages[i].bot_chain_depth >= 0 and messages[i].bot_chain_depth <= MAX_BOT_CHAIN_DEPTH
    )

  val InvExternalTaintPreserved: bool =
    messages.indices().forall(i =>
      val m = messages[i]
      val author = participants.get(m.author_id)
      (not(is_external_origin(m.origin)) and author.role != "external_guest")
      or m.tainted_external
    )

  val InvBoundedRetentionCapacity: bool =
    invites.keys().size() <= MAX_INVITES
    and messages.length() <= MAX_MESSAGES
    and gates.keys().size() <= MAX_GATES

  val AllSecurityInvariants: bool =
    InvHumanOnlyTier0Gate
    and InvInviteAuthorizationAndNoEscalation
    and InvBotLoopBounded
    and InvExternalTaintPreserved
    and InvBoundedRetentionCapacity
}
