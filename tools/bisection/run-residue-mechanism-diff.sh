#!/usr/bin/env bash
# run-residue-mechanism-diff.sh — mechanism probe for residues B and D
# in docs/findings/probe-coverage-abi.md. Diagnosis only: diffs the two
# logs; never asserts a class.
#
# Env-gated on the pin-built oracle (PINYIN_ORACLE_PREFIX). The capi
# system directory resolves through system-dir.sh. For phase D's
# same-dir settlement, set RESIDUE_MECH_SAME_DIR=1 to drive both .so
# files against the oracle's own data/ (P6-native layout).
#
# Exit: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=tools/bisection/system-dir.sh
. ./system-dir.sh
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building residue-mechanism driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o residue-mechanism-diff \
    residue-mechanism-diff.c -ldl
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

if [[ "${RESIDUE_MECH_SAME_DIR:-}" == "1" ]]; then
    SYSTEM="$ORACLE_DATA"
    echo "same-dir mode: both sides on $SYSTEM"
else
    SYSTEM="$(resolve_system_dir RESIDUE_MECH_SYSTEM residue-mechanism)"
fi

OUT_DIR="${RESIDUE_MECH_OUT:-}"
if [[ -z "$OUT_DIR" ]]; then
    OUT_DIR="$(mktemp -d)"
    CLEAN_OUT=1
else
    mkdir -p "$OUT_DIR"
    CLEAN_OUT=0
fi
CAPI_LOG="$OUT_DIR/residue-mech-capi.log"
ORACLE_LOG="$OUT_DIR/residue-mech-oracle.log"

echo "--- capi side ---"
if ! ./residue-mechanism-diff "$CAPI_SO" "$SYSTEM" > "$CAPI_LOG" 2>"$OUT_DIR/capi.err"; then
    echo "FAIL: residue-mechanism-diff crashed against oxpinyin-capi"
    cat "$OUT_DIR/capi.err" || true
    [[ "$CLEAN_OUT" == 1 ]] && rm -rf "$OUT_DIR"
    exit 1
fi
echo "oxpinyin-capi: ok ($(wc -l < "$CAPI_LOG") log lines)"

echo "--- oracle side ---"
if ! ./residue-mechanism-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2>"$OUT_DIR/oracle.err"; then
    echo "FAIL: residue-mechanism-diff crashed against the oracle"
    cat "$OUT_DIR/oracle.err" || true
    [[ "$CLEAN_OUT" == 1 ]] && rm -rf "$OUT_DIR"
    exit 1
fi
echo "oracle: ok ($(wc -l < "$ORACLE_LOG") log lines)"

echo "--- diff ---"
if diff -u "$ORACLE_LOG" "$CAPI_LOG"; then
    echo "IDENTICAL"
    [[ "$CLEAN_OUT" == 1 ]] && rm -rf "$OUT_DIR"
    exit 0
fi
echo "DIVERGENT (expected while residues B/D remain open)"
echo "logs: $ORACLE_LOG  $CAPI_LOG"
exit 2
