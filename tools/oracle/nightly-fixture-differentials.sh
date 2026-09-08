#!/usr/bin/env bash
# nightly-fixture-differentials.sh — the CI tier of the oxpinyin↔libpinyin
# differentials: everything the pinned libpinyin source plus the COMMITTED
# toned mini-model can prove, with no model20 download and no developer
# machine.
#
# The store-backends.yml policy holds: no CI job may download
# model20.text.tar.gz, so this lane substitutes fixtures/datagen-toned —
# the committed 18-file mini-model in the pinned model20 inventory — for
# the non-redistributable archive. Both differential scripts then run
# exactly as a developer runs them:
#
#   1. tools/oracle/run-differentials.sh       (training differentials)
#   2. tools/datagen/libpinyin-drop-in-differential.sh tkrzw …
#      (byte-exact datagen drop-in parity)
#
# What that substitution costs, recorded so nobody mistakes this lane for
# the model20 tier: every suite here compares the pin against oxpinyin
# over the SAME toned inputs, so parity (the property under test) holds,
# but the coverage is thin — the toned dictionary is ~28 phrases where
# model20 is 138,096 items, and the committed model20-space goldens
# (segmenter-ngseg.txt, the λ/counter manifests) exercise tokens the
# toned tables do not carry. The model20 differential remains the
# developer-machine workflow (docs/testing/oracle-environment.md). This
# lane is the drift alarm: it fires nightly against the pinned source, so
# an oxpinyin change that diverges from the pin on ANY data the two
# disagree about is caught in ≤24h instead of at the next manual run.
#
# The pin constants are read from tools/oracle/build-oracle.sh — the
# canonical home — so this script cannot drift from them.
#
# usage: nightly-fixture-differentials.sh [<work-dir>]
# env:   PREFIX (default /opt/libpinyin-tkrzw) — where the pin is
#        installed; the drop-in script expects exactly this path.
set -euo pipefail

repo=$(cd "$(dirname "$0")/../.." && pwd)
work=${1:-/tmp/oxpinyin-fixture-differentials}
prefix=${PREFIX:-/opt/libpinyin-tkrzw}

# The pin, from the canonical script (never duplicated here).
build_oracle=$repo/tools/oracle/build-oracle.sh
pin_version=$(sed -n 's/^LIBPINYIN_VERSION=//p' "$build_oracle")
pin_sha=$(sed -n 's/^LIBPINYIN_SHA=//p' "$build_oracle")
pin_url=$(sed -n 's/^LIBPINYIN_GIT_URL=//p' "$build_oracle")
if [[ -z $pin_version || -z $pin_sha || -z $pin_url ]]; then
	echo "could not read the libpinyin pin from $build_oracle" >&2
	exit 2
fi
echo "pin: libpinyin $pin_version @ $pin_sha"

model=$repo/fixtures/datagen-toned
[[ -f $model/interpolation2.text ]] || { echo "missing $model/interpolation2.text" >&2; exit 2; }

mkdir -p "$work"

# ── 1. fetch and build the pinned libpinyin (Tkrzw) ────────────────────────
src=$work/libpinyin
if [[ ! -f $src/.pin-ok ]]; then
	rm -rf "$src"
	git init -q "$src"
	git -C "$src" fetch --quiet --depth=1 "$pin_url" "$pin_sha"
	got=$(git -C "$src" rev-parse HEAD)
	if [[ $got != "$pin_sha" ]]; then
		echo "pin mismatch: fetched $got, wanted $pin_sha" >&2
		exit 2
	fi
	git -C "$src" checkout --quiet --detach FETCH_HEAD
	touch "$src/.pin-ok"
fi

# Seed the build's data dir with the committed toned model so `make
# install` compiles and installs a data dir in the same token space every
# suite below works in (the build-oracle.sh recipe, model cache swapped
# for the fixture).
cp "$model"/*.table "$model/interpolation2.text" "$src/data/"

cd "$src"
if [[ ! -x configure ]]; then
	autoreconf --force --install
fi
if [[ ! -f Makefile ]]; then
	./configure --prefix="$prefix" --disable-static --with-dbm=Tkrzw
fi
# import_interpolation has tripped its insert_freq assertion
# intermittently on full models (run-differentials.sh header); on a fresh
# partial bigram.db a re-run completed, so one retry after clearing it.
if ! make -j"$(nproc)" || ! make install; then
	echo "::warning::build or install failed once; retrying on a fresh bigram.db"
	rm -f data/bigram.db
	make install
fi

data=$prefix/lib/libpinyin/data
for f in table.conf phrase_index.bin pinyin_index.bin; do
	[[ -f $data/$f ]] || { echo "installed data dir is missing $f" >&2; exit 2; }
done
export LD_LIBRARY_PATH="$prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# ── 2. the oxpinyin redb export of the SAME model ─────────────────────────
export_dir=$work/export-redb
cd "$repo"
if [[ ! -f $export_dir/datagen-manifest.txt ]]; then
	cargo run --locked -q -p oxpinyin-datagen --no-default-features --features redb -- \
		compile --backend redb --model-dir "$model" --out-dir "$export_dir"
fi

# ── 3. evals2.text in the toned token space (the pin's own ngseg) ─────────
# The eval gate is skipped unless $data/evals2.text exists
# (run-differentials.sh); generate it the documented way — the pin's
# ngseg over raw text, run from the data dir it reads.
if [[ ! -f $data/evals2.text ]]; then
	(cd "$data" && "$prefix/bin/ngseg" "$repo/fixtures/w9/corpus-sample.txt" > evals2.text)
fi

# ── 4. both differential scripts, exactly as a developer runs them ─────────
tools/oracle/run-differentials.sh \
	--libpinyin "$src" --data "$data" --export "$export_dir" --model "$model"

tools/datagen/libpinyin-drop-in-differential.sh tkrzw "$model" "$work/drop-in"

echo "fixture-tier differentials: OK"
