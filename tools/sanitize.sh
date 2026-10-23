#!/usr/bin/env bash
# sanitize.sh — one entry point for the store backends' sanitizer gates.
#
# It only selects. Each backend keeps its own script, and with it its own
# instrumentation, flags and pass criteria; the nightly is NIGHTLY below:
#
#   bdb    tools/bdb/run-sanitizers.sh    Rust under ASan; C harnesses under ASan+UBSan
#   kc     tools/kc/run-sanitizers.sh     Rust under ASan + LeakSanitizer
#   tkrzw  tools/tkrzw/run-sanitizers.sh  Rust under ASan, on the verified libtkrzw
#   all    the three in that order; every gate runs even when an earlier one fails
#
# OXPINYIN_SANITIZER_TOOLCHAIN overrides NIGHTLY; it, OXPINYIN_SANITIZER_TARGET
# and OXPINYIN_SANITIZER_ALLOW_SKIP reach the scripts unchanged.
#
# Usage: tools/sanitize.sh <bdb|kc|tkrzw|all>
# Exit: the backend script's status; for all, 0 when every gate exits 0 and
# 1 otherwise; 2 on a usage error.

set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

# The one nightly of the three gates. Each backend script reads this line
# for its default, and so do the store-backends sanitizer jobs.
# Exact on purpose (#1074): a nightly is identified only by its date.
NIGHTLY=nightly-2026-09-07
export OXPINYIN_SANITIZER_TOOLCHAIN=${OXPINYIN_SANITIZER_TOOLCHAIN:-$NIGHTLY}

usage() {
	printf 'usage: %s <bdb|kc|tkrzw|all>\n' "${0##*/}" >&2
	exit 2
}
[ "$#" -eq 1 ] || usage
case $1 in
bdb | kc | tkrzw) exec bash "$here/$1/run-sanitizers.sh" ;;
all) ;;
*) usage ;;
esac

results=()
failed=0
for backend in bdb kc tkrzw; do
	printf '%s\n' "=== sanitize $backend ==="
	if bash "$here/$backend/run-sanitizers.sh"; then
		results+=("$backend: exit 0")
	else
		code=$?
		results+=("$backend: exit $code")
		failed=1
	fi
	printf '\n'
done
printf 'sanitize all: %s\n' "${results[@]}"
exit "$failed"
