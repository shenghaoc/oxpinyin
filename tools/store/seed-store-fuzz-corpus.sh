#!/usr/bin/env bash
# seed-store-fuzz-corpus.sh — seed the `store-open` fuzz corpus with the
# committed store files for one backend.
#
# `store-open` treats its input as a DBM file verbatim, so a corpus entry
# is a database file with no framing. Random bytes are refused by every
# backend's header check, which means an unseeded run explores only the
# open-time rejection path; seeding from `fixtures/w3/<dir>/` gives
# libFuzzer valid containers to mutate and is what reaches the record
# decoders behind the header.
#
# Seeding is deliberately a separate step, not a CI step. `store-open`
# itself does run in CI — store-backends.yml's store-file-fuzz job gates
# on it under the instrumented LMDB peer, and ci.yml's fuzz smoke and the
# nightly soak both pick it up from `cargo fuzz list` — but every one of
# those runs it UNSEEDED, which is the open-time rejection path only.
#
# What these seeds add is the reason no lane runs them: from the
# committed fixtures alone, a seeded pass faults Kyoto Cabinet and LMDB
# inside 120 s, in the container library rather than in oxpinyin code.
# redb and tkrzw survive these particular seeds — the mini fixtures are
# mostly free space, so a mutation rarely lands on a live page — and fall
# instead to a denser generated container; they are not exempt, just not
# reachable from here. `docs/findings/store-file-ingress-fuzzing.md`
# carries the measurements and the maintainer decision that is open on
# them. Seed when you are hunting; do not wire this into a lane before
# that decision lands.
#
# Usage:  tools/store/seed-store-fuzz-corpus.sh <kyotocabinet|redb|lmdb|tkrzw>
#
# Exit: 0 when at least one seed was copied; non-zero on a bad backend
# name or a missing fixture directory.

set -euo pipefail
cd "$(dirname "$0")/../.."

backend="${1-}"

# The fixture directory and the store-file names differ per backend:
# Kyoto Cabinet and tkrzw are the two containers libpinyin itself builds
# against, so their fixture directories carry libpinyin's own file names
# (`*.bin`, `bigram.db`); redb and LMDB are oxpinyin-only containers
# under their own extensions. The `*.bin` files at the top of
# `fixtures/w3/` are chunk files, not store files, and are not seeds here.
case "$backend" in
kyotocabinet) dir=kct; files=(addon_pinyin_index.bin addon_phrase_index.bin bigram.db) ;;
redb) dir=redb; files=(addon_pinyin_index.redb addon_phrase_index.redb bigram.redb) ;;
lmdb) dir=lmdb; files=(addon_pinyin_index.lmdb addon_phrase_index.lmdb bigram.lmdb) ;;
tkrzw) dir=tkt; files=(addon_pinyin_index.bin addon_phrase_index.bin bigram.db) ;;
*)
    echo "usage: $0 <kyotocabinet|redb|lmdb|tkrzw>" >&2
    exit 2
    ;;
esac

src="fixtures/w3/$dir"
dst="fuzz/corpus/store-open"

if [ ! -d "$src" ]; then
    echo "FAIL: $src is missing; regenerate the fixtures (docs/runbooks/backends.md)" >&2
    exit 1
fi

mkdir -p "$dst"

copied=0
for file in "${files[@]}"; do
    if [ -f "$src/$file" ]; then
        # Named by backend so seeds for two backends can coexist in one
        # corpus directory without one overwriting the other.
        cp "$src/$file" "$dst/seed-$dir-$file"
        copied=$((copied + 1))
    else
        echo "::warning::$src/$file is missing; skipped"
    fi
done

if [ "$copied" -eq 0 ]; then
    echo "FAIL: no seeds copied from $src" >&2
    exit 1
fi

echo "seeded $copied $backend store file(s) into $dst"
