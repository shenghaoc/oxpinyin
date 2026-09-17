#!/usr/bin/env bash
# run-abi-probe-diff.sh — the whole-surface probe over the full exported ABI.
#
# The §(e) rule (docs/findings/compatibility-policy.md) wants, for every
# exported symbol, a probe that asserts its whole observable
# surface: return status, out-params and the data they point to, written
# lengths, and handle state. abi-probe-diff.c walks all 79 exported symbols
# through one deterministic sequence and prints one label=value row per
# observation; this runner drives the same binary into oxpinyin-capi and
# the pin-built oracle and diffs the two logs. The driver refuses to run
# at all when any of the 79 is missing from a library, so a completed run
# also proves the export surface is present on both sides.
#
# Env-gated on the pin-built oracle exactly like the other differentials:
# PINYIN_ORACLE_PREFIX (default $HOME/.local/opt/pinyin-oracle).
#
# ABI_PROBE_SORT is the sort_option_t word both sides guess with
# (hex, default 1e — the parity word every other differential passes).
# 1c and 14 are ibus-libpinyin's presets, 0 the raw word; see the
# driver header for why the word matters.
#
# The capi system directory resolves through system-dir.sh:
# ABI_PROBE_SYSTEM first, then OXPINYIN_SYSTEM_DIR, then the
# conventional build locations; an unresolvable directory with a present
# oracle is FATAL, never a silent mini-fixture run. interpolation2.text is
# NOT required here: since P6 the runtime reads its unigrams from the
# chunk files' item fields, and this probe drives the runtime's own
# readers on both sides' own data.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure;
# 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=tools/bisection/system-dir.sh
. ./system-dir.sh
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building abi-probe driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o abi-probe-diff abi-probe-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi
if [[ ! -f "$ORACLE_DATA/bigram.db" ]]; then
    echo "SKIP: oracle data not found at $ORACLE_DATA"
    exit 0
fi

SYSTEM="$(resolve_system_dir ABI_PROBE_SYSTEM abi-probe)"

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
CAPI_ERR="$(mktemp)"
SORT_WORD="${ABI_PROBE_SORT:-1e}"
echo "sort word: 0x$SORT_WORD"
if ! ./abi-probe-diff "$CAPI_SO" "$SYSTEM" "$SORT_WORD" > "$CAPI_LOG" 2> "$CAPI_ERR"; then
    echo "FAIL: abi-probe-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$CAPI_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR"
    exit 1
fi
echo "oxpinyin-capi: ok ($(wc -l < "$CAPI_LOG") log lines)"

echo "--- oracle side ---"
ORACLE_LOG="$(mktemp)"
ORACLE_ERR="$(mktemp)"
if ! ./abi-probe-diff "$ORACLE_SO" "$ORACLE_DATA" "$SORT_WORD" > "$ORACLE_LOG" 2> "$ORACLE_ERR"; then
    echo "FAIL: abi-probe-diff crashed against the oracle"
    cat "$ORACLE_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$ORACLE_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 1
fi
echo "oracle: ok ($(wc -l < "$ORACLE_LOG") log lines)"

echo "--- differential (whole-surface rows for all 79 exported symbols) ---"
if diff -u "$ORACLE_LOG" "$CAPI_LOG" > /dev/null; then
    echo "abi-probe-diff: IDENTICAL"
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 0
fi
echo "DIVERGENCE"
diff -u "$ORACLE_LOG" "$CAPI_LOG" || true
rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
exit 2
