#!/usr/bin/env bash
# sanitize.sh — one entry point for the store backends' sanitizer gates.
#
# It only selects. Each backend keeps its own script, and with it its own
# instrumentation, flags and pass criteria; the nightly is NIGHTLY below:
#
#   bdb    tools/bdb/run-sanitizers.sh    Rust under ASan; C harnesses under ASan+UBSan
#   kc     tools/kc/run-sanitizers.sh     Rust under ASan + LeakSanitizer on Linux
#   tkrzw  tools/tkrzw/run-sanitizers.sh  Rust under ASan, on the verified libtkrzw
#   all    the three in that order; every gate runs even when an earlier one fails
#
# Linux and macOS select address and thread; only the macOS thread gates
# have been run, so Linux thread stays unverified. Linux also selects leak
# for bdb and kc; tkrzw leak is refused on every host because its harness
# retains process-lifetime allocations. macOS refuses standalone leak on
# arm64 because the Darwin runtime has no target. MemorySanitizer is refused
# everywhere: no Darwin target and no in-tree wrapper.
# OXPINYIN_SANITIZER_TOOLCHAIN overrides NIGHTLY; it,
# OXPINYIN_SANITIZER_TARGET and OXPINYIN_SANITIZER_ALLOW_SKIP reach the
# scripts unchanged.
#
# Usage: tools/sanitize.sh [--check] <bdb|kc|tkrzw|all> [address|thread|leak|memory]
# Exit: the backend script's status; for all, 0 when every gate exits 0 and
# 1 otherwise; 2 on a usage error. --check validates and prints the selection
# without running a gate.

# shellcheck source=lib/bash-floor.sh
. "${0%/*}/lib/bash-floor.sh"

set -euo pipefail
here=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

# The one nightly of the three gates. Each backend script reads this line
# for its default, and so do the store-backends sanitizer jobs.
# Exact on purpose (#1074): a nightly is identified only by its date.
NIGHTLY=nightly-2026-09-07
export OXPINYIN_SANITIZER_TOOLCHAIN=${OXPINYIN_SANITIZER_TOOLCHAIN:-$NIGHTLY}

usage() {
	printf 'usage: %s [--check] <bdb|kc|tkrzw|all> [address|thread|leak|memory]\n' "${0##*/}" >&2
	exit 2
}

check=0
if [ "${1:-}" = --check ]; then
	check=1
	shift
fi
[ "$#" -ge 1 ] && [ "$#" -le 2 ] || usage
backend=$1
sanitizer=${2:-address}
case $sanitizer in
asan) sanitizer=address ;;
tsan) sanitizer=thread ;;
lsan) sanitizer=leak ;;
msan) sanitizer=memory ;;
esac
case $backend in
bdb | kc | tkrzw | all) ;;
*) usage ;;
esac
case $sanitizer in
address | thread | leak | memory) ;;
*) usage ;;
esac

host_os=${OXPINYIN_SANITIZER_HOST_OS:-$(uname -s)}
host_arch=${OXPINYIN_SANITIZER_HOST_ARCH:-$(uname -m)}
case $sanitizer in
memory)
	if [ "$host_os" = Darwin ]; then
		printf '%s\n' "sanitize: MemorySanitizer is unsupported on macOS: rustc has no Darwin MSan target." >&2
	else
		printf '%s\n' "sanitize: MemorySanitizer has no in-tree wrapper; use address, thread or leak." >&2
	fi
	exit 1
	;;
leak)
	if [ "$backend" = tkrzw ] || [ "$backend" = all ]; then
		printf '%s\n' "sanitize: tkrzw LeakSanitizer is unsupported: the Rust harness keeps process-lifetime allocations that LSan reports as leaks; use address or thread." >&2
		exit 1
	fi
	if [ "$host_os" = Darwin ] && [ "$host_arch" = arm64 ]; then
		printf '%s\n' "sanitize: standalone LeakSanitizer is unsupported on macOS arm64; use address (leak detection off on Darwin) or an x86_64 host." >&2
		exit 1
	fi
	;;
esac

export OXPINYIN_SANITIZER=$sanitizer
if [ "$check" = 1 ]; then
	printf 'sanitize: check backend=%s sanitizer=%s host=%s/%s\n' "$backend" "$sanitizer" "$host_os" "$host_arch"
	exit 0
fi

case $backend in
bdb | kc | tkrzw) exec bash "$here/$backend/run-sanitizers.sh" ;;
all) ;;
esac

results=()
failed=0
for item in bdb kc tkrzw; do
	printf '%s\n' "=== sanitize $item ==="
	if bash "$here/$item/run-sanitizers.sh"; then
		results+=("$item: exit 0")
	else
		code=$?
		results+=("$item: exit $code")
		failed=1
	fi
	printf '\n'
done
printf 'sanitize all: %s\n' "${results[@]}"
exit "$failed"
