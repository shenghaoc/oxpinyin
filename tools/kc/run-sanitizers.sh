#!/usr/bin/env bash
# run-sanitizers.sh — the Kyoto Cabinet backend's sanitizer gate.
#
# The hazard this backend has is ownership, not lifetime. Kyoto Cabinet's
# kcdbget, kccurgetkey, kccurgetvalue and kccurget each return a
# caller-owned region that must be released with kcfree (kclangc.h:577,
# :923, :942) -- so the failure modes are a leak, a double free, or a
# free through the wrong allocator, and all three are exactly what ASan
# and its LeakSanitizer see. LeakSanitizer runs with ASan on Linux; on
# Darwin leak detection is off, and standalone LSan is refused on arm64.
# ThreadSanitizer runs the same Rust tests with thread instrumentation.
#
# rustc has no `undefined` sanitizer -- `-Zsanitizer=` accepts address,
# cfi, dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress, leak,
# memory, memtag, safestack, shadow-call-stack, thread and realtime, and
# nothing else. UBSan is a C/C++ instrumentation and cannot be applied to
# Rust code; this backend has no C harness here.
#
# NOT covered: libkyotocabinet's own internals. The system library is
# not rebuilt, so UB or a leak inside it is invisible here.
#
# Usage: tools/kc/run-sanitizers.sh
# Exit: 0 clean; non-zero on a sanitizer finding or a build failure.

set -euo pipefail
cd "$(dirname "$0")/../.."

# Named once, in tools/sanitize.sh.
NIGHTLY=${OXPINYIN_SANITIZER_TOOLCHAIN:-$(sed -n 's/^NIGHTLY=//p' tools/sanitize.sh)}
TARGET=${OXPINYIN_SANITIZER_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
SANITIZER=${OXPINYIN_SANITIZER:-address}
case $SANITIZER in
address)
	RUST_SANITIZER_FLAGS="-Zsanitizer=address"
	;;
thread)
	RUST_SANITIZER_FLAGS="-Zsanitizer=thread -Cunsafe-allow-abi-mismatch=sanitizer"
	;;
leak)
	RUST_SANITIZER_FLAGS="-Zsanitizer=leak -Cunsafe-allow-abi-mismatch=sanitizer"
	;;
*)
	printf 'unknown sanitizer: %s\n' "$SANITIZER" >&2
	exit 2
	;;
esac
case $SANITIZER in
thread)
	# On Darwin, Kyoto Cabinet holds more locks than the TSan deadlock
	# detector tracks (128). Turn off only the auxiliary detector there and
	# keep race reports. Linux keeps the TSan default; it was not run here.
	if [ "$(uname -s)" = Darwin ]; then
		export TSAN_OPTIONS=${TSAN_OPTIONS:-detect_deadlocks=0:abort_on_error=1}
	else
		export TSAN_OPTIONS=${TSAN_OPTIONS:-abort_on_error=1}
	fi
	;;
leak) export LSAN_OPTIONS=${LSAN_OPTIONS:-abort_on_error=1} ;;
esac
if [ "$(uname -s)" = Darwin ] && [ "$SANITIZER" = address ]; then
	export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=0:abort_on_error=1}
fi

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

case $SANITIZER in
address) display=AddressSanitizer ;;
thread) display=ThreadSanitizer ;;
leak) display=LeakSanitizer ;;
esac
if [ "$SANITIZER" = address ]; then
	if [ "$(uname -s)" = Darwin ]; then
		echo "--- Rust under $display (LeakSanitizer off on Darwin) ---"
	else
		echo "--- Rust under $display (+ LeakSanitizer) ---"
	fi
else
	echo "--- Rust under $display ---"
fi
RUSTFLAGS="$RUST_SANITIZER_FLAGS" \
	cargo "+$NIGHTLY" test -p oxpinyin-store --no-default-features --features kyotocabinet --target "$TARGET"

echo
echo "sanitizers: clean (libkyotocabinet's own internals are NOT instrumented)"
