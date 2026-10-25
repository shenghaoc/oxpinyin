#!/usr/bin/env bash
# run-sanitizers.sh — the Berkeley DB backend's sanitizer gate.
#
# Load-bearing rather than precautionary: on a BerkeleyDB distro the
# system bigram.db is BDB and every session opens it through this FFI,
# not only users who have trained.
#
# AddressSanitizer (default):
#
#   1. Rust under AddressSanitizer. Covers this backend's own FFI — the
#      DBT plumbing, the cursor buffers, the write-transaction apply —
#      plus, through ASan's malloc interposition, heap misuse of any
#      memory libdb hands back. LeakSanitizer runs with it on Linux; on
#      Darwin leak detection is off, and standalone LSan is refused on
#      arm64.
#
#   2. A C harness under -fsanitize=address,undefined, driving libdb with
#      the same call sequence and the same chunk arithmetic this backend
#      uses. This is where UBSan has something to instrument: misaligned
#      loads out of a DBT buffer, out-of-bounds indexing of a SingleGram
#      chunk, and overflow in the (size - 4) / 8 item count are UB in C
#      and are the failure modes worth catching. The harness walks a
#      bigram.db this backend itself wrote (via the bdb_write_profile
#      example), so the write direction is checked with no Rust in the
#      reader; when a real libpinyin data dir is present it is walked
#      too.
#
# ThreadSanitizer runs only the Rust half. The C harness is a
# single-threaded call-pattern probe, and libdb itself is not
# instrumented, so TSan has nothing to intersect there.
#
# rustc has no `undefined` sanitizer (`-Zsanitizer=` accepts address,
# cfi, dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress, leak,
# memory, memtag, safestack, shadow-call-stack, thread, realtime), so
# UBSan cannot be applied to Rust code at all. It is a C/C++
# instrumentation, run only in the address gate.
#
# What NEITHER half covers: libdb's own internals. The system library is
# not instrumented, so UB inside Berkeley DB is invisible to both. Saying
# so plainly matters more than the green result — covering it would mean
# rebuilding libdb 5.3.28 with the sanitizers, which is a separate job.
#
# Usage: tools/bdb/run-sanitizers.sh
# Exit: 0 clean; non-zero on a sanitizer finding or a build failure.

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail
cd "$(dirname "$0")/../.."

# Named once, in tools/sanitize.sh.
NIGHTLY=${OXPINYIN_SANITIZER_TOOLCHAIN:-$(sed -n 's/^NIGHTLY=//p' tools/sanitize.sh)}
TARGET=${OXPINYIN_SANITIZER_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}
SANITIZER=${OXPINYIN_SANITIZER:-address}
case $SANITIZER in
address)
	RUST_SANITIZER_FLAGS="-Zsanitizer=address -Cdebuginfo=1"
	;;
thread)
	RUST_SANITIZER_FLAGS="-Zsanitizer=thread -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer"
	;;
leak)
	RUST_SANITIZER_FLAGS="-Zsanitizer=leak -Cdebuginfo=1 -Cunsafe-allow-abi-mismatch=sanitizer"
	;;
*)
	printf 'unknown sanitizer: %s\n' "$SANITIZER" >&2
	exit 2
	;;
esac
case $SANITIZER in
thread) export TSAN_OPTIONS=${TSAN_OPTIONS:-abort_on_error=1} ;;
leak) export LSAN_OPTIONS=${LSAN_OPTIONS:-abort_on_error=1} ;;
esac
if [ "$(uname -s)" = Darwin ] && [ "$SANITIZER" = address ]; then
	export ASAN_OPTIONS=${ASAN_OPTIONS:-detect_leaks=0:abort_on_error=1}
fi

if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
	printf '%s is not installed.\n' "$NIGHTLY" >&2
	printf '  install it with: rustup toolchain install %s --profile minimal\n' "$NIGHTLY" >&2
	if [ -n "${OXPINYIN_SANITIZER_ALLOW_SKIP:-}" ]; then
		printf '%s\n' "  SKIP: OXPINYIN_SANITIZER_ALLOW_SKIP is set; the gate did NOT run." >&2
		exit 0
	fi
	printf '%s\n' "  ERROR: the sanitizer gate cannot run. Install the toolchain, or set" \
		"  OXPINYIN_SANITIZER_ALLOW_SKIP=1 to allow skipping it." >&2
	exit 1
fi

case $SANITIZER in
address) display=AddressSanitizer ;;
thread) display=ThreadSanitizer ;;
leak) display=LeakSanitizer ;;
esac
if [ "$SANITIZER" = address ]; then
	if [ "$(uname -s)" = Darwin ]; then
		printf '%s\n' "--- 1/2: Rust under $display (LeakSanitizer off on Darwin) ---"
	else
		printf '%s\n' "--- 1/2: Rust under $display (+ LeakSanitizer) ---"
	fi
else
	printf '%s\n' "--- 1/2: Rust under $display ---"
fi
RUSTFLAGS="$RUST_SANITIZER_FLAGS" \
	cargo "+$NIGHTLY" test -p oxpinyin-store --no-default-features --features bdb --target "$TARGET"

if [ "$SANITIZER" != address ]; then
	printf '%s\n' "" "sanitizers: clean (libdb's own internals are NOT instrumented)"
	exit 0
fi

printf '%s\n' ""
printf '%s\n' "--- 2/2: the libdb call pattern under ASan + UBSan (C) ---"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# shellcheck disable=SC2054  # the comma is part of the compiler flag, not an array separator.
SAN_FLAGS=(-fsanitize=address,undefined -fno-sanitize-recover=all)
compile=(gcc -Wall -Wextra -Werror -O1 -g "${SAN_FLAGS[@]}")
if [ -n "${OXPINYIN_BDB_INCLUDE_DIR:-}" ]; then
	compile+=("-I$OXPINYIN_BDB_INCLUDE_DIR")
fi
if [ -n "${OXPINYIN_BDB_LIB_DIR:-}" ]; then
	compile+=("-L$OXPINYIN_BDB_LIB_DIR")
fi

"${compile[@]}" -o "$work/hash-walk" tools/bdb/hash-walk.c -ldb
"${compile[@]}" -o "$work/btree-order" tools/bdb/btree-order.c -ldb

# The write direction: a file this backend created, checked by a reader
# with no Rust in it.
cargo "+$NIGHTLY" build -p oxpinyin-store --no-default-features --features bdb \
	--target "$TARGET" --example bdb_write_profile
example="${CARGO_TARGET_DIR:-target}/$TARGET/debug/examples/bdb_write_profile"
"$example" "$work/user_bigram.db"
printf '%s\n' "walking the backend-written $work/user_bigram.db"
"$work/hash-walk" "$work/user_bigram.db"

data_dir=${OXPINYIN_LIBPINYIN_DATA_DIR:-}
if [ -z "$data_dir" ]; then
	for candidate in \
		/usr/lib/x86_64-linux-gnu/libpinyin/data \
		/usr/lib/aarch64-linux-gnu/libpinyin/data \
		/usr/lib64/libpinyin/data \
		/usr/lib/libpinyin/data \
		/usr/local/lib/libpinyin/data; do
		[ -f "$candidate/bigram.db" ] && data_dir=$candidate && break
	done
fi
if [ -n "$data_dir" ]; then
	printf '%s\n' "walking $data_dir/bigram.db"
	"$work/hash-walk" "$data_dir/bigram.db"
else
	printf '%s\n' "note: no installed libpinyin bigram.db; the real-file walk did not run."
fi

printf '%s\n' ""
printf '%s\n' "exercising DB_BTREE create/put/cursor in $work"
"$work/btree-order" "$work/btree-order.db" >/dev/null

printf '%s\n' ""
printf '%s\n' "sanitizers: clean (libdb's own internals are NOT instrumented)"
