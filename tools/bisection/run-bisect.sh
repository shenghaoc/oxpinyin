#!/usr/bin/env bash
# run-bisect.sh — build the bisection harness and run it.
#
# Usage:
#   ./run-bisect.sh                       # capi-only (no oracle)
#   ./run-bisect.sh /path/to/libpinyin.so /path/to/data  # differential
#
# Modes:
#   1. capi-only    — build + run the dlopen harness against pinyin-capi
#   2. valgrind     — re-run the harness under valgrind (if available)
#   3. ld-preload   — test ibus-engine-libpinyin with LD_PRELOAD (if available)
#   4. differential — compare capi output against an oracle .so (if args given)
#
# Exits 0 on success, 1 on build/run failure, 2 on differential mismatch,
# 3 on valgrind errors, 4 on LD_PRELOAD integration failure.

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

# ── Mode 1: Run against pinyin-capi ─────────────────────────────────────

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

# ── Mode 2: Valgrind ────────────────────────────────────────────────────

if command -v valgrind > /dev/null 2>&1; then
    echo "--- valgrind ---"
    VALGRIND_LOG="$(mktemp)"
    if ! valgrind --error-exitcode=1 \
                  --leak-check=full \
                  --errors-for-leak-kinds=definite \
                  ./bisect "$CAPI_SO" "$CAPI_DATA" > /dev/null 2> "$VALGRIND_LOG"; then
        echo "FAIL: valgrind detected errors"
        cat "$VALGRIND_LOG"
        rm -f "$CAPI_LOG" "$VALGRIND_LOG"
        exit 3
    fi
    # Extract the summary line.
    grep -E "ERROR SUMMARY:" "$VALGRIND_LOG" || true
    grep -E "definitely lost:" "$VALGRIND_LOG" || true
    rm -f "$VALGRIND_LOG"
    echo "valgrind: ok"
    echo ""
else
    echo "--- valgrind ---"
    echo "SKIP: valgrind not found"
    echo ""
fi

# ── Mode 3: LD_PRELOAD integration ──────────────────────────────────────

IBUS_ENGINE=""
for candidate in /usr/libexec/ibus-engine-libpinyin /usr/lib/ibus/ibus-engine-libpinyin; do
    if [ -x "$candidate" ]; then
        IBUS_ENGINE="$candidate"
        break
    fi
done

if [ -n "$IBUS_ENGINE" ]; then
    echo "--- ld-preload integration ---"
    echo "engine: $IBUS_ENGINE"

    # Verify our symbols override the system libpinyin via LD_DEBUG.
    LDDEBUG_LOG="$(mktemp)"
    env LD_PRELOAD="$CAPI_SO" LD_DEBUG=bindings \
        "$IBUS_ENGINE" --ibus > /dev/null 2> "$LDDEBUG_LOG" &
    ENGINE_PID=$!
    sleep 2
    kill "$ENGINE_PID" 2>/dev/null || true
    wait "$ENGINE_PID" 2>/dev/null || true

    # Count how many pinyin_ symbols were bound from our .so.
    BOUND=$(grep -c "to $CAPI_SO.*pinyin_" "$LDDEBUG_LOG" 2>/dev/null || echo 0)
    echo "symbols bound from capi: $BOUND"

    if [ "$BOUND" -eq 0 ]; then
        echo "FAIL: no pinyin_ symbols resolved from $CAPI_SO"
        cat "$LDDEBUG_LOG"
        rm -f "$CAPI_LOG" "$LDDEBUG_LOG"
        exit 4
    fi

    # Verify key symbols were bound from our .so, not the system one.
    MISSING_CRITICAL=0
    for sym in pinyin_init pinyin_fini pinyin_alloc_instance pinyin_set_options; do
        if ! grep -q "to $CAPI_SO.*\`$sym'" "$LDDEBUG_LOG" 2>/dev/null; then
            echo "  MISSING: $sym not bound from capi"
            MISSING_CRITICAL=1
        fi
    done

    if [ "$MISSING_CRITICAL" -ne 0 ]; then
        echo "FAIL: critical symbols not overridden"
        rm -f "$CAPI_LOG" "$LDDEBUG_LOG"
        exit 4
    fi

    rm -f "$LDDEBUG_LOG"
    echo "ld-preload: ok (engine loaded, $BOUND symbols overridden)"
    echo ""
else
    echo "--- ld-preload integration ---"
    echo "SKIP: ibus-engine-libpinyin not found"
    echo ""
fi

# ── Mode 4: Differential (if oracle provided) ───────────────────────────

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
