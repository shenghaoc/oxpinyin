#!/usr/bin/env bash
# run-predict-diff.sh — W11 unique phrase-prediction differential.
# Non-punctuation mode: still skips type 8. Mini vs full prefix tokens
# (e.g. 测) would diverge on punctuation even when the trained bigram matches.
# Punctuation is covered by run-punct-diff.sh (#104).
# Does not edit run-import-diff.sh or run-train-diff.sh.
set -euo pipefail
cd "$(dirname "$0")"
exec ./run-w11-diff.sh predict-diff
