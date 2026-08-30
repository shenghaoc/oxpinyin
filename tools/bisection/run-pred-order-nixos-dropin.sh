#!/usr/bin/env bash
# run-pred-order-nixos-dropin.sh — the R1 drop-in differential against a
# Kyoto-Cabinet-built libpinyin from nixpkgs (NixOS's packaging). Verifies
# the data directory really holds Kyoto Cabinet files, then execs
# run-pred-order-dropin.sh; see that script for the contract.
set -euo pipefail
# The label is a claim: check bigram.db's magic before measuring. An
# unset OXPINYIN_SYSTEM_DIR defers to the inner script's setup failure.
if [ -n "${OXPINYIN_SYSTEM_DIR:-}" ] && [ -f "${OXPINYIN_SYSTEM_DIR}/bigram.db" ]; then
    [ "$(head -c 4 "${OXPINYIN_SYSTEM_DIR}/bigram.db" | od -An -tx1 | tr -d ' \n')" \
        = "4b430a00" ] \
        || { echo "${OXPINYIN_SYSTEM_DIR}/bigram.db does not carry the Kyoto Cabinet magic" >&2; exit 2; }
fi
echo "=== R1 drop-in differential: Kyoto Cabinet compat path (nixpkgs) ==="
exec "$(dirname "$0")/run-pred-order-dropin.sh"
