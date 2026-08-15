#!/usr/bin/env bash
# run-import-diff.sh — W7-T1 user-import differential.
#
# Drives the identical scripted C-ABI import sequence
# (tools/bisection/import-diff.c) into libpinyin_capi.so and the pin-built
# libpinyin.so, exports both engines' user data through the W6-T7 phrase
# export iterator, and diffs the (phrase, pinyin, count) triple sets with
# exact-integer equality. This is the second half of the same value-level
# surface the W6-T7 train differential covered.
#
# Env-gated on the pin-built oracle, mirroring W6-T7: PINYIN_ORACLE_PREFIX
# (default $HOME/.local/opt/pinyin-oracle) must hold the pin-verified prefix
# from tools/oracle/build-oracle.sh. Absent -> skip with a diagnostic, exit 0.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# ── Build the driver ────────────────────────────────────────────────────

echo "--- building import-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o import-diff import-diff.c -ldl
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

# ── Drive both engines once ─────────────────────────────────────────────

CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
if ! ./import-diff "$CAPI_SO" "$REPO_ROOT/fixtures/w3" > "$CAPI_LOG" 2> /dev/null; then
    echo "FAIL: import-diff crashed against pinyin-capi"
    cat "$CAPI_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi
if ! ./import-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> /dev/null; then
    echo "FAIL: import-diff crashed against the oracle"
    cat "$ORACLE_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi

echo "--- capi triples ---"
sort "$CAPI_LOG" | grep -E '^(add |empty batch|save|phrase)'
echo "--- oracle triples ---"
sort "$ORACLE_LOG" | grep -E '^(add |empty batch|save|phrase)'

if diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") > /dev/null; then
    echo "import-diff: IDENTICAL"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 0
fi

echo "DIVERGENCE: import logs differ"
diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") || true
rm -f "$CAPI_LOG" "$ORACLE_LOG"
exit 2
