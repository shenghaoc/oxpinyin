#!/usr/bin/env bash
# run-bisect.sh — build the bisection harness and run it.
#
# Usage:
#   ./run-bisect.sh                       # capi-only (no oracle)
#   ./run-bisect.sh /path/to/libpinyin.so /path/to/data  # differential
#
# Modes:
#   1. capi-only    — build + run the dlopen harness against oxpinyin-capi
#   2. valgrind     — re-run the harness under valgrind (if available)
#   3. ld-preload   — test ibus-engine-libpinyin with LD_PRELOAD (BISECT_LD_PRELOAD=1)
#   4. oracle contract — expected pin abort versus safe C API failure (if args given)
#
# Exits 0 on success, 1 on build/run failure, 2 on differential mismatch,
# 3 on valgrind errors, 4 on LD_PRELOAD integration failure.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
# shellcheck source=tools/bisection/oracle-cell.sh
source ./oracle-cell.sh

# ── Build the harness ────────────────────────────────────────────────────

echo "--- building bisect harness ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o bisect bisect.c -ldl
echo "build: ok"

# ── Build oxpinyin-capi ────────────────────────────────────────────────────

echo "--- building oxpinyin-capi ---"
oracle_cell_artifact OXPINYIN_CAPI_SO libpinyin_capi.so oxpinyin-capi
CAPI_SO=$OXPINYIN_CAPI_SO
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

# ── Locate system data (the committed W3 tables, per backend) ──────────────
#
# The committed W3 fixture holds one per-backend data directory
# (fixtures/w3/<kct|tkt|db>), each a complete system data directory
# the compiled-in backend opens as is. OXPINYIN_CAPI_BACKEND_EXT pins the
# backend for capi builds that select one explicitly (e.g. --features
# kyotocabinet); unset, the plain `cargo build -p oxpinyin-capi` above
# compiles the default (Berkeley DB since 2026-09-20) and db is preferred
# — the same selection run-cpp-smoke.sh makes (this harness runs it right
# below). All three flavours name their tables identically, so a
# wrong-flavour pick would open as nonsense, not as a missing file.

FIX_ROOT="$REPO_ROOT/fixtures/w3"
if [ -n "${OXPINYIN_CAPI_BACKEND_EXT:-}" ]; then
    case "$OXPINYIN_CAPI_BACKEND_EXT" in
        kct|tkt|db) SYS_EXT=$OXPINYIN_CAPI_BACKEND_EXT ;;
        *)
            echo "fatal: OXPINYIN_CAPI_BACKEND_EXT='$OXPINYIN_CAPI_BACKEND_EXT' is not one of: kct tkt db"
            exit 1
            ;;
    esac
else
    SYS_EXT=""
    for ext in db tkt kct; do
        if [ -d "$FIX_ROOT/$ext" ]; then
            SYS_EXT=$ext
            break
        fi
    done
fi
if [ -z "$SYS_EXT" ] || [ ! -d "$FIX_ROOT/$SYS_EXT" ]; then
    echo "fatal: no per-backend fixture directory under $FIX_ROOT"
    exit 1
fi
CAPI_DATA="$FIX_ROOT/$SYS_EXT"
echo "data: $CAPI_DATA"
echo ""

# ── C++ header smoke (the fork consumes pinyin.h from C++ TUs) ───────────

./run-cpp-smoke.sh
echo ""

# ── Mode 1: Run against oxpinyin-capi ─────────────────────────────────────

echo "--- running against oxpinyin-capi ---"
# stderr stays OUT of the compared log: an unbuffered driver diagnostic
# interleaving into buffered stdout corrupts a compared row (the same
# artifact run-pred-order-diff.sh measured turning a 178-row count into
# 177), and a stderr-only line on one side is a spurious diff row.
CAPI_LOG="$(mktemp)"
CAPI_ERR="$(mktemp)"
if ! ./bisect "$CAPI_SO" "$CAPI_DATA" > "$CAPI_LOG" 2> "$CAPI_ERR"; then
    echo "FAIL: bisect crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$CAPI_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR"
    exit 1
fi
echo "oxpinyin-capi: ok"
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
#
# Optional probe, gated behind BISECT_LD_PRELOAD=1.  The mere presence of
# the ibus engine binary is not enough to assert a hard FAIL (exit 4), so
# the probe must be explicitly requested.

IBUS_ENGINE=""
for candidate in /usr/libexec/ibus-engine-libpinyin /usr/lib/ibus/ibus-engine-libpinyin; do
    if [ -x "$candidate" ]; then
        IBUS_ENGINE="$candidate"
        break
    fi
done

if [ "${BISECT_LD_PRELOAD:-0}" = "1" ] && [ -n "$IBUS_ENGINE" ]; then
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
    # grep -c prints 0 (and exits 1) on no match; `|| true` keeps that 0.
    BOUND=$(grep -c "to $CAPI_SO.*pinyin_" "$LDDEBUG_LOG" 2>/dev/null || true)
    BOUND=${BOUND:-0}
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
    if [ "${BISECT_LD_PRELOAD:-0}" != "1" ]; then
        echo "SKIP: not requested (set BISECT_LD_PRELOAD=1 to enable)"
    else
        echo "SKIP: ibus-engine-libpinyin not found"
    fi
    echo ""
fi

# ── Mode 4: Differential (if oracle provided) ───────────────────────────

ORACLE_SO="${1:-}"
ORACLE_DATA="${2:-}"

if [ -n "$ORACLE_SO" ] && [ -n "$ORACLE_DATA" ]; then
    # The pinned oracle cannot finish the broad driver: nihao's right-tail
    # cursor asserts at pinyin.cpp:3092. Check this registered class-(c)
    # observation explicitly on both libraries, with the same full data.
    # The full C API smoke/Valgrind modes above remain separate coverage;
    # this is not a claim of full oracle byte parity.
    echo "--- right-tail assertion contract (nihao, offset 5) ---"
    ORACLE_LOG="$(mktemp)"
    ORACLE_ERR="$(mktemp)"
    CASE_LOG="$(mktemp)"
    oracle_status=0
    ./bisect "$ORACLE_SO" "$ORACLE_DATA" --right-tail-case > "$ORACLE_LOG" 2> "$ORACLE_ERR" || oracle_status=$?
    if [ "$oracle_status" -ne 134 ] ||
       ! grep -Fxq 'right-tail input=nihao offset=5 flags=0x0000018a' "$ORACLE_LOG" ||
       ! grep -Eq 'pinyin.cpp:3092:.*pinyin_get_right_pinyin_offset.*Assertion.*_check_offset\(matrix, right\).*failed' "$ORACLE_ERR"; then
        echo "FAIL: unexpected oracle right-tail observation (exit $oracle_status)"
        cat "$ORACLE_LOG" "$ORACLE_ERR"
        rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR" "$CASE_LOG"
        exit 1
    fi
    if ! ./bisect "$CAPI_SO" "$ORACLE_DATA" --right-tail-case > "$CASE_LOG" 2>&1 ||
       ! grep -Fxq 'right-tail: false; output untouched' "$CASE_LOG"; then
        echo "FAIL: unexpected C API right-tail observation"
        cat "$CASE_LOG"
        rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR" "$CASE_LOG"
        exit 1
    fi
    echo "oracle: expected SIGABRT, pinyin.cpp:3092 _check_offset(matrix, right)"
    echo "capi: expected false, output untouched (compatibility register row 14)"
    rm -f "$ORACLE_LOG" "$ORACLE_ERR" "$CASE_LOG"
fi

rm -f "$CAPI_LOG" "$CAPI_ERR" "${CAPI_LOG}.body"
echo ""
echo "bisection: PASS"
