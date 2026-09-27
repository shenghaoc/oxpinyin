#!/usr/bin/env bash
# run-import-semantics-diff.sh — pinyin_iterator_add_phrase semantics
# differential (issue #534).
#
# Drives tools/bisection/import-semantics-diff.c into the pin-built
# libpinyin.so and oxpinyin's libpinyin_capi.so, both opened on ONE data
# directory (the oracle's own lib/libpinyin/data) with a fresh user dir per
# side and library, and diffs the logs byte for byte (add return values,
# the library's export live and after save + reopen, the lookup probe).
#
#   libraries 0 5 6 7 15      must be IDENTICAL.
#   libraries 16 255          the pin reads its 16-slot sub-index array out
#                             of bounds (phrase_index.h:630) — a SIGSEGV or
#                             a refusal, as the heap falls (class (b));
#                             oxpinyin must answer every add false and exit
#                             cleanly.
#   library 1                 PENDING — the pin adds into the system library;
#                             oxpinyin refuses until system items are modelled
#                             (#599). Reported, not gated.
#
# Run once per backend cell, with the capi built on the matching backend:
#
#   cargo build -p oxpinyin-capi --no-default-features --features tkrzw
#   tools/bisection/run-import-semantics-diff.sh \
#       "$PINYIN_ORACLE_PREFIX"/lib/libpinyin.so target/debug/libpinyin_capi.so \
#       "$PINYIN_ORACLE_PREFIX"/lib/libpinyin/data
#
# usage: run-import-semantics-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir>
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence.
set -euo pipefail
oracle_so=${1:?path to the pin-built libpinyin.so}
capi_so=${2:?path to the oxpinyin libpinyin_capi.so}
data=${3:?data directory}
oracle_so=$(cd "$(dirname "$oracle_so")" && pwd)/$(basename "$oracle_so")
capi_so=$(cd "$(dirname "$capi_so")" && pwd)/$(basename "$capi_so")
data=$(cd "$data" && pwd)
cd "$(dirname "$0")"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$work/driver" import-semantics-diff.c -ldl

# run <side> <so> <library> — the exit status is the driver's.
run() {
  local side=$1 so=$2 library=$3
  mkdir -p "$work/$side-$library"
  (cd "$work/$side-$library" && LD_LIBRARY_PATH="$(dirname "$so")" \
    "$work/driver" "$so" "$data" "$library") >"$work/$side-$library.log" 2>/dev/null
}

status=0
for library in 0 5 6 7 15; do
  if ! run oracle "$oracle_so" "$library"; then
    echo "library $library: FAIL (oracle driver exited nonzero)"; status=1; continue
  fi
  if ! run oxpinyin "$capi_so" "$library"; then
    echo "library $library: FAIL (oxpinyin driver exited nonzero)"; status=1; continue
  fi
  if diff -u "$work/oracle-$library.log" "$work/oxpinyin-$library.log" >"$work/$library.diff"; then
    echo "library $library: IDENTICAL ($(wc -l <"$work/oracle-$library.log") lines)"
  else
    echo "library $library: DIVERGENCE"
    head -40 "$work/$library.diff"
    [[ $status == 0 ]] && status=2
  fi
done

# Libraries 16 and 255: the pin's out-of-bounds read (class (b)).
for library in 16 255; do
  set +e
  run oracle "$oracle_so" "$library"
  oracle_rc=$?
  run oxpinyin "$capi_so" "$library"
  oxpinyin_rc=$?
  set -e
  if [[ $oxpinyin_rc != 0 ]] || grep -q ': true$' "$work/oxpinyin-$library.log"; then
    echo "library $library: DIVERGENCE (oxpinyin rc=$oxpinyin_rc; every add must be false)"
    [[ $status == 0 ]] && status=2
  else
    echo "library $library: oxpinyin refuses every add; pin rc=$oracle_rc (class (b), not reproduced)"
  fi
done

# Library 1: pending the system-library model.
if run oracle "$oracle_so" 1 && run oxpinyin "$capi_so" 1; then
  echo "library 1: PENDING #599 — pin accepts $(grep -c '^add .*: true$' "$work/oracle-1.log"), oxpinyin accepts $(grep -c '^add .*: true$' "$work/oxpinyin-1.log") (not gated)"
else
  echo "library 1: PENDING #599 — a driver exited nonzero (not gated)"
fi

case $status in
  0) echo "import-semantics-diff: IDENTICAL" ;;
  2) echo "import-semantics-diff: DIVERGENCE" ;;
  *) echo "import-semantics-diff: FAILURE" ;;
esac
exit $status
