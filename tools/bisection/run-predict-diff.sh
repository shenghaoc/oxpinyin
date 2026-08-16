#!/usr/bin/env bash
# run-predict-diff.sh — W11 unique phrase-prediction differential.
# Non-punctuation mode (W11-PUNCT: punct.redb is not public-ABI consumable).
# Does not edit run-import-diff.sh or run-train-diff.sh.
set -euo pipefail
cd "$(dirname "$0")"
exec ./run-w11-diff.sh predict-diff
