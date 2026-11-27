;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; Public Yuanxu module interface for Marlin Cove's high-order governance plane.

(import (only-in :clan/poo/object .def .o)
        (only-in "./objects.ss" CoveGovernanceOperations)
        (only-in "./types.ss" CoveGovernanceContract))

(export CoveGovernance)

(.def CoveGovernance
  identity: 'governance
  core: (.o contract: CoveGovernanceContract
            operations: CoveGovernanceOperations))
