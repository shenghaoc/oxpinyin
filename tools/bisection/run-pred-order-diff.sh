#!/usr/bin/env bash
# run-pred-order-diff.sh — predicted-prefix row-order differential (the B1
# moving-number gate).
#
# Runs tools/bisection/pred-order-diff.c against the pin-built oracle and
# libpinyin_capi.so over matched tables, then reports, per prefix and API, how many
# PREDICTED_PREFIX rows sit at different positions — a number that moves
# across the B1 fix PR (baseline 2026-08-25: 好 174/178; the residual after
# the prefix-slice fix is the recorded Tkrzw bucket-walk store-layout
# divergence, docs/findings/upstream-divergences.md), not a binary verdict.
#
# Exit codes: 0 = identical; 77 = skipped; 1 = build/run failure;
# 2 = row-order divergence (the expected, measured state — do not wire
# into CI green until the B1 PR lands and the residual is the recorded
# divergence).
#
# Env: UNCOVERED_SYSTEM (the five-file system dir of
# run-uncovered-surface-diff.sh) and PINYIN_ORACLE_PREFIX, as there.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
# shellcheck source=tools/bisection/oracle-cell.sh
source ./oracle-cell.sh
# shellcheck source=tools/bisection/system-dir.sh
. ./system-dir.sh

echo "--- building pred-order-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o pred-order-diff \
    pred-order-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
oracle_cell_artifact OXPINYIN_CAPI_SO libpinyin_capi.so oxpinyin-capi
CAPI_SO=$OXPINYIN_CAPI_SO
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 77
fi
if ! grep -Fxq "$EXPECTED_PIN_REF" "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin for the $ORACLE_DBM cell"
    echo "  expected $EXPECTED_PIN_REF"
    exit 77
fi

# UNCOVERED_SYSTEM first, then OXPINYIN_SYSTEM_DIR -- the one name that
# works across every differential, so a whole sweep needs one export
# rather than a different variable per runner (see system-dir.sh).
# The tables are looked for in the extension the built capi opens
# (system_dir_detect_ext: .db for the default Berkeley DB build, .kct or
# .tkt for an explicit --features build), not in a hard-coded one.
SYSTEM="${UNCOVERED_SYSTEM:-${OXPINYIN_SYSTEM_DIR:-}}"
if [[ -z "$SYSTEM" ]] || ! system_dir_detect_ext "$SYSTEM" >/dev/null; then
    echo "SKIP: UNCOVERED_SYSTEM must name the five-file system dir"
    echo "  (see run-uncovered-surface-diff.sh)"
    exit 77
fi

# Capture stdout in memory. The oracle's unbuffered user.conf diagnostic
# stays on stderr so it cannot corrupt a compared, buffered stdout row.
if ! CAPI_LOG=$(tkrzw_run "$ORACLE_DBM" subject "$CAPI_SO" ./pred-order-diff "$CAPI_SO" "$SYSTEM"); then
    echo "FAIL: pred-order-diff crashed against oxpinyin-capi"
    printf '%s\n' "$CAPI_LOG"
    exit 1
fi
if ! ORACLE_LOG=$(tkrzw_run "$ORACLE_DBM" pin "$ORACLE_SO" ./pred-order-diff "$ORACLE_SO" "$ORACLE_DATA"); then
    echo "FAIL: pred-order-diff crashed against oracle"
    printf '%s\n' "$ORACLE_LOG"
    exit 1
fi

echo "--- per-prefix position mismatches (the moving number) ---"
total=0
rows=0
status=0
for tag in hao de yi ni zhongguo wo shi le \
           plain-hao plain-de plain-yi plain-ni plain-zhongguo plain-wo plain-shi plain-le; do
    oracle_rows=$(grep "^pred-$tag:" <<< "$ORACLE_LOG" || true)
    capi_rows=$(grep "^pred-$tag:" <<< "$CAPI_LOG" || true)
    n=$(awk 'NF { n++ } END { print n+0 }' <<< "$oracle_rows")
    if [[ "$n" -eq 0 ]]; then
        echo "$tag: no oracle rows (prefix inactive?)"
        status=1
        continue
    fi
    mismatches=$(paste -d/ <(printf '%s\n' "$oracle_rows") \
        <(printf '%s\n' "$capi_rows") | awk -F/ '$1 != $2 { c++ } END { print c+0 }')
    echo "$tag: ${mismatches}/${n} rows at different positions"
    total=$((total + mismatches))
    rows=$((rows + n))
done

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
