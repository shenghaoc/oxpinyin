#!/usr/bin/env bash
# run-bisect.sh — build the bisection harness and run it.
#
# Usage:
#   ./run-bisect.sh                       # capi-only (no oracle)
#   ./run-bisect.sh /path/to/libpinyin.so /path/to/data  # differential
#
# Exits 0 on success, 1 on build/run failure, 2 on differential mismatch.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# ── Build the harness ────────────────────────────────────────────────────

echo "--- building bisect harness ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o bisect bisect.c -ldl
echo "build: ok"

# ── Build pinyin-capi ────────────────────────────────────────────────────

echo "--- building pinyin-capi ---"
cargo build -p pinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

# ── Locate system data (redb tables for pinyin-capi) ─────────────────────

CAPI_DATA="$REPO_ROOT/fixtures/w3"
if [ ! -f "$CAPI_DATA/pinyin_index.redb" ]; then
    echo "fatal: redb tables not found at $CAPI_DATA"
    exit 1
fi
echo "data: $CAPI_DATA"
echo ""

# ── Run against pinyin-capi ──────────────────────────────────────────────

echo "--- running against pinyin-capi ---"
CAPI_LOG="$(mktemp)"
if ! ./bisect "$CAPI_SO" "$CAPI_DATA" > "$CAPI_LOG" 2>&1; then
    echo "FAIL: bisect crashed against pinyin-capi"
    cat "$CAPI_LOG"
    rm -f "$CAPI_LOG"
    exit 1
fi
echo "pinyin-capi: ok"
cat "$CAPI_LOG"
echo ""

# ── Differential (if oracle provided) ────────────────────────────────────

ORACLE_SO="${1:-}"
ORACLE_DATA="${2:-}"

if [ -n "$ORACLE_SO" ] && [ -n "$ORACLE_DATA" ]; then
    echo "--- running against oracle ---"
    ORACLE_LOG="$(mktemp)"
    if ! ./bisect "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2>&1; then
        echo "FAIL: bisect crashed against oracle"
        cat "$ORACLE_LOG"
        rm -f "$CAPI_LOG" "$ORACLE_LOG"
        exit 1
    fi
    echo "oracle: ok"
    echo ""

    echo "--- differential ---"
    # Strip header lines (so path, dirs) for comparison.
    tail -n +6 "$CAPI_LOG"  > "${CAPI_LOG}.body"
    tail -n +6 "$ORACLE_LOG" > "${ORACLE_LOG}.body"

    if diff -u "${ORACLE_LOG}.body" "${CAPI_LOG}.body" > /dev/null 2>&1; then
        echo "IDENTICAL: no ABI divergence detected"
    else
        echo "DIVERGENCE: outputs differ"
        diff -u "${ORACLE_LOG}.body" "${CAPI_LOG}.body" || true
        rm -f "$CAPI_LOG" "$ORACLE_LOG" "${CAPI_LOG}.body" "${ORACLE_LOG}.body"
        exit 2
    fi
    rm -f "$ORACLE_LOG" "${ORACLE_LOG}.body"
fi

rm -f "$CAPI_LOG" "${CAPI_LOG}.body"
echo ""
echo "bisection: PASS"
