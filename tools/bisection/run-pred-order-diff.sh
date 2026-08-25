#!/usr/bin/env bash
# run-pred-order-diff.sh — predicted-prefix row-order differential (the B1
# moving-number gate).
#
# Runs tools/bisection/pred-order-diff.c against the pin-built oracle and
# libpinyin_capi.so over matched tables, then reports, per prefix, how many
# PREDICTED_PREFIX rows sit at different positions — a number that moves
# across the B1 fix PR (baseline 2026-08-25: 好 174/178; the residual after
# the prefix-slice fix is the recorded Tkrzw bucket-walk store-layout
# divergence, docs/findings/upstream-divergences.md), not a binary verdict.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure;
# 2 = row-order divergence (the expected, measured state — do not wire
# into CI green until the B1 PR lands and the residual is the recorded
# divergence).
#
# Env: UNCOVERED_SYSTEM (the five-file system dir of
# run-uncovered-surface-diff.sh) and PINYIN_ORACLE_PREFIX, as there.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building pred-order-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o pred-order-diff \
    pred-order-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi

SYSTEM="${UNCOVERED_SYSTEM:-}"
if [[ -z "$SYSTEM" ]] || ! [[ -f "$SYSTEM/phrase_index.redb" ]]; then
    echo "SKIP: UNCOVERED_SYSTEM must name the five-file system dir"
    echo "  (see run-uncovered-surface-diff.sh)"
    exit 0
fi

# stderr stays OUT of the compared logs: the oracle writes an
# unbuffered user.conf diagnostic whose interleaving would corrupt a
# buffered stdout row and skew the row counts.
CAPI_LOG="$(mktemp)"
CAPI_ERR="$(mktemp)"
ORACLE_LOG="$(mktemp)"
ORACLE_ERR="$(mktemp)"
if ! ./pred-order-diff "$CAPI_SO" "$SYSTEM" > "$CAPI_LOG" 2> "$CAPI_ERR"; then
    echo "FAIL: pred-order-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$CAPI_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 1
fi
if ! ./pred-order-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> "$ORACLE_ERR"; then
    echo "FAIL: pred-order-diff crashed against oracle"
    cat "$ORACLE_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$ORACLE_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 1
fi

echo "--- per-prefix position mismatches (the moving number) ---"
total=0
rows=0
status=0
for tag in hao de yi ni zhongguo wo shi le; do
    grep "^pred-$tag:" "$ORACLE_LOG" > "/tmp/pred-order-oracle-$tag" || true
    grep "^pred-$tag:" "$CAPI_LOG" > "/tmp/pred-order-capi-$tag" || true
    n=$(wc -l < "/tmp/pred-order-oracle-$tag")
    if [[ "$n" -eq 0 ]]; then
        echo "$tag: no oracle rows (prefix inactive?)"
        status=1
        continue
    fi
    mismatches=$(paste -d/ "/tmp/pred-order-oracle-$tag" \
        "/tmp/pred-order-capi-$tag" | awk -F/ '$1 != $2 { c++ } END { print c+0 }')
    echo "$tag: ${mismatches}/${n} rows at different positions"
    total=$((total + mismatches))
    rows=$((rows + n))
    rm -f "/tmp/pred-order-oracle-$tag" "/tmp/pred-order-capi-$tag"
done
rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"

if (( status != 0 )); then
    echo "FAIL: a prefix produced no oracle rows"
    exit 1
fi
if (( total == 0 )); then
    echo "pred-order-diff: IDENTICAL ($rows rows)"
    exit 0
fi
echo "DIVERGENCE: $total/$rows rows at different positions"
echo "  Raw metric (text shape + order). Pre-fix baseline 2026-08-25: ~all"
echo "  rows differ; after the prefix-slice fix the residual is order-only"
echo "  (measured hao 174/178) — the recorded Tkrzw bucket-walk divergence."
echo "  See docs/findings/uncovered-surface-differentials.md."
exit 2
