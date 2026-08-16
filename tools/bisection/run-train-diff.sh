#!/usr/bin/env bash
# run-train-diff.sh — W6-T7 user-store differential.
#
# Drives the identical scripted C-ABI training sequence
# (tools/bisection/train-diff.c) into libpinyin_capi.so and the pin-built
# libpinyin.so, exports both engines' user data through the §9 iterators,
# and diffs the (phrase, pinyin, count) triple sets with exact-integer
# equality. This is the W6 differential: a divergence means the T1–T5
# arithmetic diverges from the pinned oracle.
#
# The seed sequence is observed by running the driver once per round count
# (1..8) with a single export per context: the pinned oracle's own bigram
# export segfaults (stale pinyin-join buffer) when the export cycle is
# repeated inside one context, so each observation gets a fresh process.
# The per-round bigram row then walks 138, 414, 1242, 3726, 11178, 33534,
# 77694, 121854 (stored count × 2, upstream's rendering) through the clamp.
#
# Env-gated on the pin-built oracle, mirroring the W9 PINYIN_NGSEG /
# PINYIN_GEN_NGRAM gates: PINYIN_ORACLE_PREFIX (default
# $HOME/.local/opt/pinyin-oracle) must hold the pin-verified prefix from
# tools/oracle/build-oracle.sh. Absent → skip with a diagnostic, exit 0.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# ── Build the driver ────────────────────────────────────────────────────

echo "--- building train-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o train-diff train-diff.c -ldl
echo "build: ok"

# ── Build oxpinyin-capi ────────────────────────────────────────────────────

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

# ── Locate the pin-built oracle (env-gated) ──────────────────────────────

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
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
if [ ! -f "$ORACLE_DATA/bigram.db" ]; then
    echo "SKIP: oracle data not found at $ORACLE_DATA"
    exit 0
fi
echo "oracle: $ORACLE_SO"
echo "data:   $ORACLE_DATA"
echo ""

# ── Drive both engines: one fresh process per round count ───────────────

fail=0
for rounds in 1 2 3 4 5 6 7 8; do
    echo "--- rounds=$rounds ---"
    CAPI_LOG="$(mktemp)"
    ORACLE_LOG="$(mktemp)"
    if ! TRAINDIFF_ROUNDS=$rounds ./train-diff "$CAPI_SO" "$REPO_ROOT/fixtures/w3" \
        > "$CAPI_LOG" 2>/dev/null; then
        echo "FAIL: train-diff crashed against oxpinyin-capi (rounds=$rounds)"
        cat "$CAPI_LOG"
        exit 1
    fi
    if ! TRAINDIFF_ROUNDS=$rounds ./train-diff "$ORACLE_SO" "$ORACLE_DATA" \
        > "$ORACLE_LOG" 2>/dev/null; then
        echo "FAIL: train-diff crashed against the oracle (rounds=$rounds)"
        cat "$ORACLE_LOG"
        exit 1
    fi
    if ! diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") > /dev/null; then
        echo "DIVERGENCE at rounds=$rounds: exported triples differ"
        diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") || true
        fail=1
    else
        echo "rounds=$rounds: IDENTICAL"
        sort "$CAPI_LOG" | grep -E '^(bigram|phrase):'
    fi
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    echo ""
done

# ── Mask phases (T6): the frontend's "user" and "all" clears at full
# training, one mask + one export per fresh context. ─────────────────────

for mask in user all; do
    echo "--- mask=$mask (rounds=8) ---"
    CAPI_LOG="$(mktemp)"
    ORACLE_LOG="$(mktemp)"
    if ! TRAINDIFF_ROUNDS=8 TRAINDIFF_MASK=$mask ./train-diff "$CAPI_SO" \
        "$REPO_ROOT/fixtures/w3" > "$CAPI_LOG" 2>/dev/null; then
        echo "FAIL: train-diff crashed against oxpinyin-capi (mask=$mask)"
        cat "$CAPI_LOG"
        exit 1
    fi
    if ! TRAINDIFF_ROUNDS=8 TRAINDIFF_MASK=$mask ./train-diff "$ORACLE_SO" "$ORACLE_DATA" \
        > "$ORACLE_LOG" 2>/dev/null; then
        echo "FAIL: train-diff crashed against the oracle (mask=$mask)"
        cat "$ORACLE_LOG"
        exit 1
    fi
    if ! diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") > /dev/null; then
        echo "DIVERGENCE at mask=$mask: exported triples differ"
        diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") || true
        fail=1
    else
        echo "mask=$mask: IDENTICAL"
        sort "$CAPI_LOG" | grep -E '^(bigram|phrase):' || echo "(empty export)"
    fi
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    echo ""
done

if [ "$fail" -ne 0 ]; then
    exit 2
fi
echo "train-diff: PASS — all round counts and mask phases identical"
