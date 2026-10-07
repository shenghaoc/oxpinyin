#!/usr/bin/env bash
# run-w11-diff.sh — shared pin-gated runner for W11 unique differentials.
#
# Usage: run-w11-diff.sh <driver-stem> [prebuilt-library]
#   driver-stem is user-candidate-diff | addon-candidate-diff | predict-diff
# Env: PINYIN_ORACLE_PREFIX (default $HOME/.local/opt/pinyin-oracle)
#      CAPI_W11_SYSTEM_DIR, or OXPINYIN_SYSTEM_DIR -- the oxpinyin system
#      data dir. Required once an oracle is present; see system-dir.sh.
#      (The variable is CAPI_W11_SYSTEM_DIR, with the _DIR suffix. It was
#      undocumented here before, and CAPI_W11_SYSTEM -- the name a reader
#      would guess -- silently did nothing.)

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
STEM="${1:?driver stem}"
OXPINYIN_CAPI_SO=${2:-${OXPINYIN_CAPI_SO:-}}
# shellcheck source=oracle-cell.sh
source ./oracle-cell.sh
case $STEM in
    *[!a-z0-9-]*|'') echo "FAIL: invalid driver stem: $STEM" >&2; exit 1 ;;
esac
PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 77
fi
if ! grep -Fxq "$EXPECTED_PIN_REF" "$PREFIX/oracle-pin.txt"; then
    echo "FAIL: oracle prefix at $PREFIX is off-pin for the $ORACLE_DBM cell"
    echo "  expected $EXPECTED_PIN_REF"
    exit 1
fi
echo "oracle: $ORACLE_SO"


WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
DRIVER=$WORK/$STEM

echo "--- building ${STEM} driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$DRIVER" "${STEM}.c" -ldl
echo "build: ok"

oracle_cell_artifact OXPINYIN_CAPI_SO libpinyin_capi.so oxpinyin-capi
CAPI_SO=$OXPINYIN_CAPI_SO
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi


# shellcheck source=tools/bisection/system-dir.sh
. ./system-dir.sh
CAPI_SYS="$(resolve_system_dir CAPI_W11_SYSTEM_DIR "w11-diff/${STEM}")"
CAPI_LOG="$WORK/capi.log"
ORACLE_LOG="$WORK/oracle.log"
if ! "$DRIVER" "$CAPI_SO" "$CAPI_SYS" > "$CAPI_LOG" 2> /dev/null; then
    echo "FAIL: $STEM crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi
if ! "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> /dev/null; then
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
