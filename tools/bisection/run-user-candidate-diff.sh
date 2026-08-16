#!/usr/bin/env bash
# run-user-candidate-diff.sh — W11 unique user-phrase surface differential.
# Does not edit run-import-diff.sh or run-train-diff.sh.
set -euo pipefail
cd "$(dirname "$0")"
exec ./run-w11-diff.sh user-candidate-diff
