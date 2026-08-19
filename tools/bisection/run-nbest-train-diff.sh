#!/usr/bin/env bash
# run-nbest-train-diff.sh — W14 selection-record differential.
#
# Drives the scripted C-ABI train-then-guess sequence
# (tools/bisection/nbest-train-diff.c) into libpinyin_capi.so and the
# pin-built libpinyin.so and diffs the USER-STORE EXPORT lines: after the
# same (你 → 浩) × N + (你 → 好) × N training, both engines must hold the
# same (phrase, pinyin, count) triples. The 你→浩 rounds choose the pair's
# n-best ROW from round 2 on (NBEST-wins dedup shadows its NORMAL
# candidate), so a wrong selection record piles seeds onto 你→好 instead of
# doubling 你→浩 — the divergence this runner guards
# (docs/findings/sentence-surface.md §8).
#
# The FULL logs are compared — probe blocks and export lines. The probes
# print the C-ABI sentence surface, whose n-best step costs merge the §5
# user overlay (sentence-surface.md §9), so trained user grams must move
# our rows exactly like the oracle's. The export triples alone are not
# enough: with the merge dropped they stay identical (training still
# lands the same user state through the NORMAL candidate) while the
# probe surface stops flipping toward 你浩 — the vacuous green the
# full-log comparison exists to catch.
#
# Env-gated on the pin-built oracle like the train diff
# (PINYIN_ORACLE_PREFIX, default $HOME/.local/opt/pinyin-oracle) and on a
# real-unigram capi system dir (NBEST_CAPI_SYSTEM, e.g. the matched model20
# tables with pinyin_index.redb/phrase_index.redb/bigram.redb and
# interpolation2.text). Rounds: NBESTTRAINDIFF_USER_ROUNDS (default 3),
# NBESTTRAINDIFF_ROUNDS (default 3).
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure;
# 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building nbest-train-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o nbest-train-diff nbest-train-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

# Prerequisite, not a warning: the 你→浩 row-choose only exists while the
# C-ABI sentence surface emits rows. Zero NBEST rows on either side means
# the run compared only NORMAL-candidate training and never exercised the
# shifted-row selection record — that is a failed prerequisite, not a
# (vacuously) identical result.
probe_nbest_rows() { grep -c 'cand\[0\]=NBEST' "$1" || true; }

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    echo "  expected libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c"
    exit 0
fi
if [[ ! -f "$ORACLE_DATA/bigram.db" ]]; then
    echo "SKIP: oracle data not found at $ORACLE_DATA"
    exit 0
fi

CAPI_SYSTEM="${NBEST_CAPI_SYSTEM:-}"
if [[ -z "$CAPI_SYSTEM" || ! -f "$CAPI_SYSTEM/interpolation2.text" ]]; then
    echo "SKIP: NBEST_CAPI_SYSTEM must name a real-unigram system dir"
    echo "  (pinyin_index.redb, phrase_index.redb, bigram.redb, interpolation2.text)"
    exit 0
fi

USER_ROUNDS="${NBESTTRAINDIFF_USER_ROUNDS:-3}"
AUG_ROUNDS="${NBESTTRAINDIFF_ROUNDS:-3}"

echo "--- capi side (user=$USER_ROUNDS augmented=$AUG_ROUNDS) ---"
CAPI_LOG="$(mktemp)"
if ! NBESTTRAINDIFF_USER_ROUNDS="$USER_ROUNDS" NBESTTRAINDIFF_ROUNDS="$AUG_ROUNDS" \
    ./nbest-train-diff "$CAPI_SO" "$CAPI_SYSTEM" > "$CAPI_LOG" 2>/dev/null; then
    echo "FAIL: nbest-train-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    rm -f "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
ORACLE_LOG="$(mktemp)"
if ! NBESTTRAINDIFF_USER_ROUNDS="$USER_ROUNDS" NBESTTRAINDIFF_ROUNDS="$AUG_ROUNDS" \
    ./nbest-train-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2>/dev/null; then
    echo "FAIL: nbest-train-diff crashed against oracle"
    cat "$ORACLE_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: the NBEST row path is active on both sides ---"
CAPI_ROWS="$(probe_nbest_rows "$CAPI_LOG")"
ORACLE_ROWS="$(probe_nbest_rows "$ORACLE_LOG")"
if [[ "$CAPI_ROWS" -eq 0 || "$ORACLE_ROWS" -eq 0 ]]; then
    echo "FAIL: NBEST row path inactive (capi rows: $CAPI_ROWS, oracle rows: $ORACLE_ROWS)"
    echo "  Zero NBEST rows means every training round chose a NORMAL candidate;"
    echo "  the differential then proves nothing about the shifted-row selection"
    echo "  record (docs/findings/sentence-surface.md §8)."
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi
echo "NBEST rows active: capi=$CAPI_ROWS oracle=$ORACLE_ROWS"

echo "--- differential (full log: probe surface + user-store export) ---"
if diff -u "$ORACLE_LOG" "$CAPI_LOG" > /dev/null; then
    echo "nbest-train-diff: IDENTICAL"
    grep -E '^(phrase|bigram):' "$CAPI_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 0
fi
echo "DIVERGENCE (probe surface or user-store export)"
diff -u "$ORACLE_LOG" "$CAPI_LOG" || true
rm -f "$CAPI_LOG" "$ORACLE_LOG"
exit 2
