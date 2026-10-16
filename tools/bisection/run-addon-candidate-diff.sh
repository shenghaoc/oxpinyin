#!/usr/bin/env bash
# run-addon-candidate-diff.sh — W11 unique ADDON-candidate differential.
# Does not edit run-import-diff.sh or run-train-diff.sh.
#
# Both engines read the same full data: run-w11-diff.sh resolves the capi
# directory through system-dir.sh (CAPI_W11_SYSTEM_DIR overrides).
set -euo pipefail
cd "$(dirname "$0")"
exec ./run-w11-diff.sh addon-candidate-diff "$@"
