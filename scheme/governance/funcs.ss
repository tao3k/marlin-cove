;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; Marlin Cove High-Order Room / People / Bot Governance built directly on
;;; `agentic-ai-lab`'s native Governance POO Profiles, Higher-Order Rule
;;; Combinators, and One-Shot AuthorizedEffectToken Strict Mediation.
;;;
;;; Clean architectural boundary:
;;;   - User-facing deployment settings live in typed TOML (`cove.toml`).
;;;   - `.org` context governance stays inside `marlin-agent-core` (Marlin Gateway).
;;;   - Marlin Cove inherits `AgenticAILabGovernanceProfile.` to govern room
;;;     membership, scoped invites, Telegram/Discord bridge admission, bot-chain
;;;     depth ceilings, and Human-Only Tier-0 PromptGate approvals.

(import (only-in :clan/poo/object .o .ref)
        (only-in :tao3k/agentic-ai-lab/modules/governance/interface
                 AgenticAILabGovernanceProfile.
                 agentic-ai-lab-governance-evaluate
                 agentic-ai-lab-governance-rule
                 agentic-ai-lab-governance-rule-allow
                 agentic-ai-lab-governance-rule-deny
                 agentic-ai-lab-governance-compose-all
                 agentic-ai-lab-governance-guard-when
                 agentic-ai-lab-governance-context-ref
                 agentic-ai-lab-governance-require-principal-kind
                 agentic-ai-lab-governance-require-actor-roles
                 agentic-ai-lab-governance-require-scope-subset
                 agentic-ai-lab-governance-require-chain-depth-within
                 agentic-ai-lab-governance-rule-threat-model
                 agentic-ai-lab-governance-profile-extend)
        (only-in :tao3k/agentic-ai-lab/modules/governance/policy/authorized-effect-token
                 agentic-ai-lab-semantic-root
                 agentic-ai-lab-effect-binding
                 agentic-ai-lab-token-validity
                 agentic-ai-lab-authorized-effect-token
                 agentic-ai-lab-token-validation-context)
        (only-in :tao3k/agentic-ai-lab/modules/governance/policy/strict-mediation
                 agentic-ai-lab-strict-mediation-state
                 agentic-ai-lab-strict-mediate
                 agentic-ai-lab-strict-mediation-result-state
                 agentic-ai-lab-strict-mediation-result-receipt))

(export
  marlin-cove-assessment-ready?
  marlin-cove-bridge-mention-rule
  marlin-cove-invite-profile
  marlin-cove-tier0-gate-profile
  marlin-cove-room-turn-profile
  marlin-cove-assess-invite
  marlin-cove-assess-tier0-gate
  marlin-cove-assess-room-turn
  marlin-cove-mediate-tier0-gate-once)

(def (marlin-cove-assessment-ready? assessment)
  (and (.ref assessment 'handoff-ready?)
       (not (.ref assessment 'runtime-executed?))))

;;; Cove-specific bridge mention rule: Telegram / Discord guild turns must
;;; mention the bot unless configured optional or arriving via DM.
(def marlin-cove-bridge-mention-rule
  (agentic-ai-lab-governance-guard-when
   "marlin-cove/rule/external-bridge-mention"
   (lambda (ctx)
     (and (memq (agentic-ai-lab-governance-context-ref ctx 'origin default: 'cove-native)
                '(telegram discord))
          #t))
   (agentic-ai-lab-governance-rule
    "marlin-cove/rule/require-mention-in-group"
    (lambda (ctx)
      (let ((require-mention?
             (agentic-ai-lab-governance-context-ref ctx 'require-mention default: #t))
            (mentioned?
             (agentic-ai-lab-governance-context-ref ctx 'bot-mentioned default: #f))
            (is-dm?
             (agentic-ai-lab-governance-context-ref ctx 'is-dm default: #f)))
        (if (or (not require-mention?) is-dm? mentioned?)
          (agentic-ai-lab-governance-rule-allow
           "marlin-cove/rule/require-mention-in-group"
           'bridge-turn-admitted
           "external bridge turn satisfies mention policy")
          (agentic-ai-lab-governance-rule-deny
           "marlin-cove/rule/require-mention-in-group"
           'bridge-turn-missing-mention
           "external bridge guild message did not mention configured bot")))))))

;;; Domain Profile 1: Scoped Room Invite Governance (inherits AgenticAILabGovernanceProfile.)
(def (marlin-cove-invite-profile default-guest-scope: (guest-scope "public"))
  (let* ((human-issuer
          (agentic-ai-lab-governance-require-principal-kind
           "marlin-cove/rule/invite-human-issuer" 'human))
         (authorized-role
          (agentic-ai-lab-governance-require-actor-roles
           "marlin-cove/rule/invite-role" '(owner maintainer)))
         (guest-scope-ceiling
          (agentic-ai-lab-governance-guard-when
           "marlin-cove/rule/invite-guest-scope-guard"
           (lambda (ctx)
             (eq? (agentic-ai-lab-governance-context-ref ctx 'grant-role) 'external-guest))
           (agentic-ai-lab-governance-require-scope-subset
            "marlin-cove/rule/invite-guest-scope-subset"
            'grant-scopes
            (list guest-scope))))
         (model
          (agentic-ai-lab-governance-rule-threat-model
           "marlin-cove/threat-model/invite"
           (list human-issuer authorized-role guest-scope-ceiling))))
    (agentic-ai-lab-governance-profile-extend
     AgenticAILabGovernanceProfile.
     identity: "marlin-cove/profile/invite"
     revision: "0.2.0"
     owner: "marlin-cove"
     policies: (.o default-guest-scope: guest-scope)
     threat-model: model)))

;;; Domain Profile 2: Human-Only Tier-0 PromptGate Approval Governance
(def marlin-cove-tier0-gate-profile
  (let* ((human-only
          (agentic-ai-lab-governance-require-principal-kind
           "marlin-cove/rule/tier0-human-only" 'human severity: 'critical))
         (authorized-approver
          (agentic-ai-lab-governance-require-actor-roles
           "marlin-cove/rule/tier0-approver-role" '(owner maintainer) severity: 'critical))
         (model
          (agentic-ai-lab-governance-rule-threat-model
           "marlin-cove/threat-model/tier0-gate"
           (list (agentic-ai-lab-governance-compose-all
                  "marlin-cove/rule/tier0-gate-resolution"
                  (list human-only authorized-approver)
                  severity: 'critical)))))
    (agentic-ai-lab-governance-profile-extend
     AgenticAILabGovernanceProfile.
     identity: "marlin-cove/profile/tier0-gate"
     revision: "0.2.0"
     owner: "marlin-cove"
     threat-model: model)))

;;; Domain Profile 3: Room Turn Admission & Anti-Loop Governance
(def (marlin-cove-room-turn-profile max-bot-chain-depth: (max-depth 2)
                                    default-guest-scope: (guest-scope "public"))
  (let* ((loop-guard
          (agentic-ai-lab-governance-require-chain-depth-within
           "marlin-cove/rule/bot-chain-depth" 'bot-chain-depth max-depth))
         (guest-scope-check
          (agentic-ai-lab-governance-guard-when
           "marlin-cove/rule/guest-turn-scope"
           (lambda (ctx)
             (eq? (agentic-ai-lab-governance-context-ref ctx 'actor-role) 'external-guest))
           (agentic-ai-lab-governance-require-scope-subset
            "marlin-cove/rule/guest-turn-scope-subset"
            'granted-scopes
            (list guest-scope))))
         (model
          (agentic-ai-lab-governance-rule-threat-model
           "marlin-cove/threat-model/room-turn"
           (list loop-guard marlin-cove-bridge-mention-rule guest-scope-check))))
    (agentic-ai-lab-governance-profile-extend
     AgenticAILabGovernanceProfile.
     identity: "marlin-cove/profile/room-turn"
     revision: "0.2.0"
     owner: "marlin-cove"
     policies: (.o max-bot-chain-depth: max-depth
                   default-guest-scope: guest-scope)
     threat-model: model)))

;;; Evaluate each Cove governance plane through `agentic-ai-lab`'s open protocol.
(def (marlin-cove-assess-invite context default-guest-scope: (guest-scope "public"))
  (agentic-ai-lab-governance-evaluate
   (marlin-cove-invite-profile default-guest-scope: guest-scope)
   context))

(def (marlin-cove-assess-tier0-gate context)
  (agentic-ai-lab-governance-evaluate marlin-cove-tier0-gate-profile context))

(def (marlin-cove-assess-room-turn context
                                   max-bot-chain-depth: (max-depth 2)
                                   default-guest-scope: (guest-scope "public"))
  (agentic-ai-lab-governance-evaluate
   (marlin-cove-room-turn-profile
    max-bot-chain-depth: max-depth
    default-guest-scope: guest-scope)
   context))

;;; Couple Tier-0 Gate Assessment with `agentic-ai-lab`'s One-Shot Strict
;;; Mediation (`AuthorizedEffectToken`) so an approved gate executes at most once.
(def (marlin-cove-mediate-tier0-gate-once
      gate-context mediation-state token validation-context expected-root observation)
  (let (assessment-value (marlin-cove-assess-tier0-gate gate-context))
    (if (not (marlin-cove-assessment-ready? assessment-value))
      (.o kind: 'marlin-cove.gate-mediation-denied
          assessment: assessment-value
          accepted?: #f
          code: 'tier0-governance-assessment-blocked
          state: mediation-state)
      (let* ((result
              (agentic-ai-lab-strict-mediate
               mediation-state token validation-context expected-root observation))
             (receipt-value (agentic-ai-lab-strict-mediation-result-receipt result))
             (next-state (agentic-ai-lab-strict-mediation-result-state result)))
        (.o kind: 'marlin-cove.gate-mediation-result
            assessment: assessment-value
            accepted?: (.ref receipt-value 'accepted?)
            code: (.ref receipt-value 'code)
            receipt: receipt-value
            state: next-state)))))
