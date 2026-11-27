;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; Unit tests verifying Marlin Cove governance built directly on
;;; `agentic-ai-lab`'s Governance Profiles, Higher-Order Rule Combinators,
;;; and One-Shot AuthorizedEffectToken Strict Mediation.

(import :std/test
        (only-in :clan/poo/object .o .ref)
        (only-in :tao3k/agentic-ai-lab/modules/governance/policy/authorized-effect-token
                 agentic-ai-lab-semantic-root
                 agentic-ai-lab-effect-binding
                 agentic-ai-lab-token-validity
                 agentic-ai-lab-authorized-effect-token
                 agentic-ai-lab-token-validation-context)
        (only-in :tao3k/agentic-ai-lab/modules/governance/policy/strict-mediation
                 agentic-ai-lab-strict-mediation-state)
        (only-in "../governance/funcs.ss"
                 marlin-cove-assessment-ready?
                 marlin-cove-assess-invite
                 marlin-cove-assess-tier0-gate
                 marlin-cove-assess-room-turn
                 marlin-cove-mediate-tier0-gate-once))

(export cove-governance-test)

(def cove-governance-test
  (test-suite "Marlin Cove governance on agentic-ai-lab"
    (test-case "Tier-0 gate rejects AI agents and allows human owner (inert assessment)"
      (def bot-assessment
        (marlin-cove-assess-tier0-gate (.o actor-kind: 'agent actor-role: 'owner)))
      (def human-assessment
        (marlin-cove-assess-tier0-gate (.o actor-kind: 'human actor-role: 'owner)))
      (check-equal? (marlin-cove-assessment-ready? bot-assessment) #f)
      (check-equal? (marlin-cove-assessment-ready? human-assessment) #t)
      (check-equal? (.ref human-assessment 'runtime-executed?) #f))

    (test-case "Invite profile blocks non-maintainers and external guest scope escalation"
      (def member-attempt
        (marlin-cove-assess-invite
         (.o actor-kind: 'human actor-role: 'member
             grant-role: 'external-guest grant-scopes: '("public"))))
      (def escalation-attempt
        (marlin-cove-assess-invite
         (.o actor-kind: 'human actor-role: 'owner
             grant-role: 'external-guest grant-scopes: '("org:core"))))
      (def valid-invite
        (marlin-cove-assess-invite
         (.o actor-kind: 'human actor-role: 'owner
             grant-role: 'external-guest grant-scopes: '("public"))))
      (check-equal? (marlin-cove-assessment-ready? member-attempt) #f)
      (check-equal? (marlin-cove-assessment-ready? escalation-attempt) #f)
      (check-equal? (marlin-cove-assessment-ready? valid-invite) #t))

    (test-case "Room turn profile enforces bot depth ceiling and Telegram/Discord mention policy"
      (def bot-loop
        (marlin-cove-assess-room-turn
         (.o origin: 'cove-native actor-role: 'role-agent bot-chain-depth: 3)
         max-bot-chain-depth: 2))
      (def unmentioned-tg
        (marlin-cove-assess-room-turn
         (.o origin: 'telegram actor-role: 'external-guest
             granted-scopes: '("public") require-mention: #t
             bot-mentioned: #f is-dm: #f bot-chain-depth: 0)))
      (def mentioned-tg
        (marlin-cove-assess-room-turn
         (.o origin: 'telegram actor-role: 'external-guest
             granted-scopes: '("public") require-mention: #t
             bot-mentioned: #t is-dm: #f bot-chain-depth: 0)))
      (check-equal? (marlin-cove-assessment-ready? bot-loop) #f)
      (check-equal? (marlin-cove-assessment-ready? unmentioned-tg) #f)
      (check-equal? (marlin-cove-assessment-ready? mentioned-tg) #t))

    (test-case "Tier-0 gate one-shot Strict Mediation prevents token replay"
      (def root (agentic-ai-lab-semantic-root "b1" "p1" "e1" "d1" "i1"))
      (def binding
        (agentic-ai-lab-effect-binding
         "b1" 1 "p1" "e1" "d1" "i1" "attempt-1" 'tier0-gate "cove-relay"
         "room:cove-main" 0 1 "arena-0" 1 0 32 "pld-1" "lease-1"))
      (def validity (agentic-ai-lab-token-validity 100 100 500 1))
      (def token
        (agentic-ai-lab-authorized-effect-token
         "tok-1" "nonce-1" root binding validity 'strict '(human-signature) "human:owner" "sig-1"))
      (def vctx (agentic-ai-lab-token-validation-context root binding 200 1 '()))
      (def state0 (agentic-ai-lab-strict-mediation-state "root-0" 0 '() 1))

      ;; First mediation with human owner commits and consumes nonce-1
      (def res1
        (marlin-cove-mediate-tier0-gate-once
         (.o actor-kind: 'human actor-role: 'owner)
         state0 token vctx "root-0" "obs-digest-1"))
      (check-equal? (.ref res1 'accepted?) #t)
      (check-equal? (.ref res1 'code) 'root-committed)

      ;; Second mediation replaying the same token MUST be rejected as token-reuse
      (def state1 (.ref res1 'state))
      (def res2
        (marlin-cove-mediate-tier0-gate-once
         (.o actor-kind: 'human actor-role: 'owner)
         state1 token vctx (.ref state1 'execution-root) "obs-digest-2"))
      (check-equal? (.ref res2 'accepted?) #f)
      (check-equal? (.ref res2 'code) 'token-reuse))))
