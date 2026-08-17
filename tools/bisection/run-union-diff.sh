#!/usr/bin/env bash
# run-union-diff.sh — W11 phrase-index union differential.
#
# Unique name; does not edit run-import-diff.sh or run-train-diff.sh.
# Drives tools/bisection/union-diff.c against oxpinyin-capi and the
# pin-built libpinyin. Four assertions, all exact:
#   1. imported user phrase at the same candidate index;
#   2. ADDON candidates for the same loaded library;
#   3. train-then-predict phrase list (punctuation omitted; see punct-diff).
#   4. user-bigram successors at count 9 and 10 via union-edge-diff.c
#      (pinyin.cpp:2349-2350). Public train first-seeds 69, so 9/10 are planted.
#
# Sort profile is recorded in the driver output. The empty-store parity
# profile never trains; this script uses the live guess/predict masks.
#
# Capi needs the window-scan construction (real unigrams) plus Option A
# addon_4 tables. A temp system dir carries the W3 mini tables, the mini
# art export, and a tiny interpolation2.text.
#
# Env: PINYIN_ORACLE_PREFIX (default $HOME/.local/opt/pinyin-oracle).
# Exit: 0 identical or skipped; 1 build/run failure; 2 divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building union-diff drivers ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o union-diff union-diff.c -ldl
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o union-edge-diff union-edge-diff.c -ldl
echo "--- building plant-oracle-bigram ---"
g++ -std=c++17 -Wall -Wextra -Werror -O2 plant-oracle-bigram.cc \
    -ltkrzw -lpthread -o plant-oracle-bigram
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
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi
echo "oracle: $ORACLE_SO"

CAPI_SYS="$(mktemp -d)"
CAPI_EDGE_USER="$(mktemp -d)"
ORACLE_EDGE_USER="$(mktemp -d)"
trap 'rm -rf "$CAPI_SYS" "$CAPI_EDGE_USER" "$ORACLE_EDGE_USER"' EXIT
cp "$REPO_ROOT/fixtures/w3/pinyin_index.redb" "$CAPI_SYS/"
cp "$REPO_ROOT/fixtures/w3/phrase_index.redb" "$CAPI_SYS/"
cp "$REPO_ROOT/fixtures/w3/bigram.redb" "$CAPI_SYS/"
cp "$REPO_ROOT/fixtures/w3/addon_4_pinyin_index.redb" "$CAPI_SYS/"
cp "$REPO_ROOT/fixtures/w3/addon_4_phrase_index.redb" "$CAPI_SYS/"
printf '%s\n' '\data model interpolation' '\1-gram' '\item 1 ok count 1' \
    > "$CAPI_SYS/interpolation2.text"

CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
if ! ./union-diff "$CAPI_SO" "$CAPI_SYS" > "$CAPI_LOG" 2> /dev/null; then
    echo "FAIL: union-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi
if ! ./union-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> /dev/null; then
    echo "FAIL: union-diff crashed against the oracle"
    cat "$ORACLE_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi

echo "--- capi ---"
cat "$CAPI_LOG"
echo "--- oracle ---"
cat "$ORACLE_LOG"

if ! diff -u "$ORACLE_LOG" "$CAPI_LOG" > /dev/null; then
    echo "DIVERGENCE: union-diff logs differ"
    diff -u "$ORACLE_LOG" "$CAPI_LOG" || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 2
fi
rm -f "$CAPI_LOG" "$ORACLE_LOG"
echo "union-diff: IDENTICAL"

# ── 4. filter-edge: planted successors at count 9 (absent) and 10 (kept)
# Public pinyin_train first-seeds 69, so 9/10 are planted into each
# engine's user-bigram store after a shared three-phrase import.

echo "--- filter-edge setup ---"
if ! ./union-edge-diff "$CAPI_SO" "$CAPI_SYS" setup "$CAPI_EDGE_USER"; then
    echo "FAIL: capi edge setup"
    exit 1
fi
if ! ./union-edge-diff "$ORACLE_SO" "$ORACLE_DATA" setup "$ORACLE_EDGE_USER"; then
    echo "FAIL: oracle edge setup"
    exit 1
fi
if ! ./union-edge-diff "$CAPI_SO" "$CAPI_SYS" plant "$CAPI_EDGE_USER"; then
    echo "FAIL: capi edge plant"
    exit 1
fi
# First three USER_DICTIONARY tokens (`pinyin.cpp:592-594`).
if ! ./plant-oracle-bigram "$ORACLE_EDGE_USER" 0x07000001 0x07000002 9 0x07000003 10; then
    echo "FAIL: oracle edge plant"
    exit 1
fi

CAPI_EDGE="$(mktemp)"
ORACLE_EDGE="$(mktemp)"
if ! ./union-edge-diff "$CAPI_SO" "$CAPI_SYS" predict "$CAPI_EDGE_USER" > "$CAPI_EDGE" 2> /dev/null; then
    echo "FAIL: union-edge-diff predict crashed against oxpinyin-capi"
    cat "$CAPI_EDGE"
    rm -f "$CAPI_EDGE" "$ORACLE_EDGE"
    exit 1
fi
if ! ./union-edge-diff "$ORACLE_SO" "$ORACLE_DATA" predict "$ORACLE_EDGE_USER" > "$ORACLE_EDGE" 2> /dev/null; then
    echo "FAIL: union-edge-diff predict crashed against the oracle"
    cat "$ORACLE_EDGE"
    rm -f "$CAPI_EDGE" "$ORACLE_EDGE"
    exit 1
fi

echo "--- capi edge ---"
cat "$CAPI_EDGE"
echo "--- oracle edge ---"
cat "$ORACLE_EDGE"

if ! diff -u "$ORACLE_EDGE" "$CAPI_EDGE" > /dev/null; then
    echo "DIVERGENCE: filter-edge logs differ"
    diff -u "$ORACLE_EDGE" "$CAPI_EDGE" || true
    rm -f "$CAPI_EDGE" "$ORACLE_EDGE"
    exit 2
fi
# Lock the copied constant: 9 must be dropped, 10 must be a bigram hit.
if ! grep -qx 'pred-edge-9: 甲甲 absent' "$CAPI_EDGE"; then
    echo "FAIL: count=9 successor was not filtered"
    rm -f "$CAPI_EDGE" "$ORACLE_EDGE"
    exit 2
fi
if ! grep -qx 'pred-edge-10: 乙乙 type=4' "$CAPI_EDGE"; then
    echo "FAIL: count=10 successor was not predicted"
    rm -f "$CAPI_EDGE" "$ORACLE_EDGE"
    exit 2
fi
rm -f "$CAPI_EDGE" "$ORACLE_EDGE"
echo "union-diff filter-edge: IDENTICAL"
exit 0
