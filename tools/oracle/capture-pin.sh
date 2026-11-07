#!/usr/bin/env bash
# Capture one run of the proposed pin observations, in the Debian pin environment.
#
#   OXPINYIN_PROVISION_CACHE=CACHE [PINYIN_MODEL_CACHE=MODEL] bash tools/oracle/capture-pin.sh RUN
#
# RUN names the run (1, 2, ...). The C API observations run on the image's tkrzw cell on the pinned
# libpinyin-data. The utility runs use the bdb, kc and tkrzw builds of
# `python -m tools.dev provision oracle --cache-dir CACHE`: CACHE/oracle/CELL and the build tree
# beside it, mounted read-only at /provision. The data sequence (C1) reads the verified model20
# extraction MODEL/extracted, the cache `provision oracle` fetched (default CACHE/model20), mounted
# read-only at /model20. tools/oracle/pin_capture.py checks the prefixes, the
# loaded libraries and the utility builds against the prefixes' records and the data against its
# pin, then stages the proposed observations and capture.json in target/pin-observations/RUN/.
# Nothing else is written (docs/runbooks/pin-captures.md).

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
run=${1:?usage: capture-pin.sh RUN}
[[ $run =~ ^[A-Za-z0-9_-]+$ ]] || { echo "FAIL: a run name uses letters, digits, _ and -: $run" >&2; exit 2; }
# shellcheck source=capture-image.sh
. tools/oracle/capture-image.sh
[[ -d ${OXPINYIN_PROVISION_CACHE:-} ]] || {
    echo "FAIL: set OXPINYIN_PROVISION_CACHE to a \`tools.dev provision oracle --cache-dir\` directory" >&2
    exit 2
}
model_cache=${PINYIN_MODEL_CACHE:-$OXPINYIN_PROVISION_CACHE/model20}
[[ -d $model_cache/extracted ]] || {
    echo "FAIL: $model_cache/extracted is not a model20 extraction; set PINYIN_MODEL_CACHE to the cache provision oracle used" >&2
    exit 2
}
OXPINYIN_CAPTURE_PROVISION=$(cd "$OXPINYIN_PROVISION_CACHE" && pwd) OXPINYIN_CAPTURE_MODEL=$(cd "$model_cache" && pwd) \
    run_in_capture_image target/pin-observations \
    tools/oracle/pin_capture.py capture "$run" --prefix /opt/reference/tkrzw --data /data \
    --provision-cache /provision --model-dir /model20/extracted --staging /work/target/pin-observations
