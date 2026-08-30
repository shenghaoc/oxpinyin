#!/usr/bin/env bash
# run-pred-order-kc-dropin.sh — the R1 drop-in differential against a
# Kyoto-Cabinet-built libpinyin (Fedora's build). Verifies the data
# directory really holds Kyoto Cabinet files, then execs
# run-pred-order-dropin.sh; see that script for the contract.
set -euo pipefail
# The label is a claim: check bigram.db's magic AND its type byte — the
# pin's ngram is a Kyoto Cabinet HashDB (`KC\n\0`, 0x30 at offset 8) —
# before measuring. An unset OXPINYIN_SYSTEM_DIR defers to the inner
# script's setup failure.
if [ -n "${OXPINYIN_SYSTEM_DIR:-}" ] && [ -f "${OXPINYIN_SYSTEM_DIR}/bigram.db" ]; then
    header=$(head -c 9 "${OXPINYIN_SYSTEM_DIR}/bigram.db" | od -An -tx1 | tr -d ' \n') \
        || { echo "cannot read ${OXPINYIN_SYSTEM_DIR}/bigram.db" >&2; exit 2; }
    case "$header" in
        4b430a00????????30*) : ;;
        *) echo "${OXPINYIN_SYSTEM_DIR}/bigram.db is not a Kyoto Cabinet HashDB (magic 4b430a00, type 0x30 at offset 8)" >&2; exit 2 ;;
    esac
fi
echo "=== R1 drop-in differential: Kyoto Cabinet compat path ==="
exec "$(dirname "$0")/run-pred-order-dropin.sh"
