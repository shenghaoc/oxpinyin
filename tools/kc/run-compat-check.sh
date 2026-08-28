#!/usr/bin/env bash
# run-compat-check.sh — the Kyoto Cabinet backend's compatibility gate.
#
# Reads real libpinyin bigram records through the Kyoto Cabinet backend
# and checks that every one of them re-encodes to the bytes on disk. That
# round-trip is the write gate: a drop-in that trains a user's profile
# writes back into libpinyin's files, and bytes that differ from what
# libpinyin would have written corrupt the profile with no error anywhere.
#
# WHERE THE DATA COMES FROM, AND HOW STRONG THAT MAKES THIS
#
#   1. A Kyoto-Cabinet-built libpinyin is installed. The tests read its
#      own bigram.db, and this is a genuine compatibility check.
#
#   2. Otherwise, and this is the weaker case: the installed libpinyin was
#      built against another DBM, so tools/kc/bdb-to-kc.c transcribes its
#      records into a Kyoto Cabinet container -- real keys, real
#      SingleGram chunks, no Rust in the transcription -- and the tests
#      read that. It proves this backend reads libpinyin's RECORDS out of
#      a Kyoto Cabinet file. It does NOT prove a Kyoto-Cabinet-built
#      libpinyin would have written that file. Only a machine with one
#      installed can show that, and this script says which case it took.
#
# What licenses case 2 is that the chunk and the key are
# backend-independent: ngram.cpp is unconditional in
# src/storage/Makefile.am:72, while ngram_bdb.cpp and ngram_kyotodb.cpp
# are added under `if BERKELEYDB` / `if KYOTOCABINET`.
#
# Usage: tools/kc/run-compat-check.sh
# Exit: 0 clean or skipped; non-zero on a failure.

set -euo pipefail
cd "$(dirname "$0")/../.."

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

if [ -z "$data_dir" ]; then
	echo "SKIP: no installed libpinyin data directory."
	echo "  Install the distro's data package (Debian/Ubuntu: libpinyin-data) or"
	echo "  set OXPINYIN_LIBPINYIN_DATA_DIR."
	echo "  NOTE: the compatibility gate did NOT run. Only the backend's own"
	echo "  self-consistency tests would have; they say nothing about libpinyin."
	exit 0
fi

bigram=$data_dir/bigram.db
# libpinyin names this file bigram.db whichever DBM it was built against,
# so the format is read from the magic, never from the name.
magic=$(head -c 4 "$bigram" | od -An -tx1 | tr -d ' \n')
if [ "$magic" = "4b430a00" ]; then
	echo "$bigram is already Kyoto Cabinet — reading it directly."
	echo "This is the strong case: a file a Kyoto-Cabinet-built libpinyin wrote."
	kc_bigram=$bigram
else
	echo "$bigram is not Kyoto Cabinet (magic $magic)."
	echo "Transcribing its records into a Kyoto Cabinet container."
	echo "WEAKER CASE: this checks that the backend reads libpinyin's records,"
	echo "not that a Kyoto-Cabinet-built libpinyin wrote this exact file."
	work=$(mktemp -d)
	trap 'rm -rf "$work"' EXIT
	gcc -Wall -Wextra -Werror -O2 -o "$work/bdb-to-kc" tools/kc/bdb-to-kc.c \
		-ldb -lkyotocabinet
	"$work/bdb-to-kc" "$bigram" "$work/bigram.db"
	kc_bigram=$work/bigram.db
fi

echo
echo "--- reading it through the Kyoto Cabinet backend ---"
OXPINYIN_KC_BIGRAM=$kc_bigram OXPINYIN_KC_STRICT=1 \
	cargo test --locked -p oxpinyin-store --features kyotocabinet \
	--test kc_libpinyin_files -- --nocapture
