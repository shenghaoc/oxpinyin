#!/usr/bin/env bash
# run-zhuyin-import-diff.sh — zhuyin_iterator_add_phrase differential
# (issue #575).
#
# Drives tools/bisection/zhuyin-import-diff.c into the pin-built
# libzhuyin.so (a prefix built with --enable-libzhuyin) and oxpinyin's
# libzhuyin_capi.so, both opened on ONE data directory (the oracle's own
# lib/libpinyin/data) with a fresh user dir per side and library, and diffs
# the logs byte for byte (add return values, then every battery phrase's
# tokens with their unigram frequencies and pronunciations, live and after
# save + reopen). Nothing is masked: #600 (the unigram field) and #601
# (library 5/6 pronunciations read from the addon facade) are both fixed.
#
#   libraries 0 1 2 3 4 5 6 7 15 must be IDENTICAL.
#   libraries 16 255          the pin reads its 16-slot sub-index array out
#                             of bounds (phrase_index.h:630) — a SIGSEGV or
#                             a refusal, as the heap falls (class (b));
#                             oxpinyin must answer every add false and exit
#                             cleanly.
#
# Run once per backend cell, with the capi built on the matching backend:
#
#   cargo build -p oxpinyin-zhuyin-capi --no-default-features --features tkrzw
#   tools/bisection/run-zhuyin-import-diff.sh \
#       "$ZHUYIN_ORACLE_PREFIX"/lib/libzhuyin.so target/debug/libzhuyin_capi.so \
#       "$ZHUYIN_ORACLE_PREFIX"/lib/libpinyin/data
#
# usage: run-zhuyin-import-diff.sh <libzhuyin.so> <libzhuyin_capi.so> <data-dir>
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence.
set -euo pipefail
oracle_so=${1:?path to the pin-built libzhuyin.so}
capi_so=${2:?path to the oxpinyin libzhuyin_capi.so}
data=${3:?data directory}
oracle_so=$(cd "$(dirname "$oracle_so")" && pwd)/$(basename "$oracle_so")
capi_so=$(cd "$(dirname "$capi_so")" && pwd)/$(basename "$capi_so")
data=$(cd "$data" && pwd)
cd "$(dirname "$0")"

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# shellcheck disable=SC2046  # pkg-config's flags are word lists.
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$work/driver" zhuyin-import-diff.c -ldl \
  $(pkg-config --cflags --libs glib-2.0)

# run <side> <so> <library> — the exit status is the driver's.
run() {
  local side=$1 so=$2 library=$3
  mkdir -p "$work/$side-$library"
  (cd "$work/$side-$library" && LD_LIBRARY_PATH="$(dirname "$so")" \
    "$work/driver" "$so" "$data" "$library") >"$work/$side-$library.log" 2>/dev/null
}

status=0
for library in 0 1 2 3 4 5 6 7 15; do
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

case $status in
  0) echo "zhuyin-import-diff: IDENTICAL" ;;
  2) echo "zhuyin-import-diff: DIVERGENCE" ;;
  *) echo "zhuyin-import-diff: FAILURE" ;;
esac
exit $status
