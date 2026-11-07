#!/usr/bin/env bash
# Capture one run of the apostrophe observations on the pin, in the Debian pin environment.
#
#   bash tools/oracle/capture-pin.sh RUN
#
# RUN names the run (1, 2, ...). The pin is the image's tkrzw cell on the pinned libpinyin-data.
# tools/oracle/pin_capture.py checks the prefix, the loaded library against the prefix's record and
# the data against its pin, then stages the proposed observations and capture.json in
# target/pin-observations/RUN/. Nothing else is written (docs/runbooks/pin-captures.md).

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/../.."
run=${1:?usage: capture-pin.sh RUN}
[[ $run =~ ^[A-Za-z0-9_-]+$ ]] || { echo "FAIL: a run name uses letters, digits, _ and -: $run" >&2; exit 2; }
# shellcheck source=capture-image.sh
. tools/oracle/capture-image.sh
run_in_capture_image target/pin-observations tools/oracle/pin_capture.py capture "$run" \
    --prefix /opt/reference/tkrzw --data /data --staging /work/target/pin-observations
