#!/usr/bin/env bash
# run-addon-candidate-diff.sh — W11 unique ADDON-candidate differential.
# Does not edit run-import-diff.sh or run-train-diff.sh.
#
# Capi needs the window-scan construction (real unigrams) plus the Option A
# addon_4 tables. A temp system dir carries the W3 mini tables, the mini
# art export, and a tiny interpolation2.text so fixture init installs
# real unigrams.
set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
SYS="$(mktemp -d)"
trap 'rm -rf "$SYS"' EXIT
cp "$REPO_ROOT/fixtures/w3/pinyin_index.redb" "$SYS/"
cp "$REPO_ROOT/fixtures/w3/phrase_index.redb" "$SYS/"
cp "$REPO_ROOT/fixtures/w3/bigram.redb" "$SYS/"
cp "$REPO_ROOT/fixtures/w3/addon_4_pinyin_index.redb" "$SYS/"
cp "$REPO_ROOT/fixtures/w3/addon_4_phrase_index.redb" "$SYS/"
printf '%s\n' '\data model interpolation' '\1-gram' '\item 1 ok count 1' \
    > "$SYS/interpolation2.text"
export CAPI_W11_SYSTEM_DIR="$SYS"
exec ./run-w11-diff.sh addon-candidate-diff
