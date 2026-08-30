#!/usr/bin/env bash
# run-pred-order-tkrzw-dropin.sh — the R1 drop-in differential against a
# tkrzw-built libpinyin (Debian testing's build). Verifies the data
# directory really holds tkrzw files, then execs
# run-pred-order-dropin.sh; see that script for the contract.
set -euo pipefail
# The label is a claim: check bigram.db's magic before measuring. An
# unset OXPINYIN_SYSTEM_DIR defers to the inner script's setup failure.
if [ -n "${OXPINYIN_SYSTEM_DIR:-}" ] && [ -f "${OXPINYIN_SYSTEM_DIR}/bigram.db" ]; then
    [ "$(head -c 9 "${OXPINYIN_SYSTEM_DIR}/bigram.db" | od -An -tx1 | tr -d ' \n')" \
        = "546b727a774844420a" ] \
        || { echo "${OXPINYIN_SYSTEM_DIR}/bigram.db does not carry the tkrzw HashDBM magic" >&2; exit 2; }
fi
echo "=== R1 drop-in differential: tkrzw compat path ==="
exec "$(dirname "$0")/run-pred-order-dropin.sh"
