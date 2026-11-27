;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; Select the Gerbil Scheme governance modules owned by Marlin Cove.

(import (only-in :tao3k/yuanxu/prelude yuanxu!))

(yuanxu!
  :load-modules "."
  mode: explicit
  mount: MarlinCove
  :test-dir "./t"
  (governance))
