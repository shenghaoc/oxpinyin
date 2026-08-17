#!/usr/bin/env bash
# run-punct-diff.sh — punctuation-prediction differential (#104).
# Compares PREDICTED_PUNCTUATION lists for prefixes present in both the
# W3 mini phrase index and the pin's full tables.
# Does not edit run-import-diff.sh, run-train-diff.sh, or the non-punctuation
# prediction scripts (those still skip type 8: mini vs full prefix tokens).
set -euo pipefail
cd "$(dirname "$0")"
exec ./run-w11-diff.sh punct-diff
