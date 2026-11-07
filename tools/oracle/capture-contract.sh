#!/usr/bin/env bash
# Capture contract-diff references from the pin, in the Debian testing pin environment.
#
#   bash tools/oracle/capture-contract.sh CELL [capture options]
#
# CELL is bdb, kc or tkrzw. The options are those of `contract-diff.py capture`
# (--cases, --issue, --family, --cases-prefix, --runs, --case-timeout, --out). References
# are written to fixtures/contract-refs unless --out names another directory under the
# checkout. The pin is the published oracle image (`python -m tools.oracle.image image-tag`), pulled when
# absent; build it locally with build-contract-image.sh when unpublished.
# An investigative live comparison is a capture into a temporary --out, then
# `just runner contract-diff verify CELL ... --references <that directory>`.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
set -euo pipefail
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
cd "$repo"
cell=${1:?usage: capture-contract.sh CELL [capture options]}
shift
# --out DIR (or --out=DIR) names the reference directory, relative to the checkout, which is
# mounted writable; the checkout itself is read-only. Every other option passes through.
out=fixtures/contract-refs
args=()
while (($#)); do
    case $1 in
    --out) [[ $# -ge 2 ]] || { echo "FAIL: --out needs a directory" >&2; exit 2; }; out=$2; shift 2 ;;
    --out=*) out=${1#--out=}; shift ;;
    *) args+=("$1"); shift ;;
    esac
done
case $out in
'' | /* | .. | ../* | */.. | */../*) echo "FAIL: --out must be a relative path under the checkout without ..: $out" >&2; exit 2 ;;
esac
out=${out%/}
case $cell in bdb | kc | tkrzw) ;; *) echo "FAIL: unknown backend: $cell" >&2; exit 2 ;; esac
# shellcheck source=capture-image.sh
. tools/oracle/capture-image.sh
run_in_capture_image "$out" tools/bisection/contract-diff.py capture "$cell" \
    "/opt/reference/$cell" --data /data --out "/work/$out" "${args[@]}"
