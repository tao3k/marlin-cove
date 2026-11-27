;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; POO ObjectContract for Marlin Cove's governance operations built on
;;; `agentic-ai-lab`'s Governance library.

(import (only-in :clan/poo/object .cc .o object?)
        (only-in :clan/poo/type Type.)
        (only-in :clan/poo/types/object-contract ObjectContract.))

(export CoveGovernanceContract)

(def PolicyFn (.cc Type. '.element? procedure?))
(def ProfileObj (.cc Type. '.element? object?))

(def CoveGovernanceContract
  (.cc ObjectContract.
       responsibilities: (.o base-profile: ProfileObj
                             evaluate: PolicyFn
                             compose-all: PolicyFn
                             compose-any: PolicyFn
                             guard-when: PolicyFn
                             assessment-ready?: PolicyFn
                             invite-profile: PolicyFn
                             tier0-gate-profile: ProfileObj
                             room-turn-profile: PolicyFn
                             assess-invite: PolicyFn
                             assess-tier0-gate: PolicyFn
                             assess-room-turn: PolicyFn
                             mediate-tier0-gate-once: PolicyFn)))
