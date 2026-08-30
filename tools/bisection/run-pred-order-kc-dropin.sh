#!/usr/bin/env bash
# run-pred-order-kc-dropin.sh — the R1 drop-in differential against a
# Kyoto-Cabinet-built libpinyin (Fedora's build). A thin label over
# run-pred-order-dropin.sh; see that script for the contract.
set -euo pipefail
echo "=== R1 drop-in differential: Kyoto Cabinet compat path ==="
exec "$(dirname "$0")/run-pred-order-dropin.sh"
