#!/usr/bin/env bash
# run-w11-diff.sh — shared pin-gated runner for W11 unique differentials.
#
# Usage: run-w11-diff.sh <driver-stem> <extra-capi-system-dir-setup>
#   driver-stem is user-candidate-diff | addon-candidate-diff | predict-diff
# Env: PINYIN_ORACLE_PREFIX (default $HOME/.local/opt/pinyin-oracle)

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
STEM="${1:?driver stem}"
shift || true

echo "--- building ${STEM} driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$STEM" "${STEM}.c" -ldl
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

CAPI_SYS="${CAPI_W11_SYSTEM_DIR:-$REPO_ROOT/fixtures/w3}"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
if ! ./"$STEM" "$CAPI_SO" "$CAPI_SYS" > "$CAPI_LOG" 2> /dev/null; then
    echo "FAIL: $STEM crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi
if ! ./"$STEM" "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> /dev/null; then
    echo "FAIL: $STEM crashed against the oracle"
    cat "$ORACLE_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi

echo "--- capi ---"
cat "$CAPI_LOG"
echo "--- oracle ---"
cat "$ORACLE_LOG"

if ! diff -u "$ORACLE_LOG" "$CAPI_LOG" > /dev/null; then
    echo "DIVERGENCE: $STEM logs differ"
    diff -u "$ORACLE_LOG" "$CAPI_LOG" || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 2
fi
rm -f "$CAPI_LOG" "$ORACLE_LOG"
echo "${STEM}: IDENTICAL"
exit 0
