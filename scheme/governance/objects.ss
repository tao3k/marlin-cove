;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; POO declarations grouping Marlin Cove governance operations backed by
;;; `agentic-ai-lab`'s native Governance library.

(import (only-in :clan/poo/object .o)
        (only-in :tao3k/agentic-ai-lab/modules/governance/interface
                 AgenticAILabGovernanceProfile.
                 agentic-ai-lab-governance-evaluate
                 agentic-ai-lab-governance-compose-all
                 agentic-ai-lab-governance-compose-any
                 agentic-ai-lab-governance-guard-when)
        (only-in "./funcs.ss"
                 marlin-cove-assessment-ready?
                 marlin-cove-invite-profile
                 marlin-cove-tier0-gate-profile
                 marlin-cove-room-turn-profile
                 marlin-cove-assess-invite
                 marlin-cove-assess-tier0-gate
                 marlin-cove-assess-room-turn
                 marlin-cove-mediate-tier0-gate-once))

(export CoveGovernanceOperations)

(def CoveGovernanceOperations
  (.o base-profile: AgenticAILabGovernanceProfile.
      evaluate: agentic-ai-lab-governance-evaluate
      compose-all: agentic-ai-lab-governance-compose-all
      compose-any: agentic-ai-lab-governance-compose-any
      guard-when: agentic-ai-lab-governance-guard-when
      assessment-ready?: marlin-cove-assessment-ready?
      invite-profile: marlin-cove-invite-profile
      tier0-gate-profile: marlin-cove-tier0-gate-profile
      room-turn-profile: marlin-cove-room-turn-profile
      assess-invite: marlin-cove-assess-invite
      assess-tier0-gate: marlin-cove-assess-tier0-gate
      assess-room-turn: marlin-cove-assess-room-turn
      mediate-tier0-gate-once: marlin-cove-mediate-tier0-gate-once))
