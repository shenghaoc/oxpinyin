#!/usr/bin/env bash
# run-pred-order-tkrzw-dropin.sh — the R1 drop-in differential against a
# tkrzw-built libpinyin (Debian testing's build). A thin label over
# run-pred-order-dropin.sh; see that script for the contract.
set -euo pipefail
echo "=== R1 drop-in differential: tkrzw compat path ==="
exec "$(dirname "$0")/run-pred-order-dropin.sh"
