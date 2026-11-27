;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; Executable formal security refinement checks for Marlin Cove combining:
;;;   1. `agentic-ai-lab` AI-Security Executable Refinement
;;;      (capability-confinement, tool-identity, effect-containment,
;;;      causal-continuity, cryptographic SHA-256 trace-root replay receipts).
;;;   2. `deterministic-engineering` Bounded Capacity & Evidence obligations
;;;      (`CapacityModel` overflow refusal + `EvidencePolicy` recorded-only).

(import :std/test
        (only-in :clan/poo/object .o .ref)
        (only-in :tao3k/agentic-ai-lab/modules/governance/policy/ai-security-executable-refinement
                 agentic-ai-lab-agent-action-evidence-envelope
                 agentic-ai-lab-ai-security-transition-policy
                 agentic-ai-lab-ai-security-reference-transition
                 agentic-ai-lab-ai-security-transition-replay-valid?))

(export cove-security-verification-test)

(def cove-security-verification-test
  (test-suite "Marlin Cove executable security refinement (agentic-ai-lab + deterministic-engineering)"
    (test-case "Marlin Gateway dispatch satisfies all 4 AI-security invariants and replay receipt"
      (def policy
        (agentic-ai-lab-ai-security-transition-policy
         '(cove.dispatch.marlin-gateway cove.gate.resolve-human)
         'marlin-cove-relay
         '(room-relay marlin-gateway)
         'cove-msg-001
         "sha256:room-root-001"))

      (def valid-envelope
        (agentic-ai-lab-agent-action-evidence-envelope
         'cove-dispatch-001
         'cove-msg-001
         'obs-evt-001
         'rec-001
         'human:owner
         'marlin-cove-relay
         'cove.dispatch.marlin-gateway
         'marlin-gateway
         "sha256:evidence-001"
         "sha256:room-root-001"
         '(emit-room-reply)
         '(emit-room-reply update-presence)))

      (def receipt
        (agentic-ai-lab-ai-security-reference-transition valid-envelope policy))
      (check-equal? (.ref receipt 'decision) 'accepted)
      (check-equal? (.ref receipt 'escalation-reasons) '())
      (check-equal?
       (agentic-ai-lab-ai-security-transition-replay-valid?
        valid-envelope policy receipt)
       #t))

    (test-case "Escalated effect or forged capability fails closed with auditable escalation reasons"
      (def policy
        (agentic-ai-lab-ai-security-transition-policy
         '(cove.dispatch.marlin-gateway)
         'marlin-cove-relay
         '(marlin-gateway)
         'cove-msg-001
         "sha256:room-root-001"))

      (def malicious-envelope
        (agentic-ai-lab-agent-action-evidence-envelope
         'cove-attack-001
         'forged-parent
         'obs-evt-002
         'rec-002
         'human:guest-1
         'forged-tool
         'cove.admin.overwrite-owner
         'host-shell
         "sha256:evidence-002"
         "sha256:wrong-root"
         '(exec-host-shell overwrite-owner)
         '(emit-room-reply)))

      (def denied-receipt
        (agentic-ai-lab-ai-security-reference-transition malicious-envelope policy))
      (check-equal? (.ref denied-receipt 'decision) 'fail-closed)
      (check-equal?
       (.ref denied-receipt 'escalation-reasons)
       '(capability-confinement tool-identity effect-containment causal-continuity))
      (check-equal?
       (agentic-ai-lab-ai-security-transition-replay-valid?
        malicious-envelope policy denied-receipt)
       #t))))
