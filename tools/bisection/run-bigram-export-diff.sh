#!/usr/bin/env bash
# run-bigram-export-diff.sh — user-bigram export differential (issue #541,
# register row 36).
#
# Drives tools/bisection/bigram-export-diff.c into a pin-built libpinyin.so
# and oxpinyin's libpinyin_capi.so, both opened on ONE data directory (the
# oracle's own lib/libpinyin/data) with a fresh user dir per side and
# scenario (empty, one, two, many, repeat).
#
# The oracle prefix must carry the instrumentation patch
# tools/bisection/patches/bigram-export-strjoinv: the unpatched pin reads
# past its join arrays (pinyin.cpp:843-849, class (b)) and crashes before a
# second row on most heaps. The patch changes no walk; see its header.
#
#   tools/oracle/build-oracle.sh --dbm <cell> \
#       --apply-patches tools/bisection/patches/bigram-export-strjoinv \
#       --prefix <patched-prefix>
#   cargo build -p oxpinyin-capi [--no-default-features --features <cell>]
#   tools/bisection/run-bigram-export-diff.sh \
#       <patched-prefix>/lib/libpinyin.so target/debug/libpinyin_capi.so \
#       <patched-prefix>/lib/libpinyin/data
#
# Gate (BIGRAM_DIFF_GATE):
#   returns (default) — every scenario's get= column obeys the pin's law on
#                       both sides (get_next answers has_next after the row:
#                       true while rows remain, false on the last — register
#                       row 36), and the scenarios whose walk has one
#                       predecessor (empty, one) are byte-identical. The full
#                       walks are printed, not gated: their row set and order
#                       follow the pin's DB walk (#541's order points).
#   full              — every scenario byte-identical.
#
# usage: run-bigram-export-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir>
#
# Exit codes: 0 = pass; 1 = build/run/provisioning failure; 2 = divergence.
set -euo pipefail
oracle_so=${1:?path to the patched pin-built libpinyin.so}
capi_so=${2:?path to the oxpinyin libpinyin_capi.so}
data=${3:?data directory}
gate=${BIGRAM_DIFF_GATE:-returns}
oracle_so=$(cd "$(dirname "$oracle_so")" && pwd)/$(basename "$oracle_so")
capi_so=$(cd "$(dirname "$capi_so")" && pwd)/$(basename "$capi_so")
data=$(cd "$data" && pwd)
cd "$(dirname "$0")"

prefix=$(cd "$(dirname "$oracle_so")/.." && pwd)
if ! grep -q 'pin-bigram-export-strjoinv.patch' "$prefix/oracle-patches.sha256" 2>/dev/null; then
  echo "FAIL: $prefix was not built with tools/bisection/patches/bigram-export-strjoinv"
  exit 1
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$work/driver" bigram-export-diff.c -ldl

run() {
  local side=$1 so=$2 scenario=$3
  mkdir -p "$work/$side-$scenario"
  (cd "$work/$side-$scenario" && LD_LIBRARY_PATH="$(dirname "$so")" \
    "$work/driver" "$so" "$data" "$scenario") >"$work/$side-$scenario.log" 2>/dev/null
}

# The row-36 law over one log: every get= but the last is true, the last
# false, and no row came back empty — an always-true get_next shows up as a
# trailing "(null)" row the driver's loop fetched past the end.
returns_law() {
  awk -F'\tget=' '/^row \(null\)/{bad=1} /^row /{v[n++]=$2}
    END{if(bad) exit 1; for(i=0;i<n;i++) if(v[i]!=(i<n-1?"true":"false")) exit 1}' "$1"
}

status=0
for scenario in empty one two many repeat; do
  if ! run oracle "$oracle_so" "$scenario"; then
    echo "$scenario: FAIL (oracle driver exited nonzero)"; status=1; continue
  fi
  if ! run oxpinyin "$capi_so" "$scenario"; then
    echo "$scenario: FAIL (oxpinyin driver exited nonzero)"; status=1; continue
  fi
  law=ok
  returns_law "$work/oracle-$scenario.log" || law="pin breaks the law"
  returns_law "$work/oxpinyin-$scenario.log" || law="oxpinyin breaks the law"
  if diff -u "$work/oracle-$scenario.log" "$work/oxpinyin-$scenario.log" >"$work/$scenario.diff"; then
    echo "$scenario: IDENTICAL ($(grep -c '^row ' "$work/oracle-$scenario.log") rows)"
    continue
  fi
  gated=$([[ $gate == full || $scenario == empty || $scenario == one ]] && echo yes || echo no)
  if [[ $law != ok ]]; then
    echo "$scenario: DIVERGENCE (row 36: $law)"
    [[ $status == 0 ]] && status=2
  elif [[ $gated == yes ]]; then
    echo "$scenario: DIVERGENCE"
    [[ $status == 0 ]] && status=2
  else
    echo "$scenario: returns law holds on both sides; walk differs (pin $(grep -c '^row ' "$work/oracle-$scenario.log") rows, oxpinyin $(grep -c '^row ' "$work/oxpinyin-$scenario.log") rows; not gated)"
  fi
  head -40 "$work/$scenario.diff"
done

case $status in
  0) echo "bigram-export-diff ($gate): PASS" ;;
  2) echo "bigram-export-diff ($gate): DIVERGENCE" ;;
  *) echo "bigram-export-diff ($gate): FAILURE" ;;
esac
exit $status
