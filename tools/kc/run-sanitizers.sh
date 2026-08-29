#!/usr/bin/env bash
# run-sanitizers.sh — the Kyoto Cabinet backend's sanitizer gate.
#
# Load-bearing rather than precautionary: bigram.db is on the SYSTEM data
# path, so every session that takes the compat path opens a multi-megabyte
# database through this FFI, not only users who have trained.
#
# The hazard this backend actually has is ownership, not lifetime. Kyoto
# Cabinet's kcdbget, kccurgetkey, kccurgetvalue and kccurget each return a
# caller-owned region that must be released with kcfree (kclangc.h:577,
# :923, :942) -- so the failure modes are a leak, a double free, or a free
# through the wrong allocator, and all three are exactly what ASan and its
# LeakSanitizer see. That is why this gate matters more here than a
# lifetime-checked backend would need.
#
# rustc has no `undefined` sanitizer -- `-Zsanitizer=` accepts address,
# cfi, dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress, leak,
# memory, memtag, safestack, shadow-call-stack, thread and realtime, and
# nothing else. UBSan is a C/C++ instrumentation and cannot be applied to
# Rust code.
#
# NOT covered: libkyotocabinet's own internals. The system library is not
# rebuilt, so UB or a leak inside it is invisible here. Covering that would
# mean building Kyoto Cabinet 1.2.x with the sanitizers, a separate job.
#
# Usage: tools/kc/run-sanitizers.sh
# Exit: 0 clean; non-zero on a sanitizer finding or a build failure.

set -euo pipefail
cd "$(dirname "$0")/../.."

NIGHTLY=${OXPINYIN_SANITIZER_TOOLCHAIN:-nightly-2026-08-01}
TARGET=${OXPINYIN_SANITIZER_TARGET:-x86_64-unknown-linux-gnu}

if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
	echo "SKIP: $NIGHTLY is not installed."
	echo "  install it with: rustup toolchain install $NIGHTLY --profile minimal"
	echo "  NOTE: this gate did NOT run."
	exit 0
fi

# A real bigram.db makes the walk cover libpinyin's records rather than a
# handful of synthetic ones; without it the suite still runs, smaller.
if [ -z "${OXPINYIN_KC_BIGRAM:-}" ]; then
	for candidate in \
		/usr/lib/x86_64-linux-gnu/libpinyin/data/bigram.db \
		/usr/lib64/libpinyin/data/bigram.db \
		/usr/lib/libpinyin/data/bigram.db; do
		if [ -f "$candidate" ] &&
			[ "$(head -c 4 "$candidate" | od -An -tx1 | tr -d ' \n')" = "4b430a00" ]; then
			OXPINYIN_KC_BIGRAM=$candidate
			export OXPINYIN_KC_BIGRAM
			break
		fi
	done
fi
if [ -n "${OXPINYIN_KC_BIGRAM:-}" ]; then
	echo "walking $OXPINYIN_KC_BIGRAM under the sanitizer"
else
	echo "NOTE: no Kyoto Cabinet bigram.db; the compat tests will self-skip."
	echo "  tools/kc/run-compat-check.sh can produce one from installed data."
fi

echo "--- Rust under AddressSanitizer (+ LeakSanitizer) ---"
RUSTFLAGS="-Zsanitizer=address" \
	cargo "+$NIGHTLY" test -p oxpinyin-store --features kyotocabinet --target "$TARGET"

echo
echo "sanitizers: clean (libkyotocabinet's own internals are NOT instrumented)"
