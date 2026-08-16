#!/usr/bin/env bash
# run-train-diff-dynamic-off.sh — W10 DYNAMIC_ADJUST off-behaviour check.
#
# Runs one round of the W6-T7 training differential with DYNAMIC_ADJUST
# clear, then diffs the exported (phrase, pinyin, count) rows exactly. The
# pin's `pinyin_train` has no DYNAMIC_ADJUST gate, so the expected off
# behaviour is that both engines still write; this script proves they write
# identically.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building train-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o train-diff train-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"
if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 0
fi

OPTIONS="${TRAINDIFF_OFF_OPTIONS:-0x00000188}"
echo "options: $OPTIONS"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"

if ! TRAINDIFF_ROUNDS=1 TRAINDIFF_OPTIONS="$OPTIONS" \
    ./train-diff "$CAPI_SO" "$REPO_ROOT/fixtures/w3" > "$CAPI_LOG" 2>/dev/null; then
    echo "FAIL: train-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
if ! TRAINDIFF_ROUNDS=1 TRAINDIFF_OPTIONS="$OPTIONS" \
    ./train-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2>/dev/null; then
    echo "FAIL: train-diff crashed against the oracle"
    cat "$ORACLE_LOG"
    exit 1
fi

if ! diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") > /dev/null; then
    echo "FAIL: DYNAMIC_ADJUST-off training exports differ"
    diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 2
fi

rows="$(grep -cE '^(bigram|phrase):' "$CAPI_LOG" || true)"
echo "DYNAMIC_ADJUST off: exports identical; wrote $rows rows (pin writes, no no-op)"
rm -f "$CAPI_LOG" "$ORACLE_LOG"
echo "train-diff dynamic-off: PASS"
