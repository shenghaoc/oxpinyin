#!/usr/bin/env bash
# run-sanitizers.sh — the Kyoto Cabinet backend's sanitizer gate.
#
# The hazard this backend has is ownership, not lifetime. Kyoto Cabinet's
# kcdbget, kccurgetkey, kccurgetvalue and kccurget each return a
# caller-owned region that must be released with kcfree (kclangc.h:577,
# :923, :942) -- so the failure modes are a leak, a double free, or a
# free through the wrong allocator, and all three are exactly what ASan
# and its LeakSanitizer see.
#
# rustc has no `undefined` sanitizer -- `-Zsanitizer=` accepts address,
# cfi, dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress, leak,
# memory, memtag, safestack, shadow-call-stack, thread and realtime, and
# nothing else. UBSan is a C/C++ instrumentation and cannot be applied to
# Rust code.
#
# NOT covered: libkyotocabinet's own internals. The system library is
# not rebuilt, so UB or a leak inside it is invisible here.
#
# Usage: tools/kc/run-sanitizers.sh
# Exit: 0 clean; non-zero on a sanitizer finding or a build failure.

set -euo pipefail
cd "$(dirname "$0")/../.."

NIGHTLY=${OXPINYIN_SANITIZER_TOOLCHAIN:-nightly-2026-08-01}
TARGET=${OXPINYIN_SANITIZER_TARGET:-x86_64-unknown-linux-gnu}

if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
	echo "$NIGHTLY is not installed."
	echo "  install it with: rustup toolchain install $NIGHTLY --profile minimal"
	if [ -n "${OXPINYIN_SANITIZER_ALLOW_SKIP:-}" ]; then
		echo "  SKIP: OXPINYIN_SANITIZER_ALLOW_SKIP is set; the gate did NOT run."
		exit 0
	fi
	echo "  ERROR: the sanitizer gate cannot run. Install the toolchain, or set"
	echo "  OXPINYIN_SANITIZER_ALLOW_SKIP=1 to allow skipping it."
	exit 1
fi

echo "--- Rust under AddressSanitizer (+ LeakSanitizer) ---"
RUSTFLAGS="-Zsanitizer=address" \
	cargo "+$NIGHTLY" test -p oxpinyin-store --features kyotocabinet --target "$TARGET"

echo
echo "sanitizers: clean (libkyotocabinet's own internals are NOT instrumented)"
