#!/usr/bin/env bash
# run-sanitizers.sh — the Berkeley DB backend's sanitizer gate.
#
# Load-bearing rather than precautionary: on a BerkeleyDB distro the
# system bigram.db is BDB and every session opens it through this FFI,
# not only users who have trained.
#
# Two halves, because they cover different code:
#
#   1. Rust under AddressSanitizer. Covers this backend's own FFI — the
#      DBT plumbing, the cursor buffers, the write-transaction apply —
#      plus, through ASan's malloc interposition, heap misuse of any
#      memory libdb hands back. LeakSanitizer runs with it by default
#      on Linux.
#
#      rustc has no `undefined` sanitizer (`-Zsanitizer=` accepts address,
#      cfi, dataflow, hwaddress, kcfi, kernel-address, kernel-hwaddress,
#      leak, memory, memtag, safestack, shadow-call-stack, thread,
#      realtime), so UBSan cannot be applied to Rust code at all. It is a
#      C/C++ instrumentation.
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
# What NEITHER half covers: libdb's own internals. The system library is
# not instrumented, so UB inside Berkeley DB is invisible to both. Saying
# so plainly matters more than the green result — covering it would mean
# rebuilding libdb 5.3.28 with the sanitizers, which is a separate job.
#
# Usage: tools/bdb/run-sanitizers.sh
# Exit: 0 clean; non-zero on a sanitizer finding or a build failure.

set -euo pipefail
cd "$(dirname "$0")/../.."

NIGHTLY=${OXPINYIN_SANITIZER_TOOLCHAIN:-nightly-2026-09-07}
TARGET=${OXPINYIN_SANITIZER_TARGET:-$(rustc -vV | sed -n 's/^host: //p')}

if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
	printf 'ERROR: %s is not installed; sanitizer gate did NOT run.\n' "$NIGHTLY" >&2
	printf '  install it with: rustup toolchain install %s --profile minimal\n' "$NIGHTLY" >&2
	exit 1
fi

printf '%s\n' "--- 1/2: Rust under AddressSanitizer (+ LeakSanitizer) ---"
RUSTFLAGS="-Zsanitizer=address -Cdebuginfo=1" \
	cargo "+$NIGHTLY" test -p oxpinyin-store --no-default-features --features bdb --target "$TARGET"

printf '%s\n' ""
printf '%s\n' "--- 2/2: the libdb call pattern under ASan + UBSan (C) ---"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
SAN_FLAGS=(-fsanitize=address,undefined -fno-sanitize-recover=all)

gcc -Wall -Wextra -Werror -O1 -g "${SAN_FLAGS[@]}" \
	-o "$work/hash-walk" tools/bdb/hash-walk.c -ldb
gcc -Wall -Wextra -Werror -O1 -g "${SAN_FLAGS[@]}" \
	-o "$work/btree-order" tools/bdb/btree-order.c -ldb

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
