#!/usr/bin/env bash
# #599: instruction counts (including phase breakdown), peak heap, and mapped
# pages for import/save or cold open. Profiling output stays in memory;
# the new work directory contains only the driver and fresh user profiles.
# Usage: <pin-so> <ox-so> <system-data-dir> <new-work-dir> [iterations|--cold-open]
#        [--function-order ARM/FACADE=POLICY] [--unordered-arm ARM] [--jobs N]
# The collections (pin/ox x Callgrind/heap/pages) are orchestrated by
# system_import_profile.py: independent, so they run in an admitted pool and
# report in a fixed order; --jobs 1 runs them one after another.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail
python_request=$(<"$(dirname "${BASH_SOURCE[0]}")/../../.python-version")
pin=$(realpath "${1:?pin-so}")
ox=$(realpath "${2:?ox-so}")
data=$(realpath "${3:?system-data-dir}")
out=${4:?new-output-dir}
shift 4
iterations=1000
if (( $# )) && [[ "$1" != --function-order && "$1" != --unordered-arm && "$1" != --jobs ]]; then
    iterations=$1
    shift
fi
root=$(cd "$(dirname "$0")" && pwd)
source "$root/../packaging/function-order.sh"
jobs=()
rest=()
while (( $# )); do
    case "$1" in
        --jobs)
            [[ $# -ge 2 && -n "${2:-}" ]] || { echo "--jobs needs a count" >&2; exit 2; }
            jobs=(--jobs "$2"); shift 2 ;;
        *) rest+=("$1"); shift ;;
    esac
done
function_order_arguments "${rest[@]}"
function_order_measurement --library "ox/pinyin=$ox" \
    --reference-arm pin --library "pin/pinyin=$pin" "${FUNCTION_ORDER_ARGS[@]}"
mkdir "$out"
out=$(realpath "$out")
cc -std=gnu11 -Wall -Wextra -Werror -O2 "$root/system-import-profile.c" -ldl -o "$out/driver"
uv run --no-project --managed-python --python "$python_request" python -X lazy_imports=normal \
    "$root/system_import_profile.py" "${jobs[@]}" -- "$pin" "$ox" "$data" "$out" "$iterations"
