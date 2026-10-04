#!/usr/bin/env bash
# run-addon-candidate-diff.sh — W11 unique ADDON-candidate differential.
# Does not edit run-import-diff.sh or run-train-diff.sh.
#
# Capi uses the complete native W3 fixture for the selected backend,
# including the art chunk and shared addon indexes.
set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
# shellcheck source=oracle-cell.sh
source ./oracle-cell.sh
SYS="$(mktemp -d)"
trap 'rm -rf "$SYS"' EXIT
# Each backend directory is a complete native-layout data set, including
# table.conf, chunk files and the shared addon index pair.
FIXTURE_EXT=${OXPINYIN_CAPI_BACKEND_EXT:-db}
case "$FIXTURE_EXT" in
    db|kct|tkt) ;;
    *) echo "fatal: invalid fixture backend: $FIXTURE_EXT" >&2; exit 1 ;;
esac
cp -a "$REPO_ROOT/fixtures/w3/$FIXTURE_EXT/." "$SYS/"
printf '%s\n' '\data model interpolation' '\1-gram' '\item 1 ok count 1' \
    > "$SYS/interpolation2.text"
export CAPI_W11_SYSTEM_DIR="$SYS"
./run-w11-diff.sh addon-candidate-diff "$@"
