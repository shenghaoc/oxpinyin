#!/usr/bin/env bash
# run-same-data-dir-diff.sh — the drop-in invariant, end to end.
#
#   same data directory + same backend + same input/state
#         → libpinyin.so  ≡  libpinyin_capi.so (oxpinyin)
#
# Drives every `<so> <systemdir>` C-ABI differential driver in this
# directory into the pin-built libpinyin and into oxpinyin's C ABI, both
# opened on ONE unchanged data directory (a libpinyin install's own
# `data/`, or an `oxpinyin-datagen compile` output — the file set is the
# same), and diffs the full logs. No conversion, no import, no fixture
# image: the directory is the test input to both implementations.
#
# Runs inside the perf-matrix container (tools/bisection/Dockerfile.perf-matrix):
#
#   cargo build --locked -p oxpinyin-capi                    # KC, the default
#   tools/bisection/run-same-data-dir-diff.sh \
#       /opt/libpinyin-kc/lib/libpinyin.so target/debug/libpinyin_capi.so \
#       /opt/libpinyin-kc/lib/libpinyin/data
#
#   cargo build --locked -p oxpinyin-capi --no-default-features --features tkrzw
#   tools/bisection/run-same-data-dir-diff.sh \
#       /opt/libpinyin-tkrzw/lib/libpinyin.so target/debug/libpinyin_capi.so \
#       /opt/libpinyin-tkrzw/lib/libpinyin/data
#
# usage: run-same-data-dir-diff.sh <libpinyin.so> <libpinyin_capi.so> <data-dir> [driver ...]
#
# Exit codes: 0 = identical on every driver; 1 = build/run failure;
# 2 = at least one driver's logs differ (each divergence is printed).
set -euo pipefail

oracle_so=${1:?path to the pin-built libpinyin.so}
capi_so=${2:?path to the oxpinyin libpinyin_capi.so}
data=${3:?data directory}
shift 3
# Resolve the caller's (possibly relative) paths before moving into the
# drivers' directory.
oracle_so=$(cd "$(dirname "$oracle_so")" && pwd)/$(basename "$oracle_so")
capi_so=$(cd "$(dirname "$capi_so")" && pwd)/$(basename "$capi_so")
data=$(cd "$data" && pwd)
cd "$(dirname "$0")"

# Every driver that takes exactly `<so> <systemdir>` and needs no user
# state beyond what it creates itself. pred-order-diff is included on
# purpose: its divergence is the registered prediction tie order
# (`docs/findings/upstream-divergences.md`, "Predicted-candidate tie
# order"), which this runner reports rather than hides.
default_drivers=(
  key-surface-diff
  dict-surface-diff
  phrase-surface-diff
  pred-order-diff
  predict-diff
  punct-diff
  addon-candidate-diff
  user-candidate-diff
  union-diff
  import-diff
  live-typing-diff
  nbest-train-diff
)
# bisect's surface mode is not in the default set: its offset sweep trips
# an upstream assertion (`_check_offset`, pinyin.cpp:2175) inside the pin
# itself. Name it explicitly to run it anyway.
drivers=("$@")
((${#drivers[@]})) || drivers=("${default_drivers[@]}")

for f in "$oracle_so" "$capi_so"; do
  [[ -f $f ]] || { echo "fatal: $f not found" >&2; exit 1; }
done
[[ -f $data/gb_char.bin ]] || { echo "fatal: $data holds no gb_char.bin" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
status=0
echo "data dir: $data"
echo "oracle:   $oracle_so"
echo "oxpinyin: $capi_so"

for driver in "${drivers[@]}"; do
  printf -- '--- %s ---\n' "$driver"
  # A few drivers use glib's GArray directly; link it when pkg-config
  # knows it, otherwise build without.
  glib_flags=$(pkg-config --cflags --libs glib-2.0 2>/dev/null || true)
  # shellcheck disable=SC2086
  if ! gcc -std=gnu11 -Wall -Wextra -O2 -o "$work/$driver" "$driver.c" -ldl $glib_flags 2>"$work/$driver.build"; then
    echo "  build failed:"; sed 's/^/    /' "$work/$driver.build"; status=1; continue
  fi
  run() {
    # Each side gets a fresh working directory: several drivers create
    # user state relative to the cwd, and both sides must start equal.
    local side=$1 so=$2 log=$3
    rm -rf "$work/$side"; mkdir -p "$work/$side"
    ( cd "$work/$side" && LD_LIBRARY_PATH="$(dirname "$so")" "$work/$driver" "$so" "$data" ) >"$log" 2>"$log.err"
  }
  if ! run oracle "$oracle_so" "$work/$driver.oracle"; then
    echo "  $driver crashed against the oracle:"; tail -5 "$work/$driver.oracle.err" | sed 's/^/    /'; status=1; continue
  fi
  if ! run oxpinyin "$capi_so" "$work/$driver.capi"; then
    echo "  $driver crashed against oxpinyin:"; tail -5 "$work/$driver.capi.err" | sed 's/^/    /'; status=1; continue
  fi
  if diff -u "$work/$driver.oracle" "$work/$driver.capi" >"$work/$driver.diff"; then
    echo "  IDENTICAL ($(wc -l <"$work/$driver.oracle") log lines)"
  else
    differing=$(grep -c '^[-+][^-+]' "$work/$driver.diff" || true)
    echo "  DIVERGENCE ($differing differing lines):"
    head -60 "$work/$driver.diff" | sed 's/^/    /'
    [[ $status == 0 ]] && status=2
  fi
done
echo
case $status in
  0) echo "same-data-dir differential: IDENTICAL on ${#drivers[@]} drivers" ;;
  2) echo "same-data-dir differential: DIVERGENCE" ;;
  *) echo "same-data-dir differential: FAILURE" ;;
esac
exit $status
