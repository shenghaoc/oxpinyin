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
# same), and compares full stdout. No conversion, no import, no fixture
# image: the directory is the test input to both implementations.
#
# Runs inside the perf-matrix container (tools/bisection/Dockerfile.perf-matrix):
#
#   cargo build --locked -p oxpinyin-capi --no-default-features --features kyotocabinet
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
REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
# shellcheck source=tools/bisection/oracle-cell.sh
source "$REPO_ROOT/tools/bisection/oracle-cell.sh"

oracle_so=${1:?path to the pin-built libpinyin.so}
[[ -f $oracle_so ]] || { echo "SKIP: missing oracle: $oracle_so" >&2; exit 77; }
capi_so=${2:-${OXPINYIN_CAPI_SO:?prebuilt libpinyin required}}
data=${3:?data directory}
shift 3
# Resolve the caller's (possibly relative) paths before moving into the
# drivers' directory.
oracle_so=$(cd "$(dirname "$oracle_so")" && pwd)/$(basename "$oracle_so")
capi_so=$(cd "$(dirname "$capi_so")" && pwd)/$(basename "$capi_so")
data=$(cd "$data" && pwd)
cd "$(dirname "$0")"

# Drivers taking `<so> <systemdir>` and needing no user
# state beyond what it creates itself. pred-order-diff is included on
# purpose: its divergence is the registered prediction tie order
# (`docs/findings/upstream-divergences.md`, "Predicted-candidate tie
# order"), which this runner reports rather than hides. DYNAMIC_ADJUST
# additionally runs on/off with fresh profiles and checks non-vacuity.
default_drivers=(
  key-surface-diff
  key-parse-diff
  tie-order-diff
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
  dynamic-adjust-diff
)
# bisect's surface mode is not in the default set: its offset sweep trips
# an upstream `_check_offset` assertion inside the pin itself — at the pin
# 074a2219 the `assert(_check_offset(...))` call sites, pinyin.cpp:3035,
# :3057, :3067, :3092 and :3204 (`_check_offset` itself, :2163-2182, now
# returns false; the :2175 once cited here was the 0c5e80e1 pin's assert
# inside it). Name it explicitly to run it anyway.
drivers=("$@")
((${#drivers[@]})) || drivers=("${default_drivers[@]}")

for f in "$oracle_so" "$capi_so"; do
  [[ -f $f ]] || { echo "fatal: $f not found" >&2; exit 1; }
done
[[ -f $data/gb_char.bin ]] || { echo "fatal: $data holds no gb_char.bin" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Stream stdout, including trailing newlines, into private scratch files.
# Stderr remains a diagnostic stream. The EXIT trap removes all scratch data.
oracle_output=$work/oracle.stdout
capi_output=$work/oxpinyin.stdout
diff_output=$work/comparison.diff
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
  if ! gcc -std=gnu11 -Wall -Wextra -O2 -o "$work/$driver" "$driver.c" -ldl $glib_flags; then
    echo "  build failed"; status=1; continue
  fi
  driver_args=()
  [[ $driver != punct-diff ]] || driver_args=(matched-tables)
  run() {
    # Each side gets a fresh working directory: several drivers create
    # user state relative to the cwd, and both sides must start equal.
    local side=$1 so=$2
    rm -rf "${work:?}/$side"; mkdir -p "$work/$side"
    ( cd "$work/$side" && LD_LIBRARY_PATH="$(dirname "$so")" "$work/$driver" "$so" "$data" "${driver_args[@]}" )
  }
  if [[ $driver == dynamic-adjust-diff ]]; then
    # This driver also needs a bit state and a fresh user profile. Preserve
    # its standalone runner's non-vacuity requirement before comparing.
    failed=0
    declare -A logs=() choices=()
    for engine in oracle oxpinyin; do
      if [[ $engine == oracle ]]; then so=$oracle_so; else so=$capi_so; fi
      for mode in on off; do
        dest="$work/$engine-$mode"
        rm -rf "$dest"
        mkdir -p "$dest/user"
        run_dynamic() {
          ( cd "$dest" && LD_LIBRARY_PATH="$(dirname "$so")" \
              "$work/$driver" "$so" "$data" "$mode" "$dest/user" )
        }
        output=$work/$engine-$mode.stdout
        if ! run_dynamic > "$output"; then
          echo "  $driver crashed against $engine ($mode)"
          status=1; failed=1
        fi
        logs[$engine-$mode]=$output
      done
    done
    [[ $failed == 0 ]] || continue
    for side in oracle-on oracle-off oxpinyin-on oxpinyin-off; do
      choices[$side]=$(grep '^CHOICE|' "${logs[$side]}" || true)
      if [[ $(printf '%s\n' "${choices[$side]}" | wc -l) -ne 7 ]]; then
        echo "  FAIL: $side did not report all fixed choices"
        status=1; failed=1
      elif [[ ${choices[oracle-on]} != "${choices[$side]}" ]]; then
        echo "  FAIL: chosen identity or advanced cursor differs for $side"
        status=1; failed=1
      fi
    done
    [[ $failed == 0 ]] || continue
    for engine in oracle oxpinyin; do
      if cmp -s "${logs[$engine-on]}" "${logs[$engine-off]}"; then
        echo "  VACUOUS: $engine produced identical output with the bit set and clear."
        status=1; failed=1
      fi
    done
    [[ $failed == 0 ]] || continue
    echo "  non-vacuity: both engines' output changes with the bit"
    for mode in on off; do
      if diff -u --label "oracle-$mode" --label "oxpinyin-$mode" \
          "${logs[oracle-$mode]}" "${logs[oxpinyin-$mode]}" > "$diff_output"; then
        echo "  DYNAMIC_ADJUST=$mode: IDENTICAL ($(wc -l < "${logs[oracle-$mode]}") log lines)"
      else
        echo "  DYNAMIC_ADJUST=$mode: DIVERGENCE"
        sed -n '1,60p' "$diff_output" | sed 's/^/    /'
        [[ $status == 0 ]] && status=2
      fi
    done
    continue
  fi
  if ! run oracle "$oracle_so" > "$oracle_output"; then
    echo "  $driver crashed against the oracle"; status=1; continue
  fi
  if ! run oxpinyin "$capi_so" > "$capi_output"; then
    echo "  $driver crashed against oxpinyin"; status=1; continue
  fi
  if diff -u --label oracle --label oxpinyin \
      "$oracle_output" "$capi_output" > "$diff_output"; then
    echo "  IDENTICAL ($(wc -l < "$oracle_output") log lines)"
  else
    differing=$(grep -c '^[-+][^-+]' "$diff_output" || true)
    echo "  DIVERGENCE ($differing differing lines):"
    sed -n '1,60p' "$diff_output" | sed 's/^/    /'
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
