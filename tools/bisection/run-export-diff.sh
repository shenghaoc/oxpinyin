#!/usr/bin/env bash
# run-export-diff.sh — phrase-export differential (issue #533).
#
# Drives tools/bisection/export-diff.c into the pin-built libpinyin.so and
# oxpinyin's libpinyin_capi.so, both opened on ONE data directory (the
# oracle's own lib/libpinyin/data) with a fresh user dir each, and diffs
# the logs byte for byte, unsorted: export order is part of the surface.
#
# Run once per backend cell, with the capi built on the matching backend:
#
#   cargo build -p oxpinyin-capi --no-default-features --features tkrzw
#   tools/bisection/run-export-diff.sh \
#       "$PINYIN_ORACLE_PREFIX"/lib/libpinyin.so target/debug/libpinyin_capi.so \
#       "$PINYIN_ORACLE_PREFIX"/lib/libpinyin/data
#
# usage: run-export-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir>
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence.
set -euo pipefail
REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# shellcheck source=tools/bisection/oracle-cell.sh
source "$REPO_ROOT/tools/bisection/oracle-cell.sh"
oracle_so=${1:?path to the pin-built libpinyin.so}
[[ -f $oracle_so ]] || { echo "SKIP: missing oracle: $oracle_so" >&2; exit 77; }
capi_so=${2:-${OXPINYIN_CAPI_SO:?prebuilt libpinyin required}}
data=${3:?data directory}
oracle_so=$(cd "$(dirname "$oracle_so")" && pwd)/$(basename "$oracle_so")
capi_so=$(cd "$(dirname "$capi_so")" && pwd)/$(basename "$capi_so")
data=$(cd "$data" && pwd)
cd "$(dirname "$0")"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$work/export-diff" export-diff.c -ldl

run() {
  local side=$1 so=$2
  mkdir -p "$work/$side"
  if ! (cd "$work/$side" && LD_LIBRARY_PATH="$(dirname "$so")" "$work/export-diff" "$so" "$data") \
    >"$work/$side.log" 2>"$work/$side.err"; then
    echo "FAIL: export-diff crashed against $side"
    tail -5 "$work/$side.err"
    exit 1
  fi
}
run oracle "$oracle_so"
run oxpinyin "$capi_so"

echo "oracle summary:"
grep -v '^row ' "$work/oracle.log" | sed 's/^/  /'
if diff -u "$work/oracle.log" "$work/oxpinyin.log" >"$work/diff"; then
  echo "export-diff: IDENTICAL ($(wc -l <"$work/oracle.log") lines)"
  exit 0
fi
echo "export-diff: DIVERGENCE ($(grep -c '^[-+][^-+]' "$work/diff" || true) differing lines)"
head -60 "$work/diff"
exit 2
