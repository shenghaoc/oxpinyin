#!/usr/bin/env bash
# run-sanitizers.sh — the Berkeley DB backend's sanitizer gate.
#
# Load-bearing rather than precautionary: bigram.db is on the SYSTEM data
# path, so every session that takes the compat path opens a 25.9 MB
# Berkeley DB file through this FFI, not only users who have trained.
#
# Two halves, because they cover different code:
#
#   1. Rust under AddressSanitizer. Covers this backend's own FFI — the
#      DBT plumbing, the cursor borrows, the chunk decode — plus, through
#      ASan's malloc interposition, heap misuse of the memory libdb hands
#      back. LeakSanitizer runs with it by default on Linux.
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
#      and are the failure modes worth catching. Rust's own decode indexes
#      byte by byte through `from_ne_bytes`, so it cannot misalign.
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

NIGHTLY=${OXPINYIN_SANITIZER_TOOLCHAIN:-nightly-2026-08-01}
TARGET=${OXPINYIN_SANITIZER_TARGET:-x86_64-unknown-linux-gnu}

if ! rustup toolchain list | grep -q "^$NIGHTLY"; then
	echo "SKIP: $NIGHTLY is not installed."
	echo "  install it with: rustup toolchain install $NIGHTLY --profile minimal"
	echo "  NOTE: the Rust half of this gate did NOT run."
	exit 0
fi

echo "--- 1/2: Rust under AddressSanitizer (+ LeakSanitizer) ---"
RUSTFLAGS="-Zsanitizer=address" \
	cargo "+$NIGHTLY" test -p oxpinyin-store --features bdb --target "$TARGET"

echo
echo "--- 2/2: the libdb call pattern under ASan + UBSan (C) ---"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
SAN_FLAGS=(-fsanitize=address,undefined -fno-sanitize-recover=all)

gcc -Wall -Wextra -Werror -O1 -g "${SAN_FLAGS[@]}" \
	-o "$work/hash-walk" tools/bdb/hash-walk.c -ldb
gcc -Wall -Wextra -Werror -O1 -g "${SAN_FLAGS[@]}" \
	-o "$work/btree-order" tools/bdb/btree-order.c -ldb

data_dir=${OXPINYIN_LIBPINYIN_DATA_DIR:-}
if [ -z "$data_dir" ]; then
	for candidate in \
		/usr/lib/x86_64-linux-gnu/libpinyin/data \
		/usr/lib64/libpinyin/data \
		/usr/lib/libpinyin/data \
		/usr/local/lib/libpinyin/data; do
		[ -f "$candidate/bigram.db" ] && data_dir=$candidate && break
	done
fi

if [ -n "$data_dir" ]; then
	echo "walking $data_dir/bigram.db"
	"$work/hash-walk" "$data_dir/bigram.db"
else
	echo "SKIP: no installed libpinyin bigram.db, so the DB_HASH walk did NOT run."
	echo "  Install libpinyin-data or set OXPINYIN_LIBPINYIN_DATA_DIR."
fi

echo
echo "exercising DB_BTREE create/put/cursor in $work"
"$work/btree-order" "$work/btree-order.db" >/dev/null

echo
echo "sanitizers: clean (libdb's own internals are NOT instrumented)"
