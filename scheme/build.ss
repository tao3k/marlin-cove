#!/usr/bin/env gxi
;;; SPDX-FileCopyrightText: 2026 tao3k team and Contributors
;;;
;;; SPDX-License-Identifier: Apache-2.0 AND LGPL-2.1-or-later

;;; Minimal Yuanxu build script for Marlin Cove's Scheme governance plane.

(import (only-in :clan/poo/object .get)
        (only-in :std/build-script defbuild-script)
        (only-in "./init.ss" marlin-cove.init.exports))

(defbuild-script (.get marlin-cove.init.exports std-make-spec))
