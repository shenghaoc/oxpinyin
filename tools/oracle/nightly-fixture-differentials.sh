#!/usr/bin/env bash
# nightly-fixture-differentials.sh — the CI tier of the oxpinyin↔libpinyin
# differentials: everything the pinned libpinyin source plus COMMITTED
# fixtures can prove, with no model20 download and no developer machine.
#
# The store-backends.yml policy holds: no CI job may download
# model20.text.tar.gz. Substituting committed fixtures is therefore the
# only way any differential can run in CI, and the committed corpora pin
# what that substitution can carry. Each gate below runs in the wiring it
# was verified green in (Debian container, 2026-09-08); every excluded
# suite is excluded by mechanism, not judgment:
#
#   GREEN, toned tier (pin-built data from fixtures/datagen-toned):
#     - segment ngseg live parity (pin ngseg vs the Rust segmenter over
#       the same raw text; bit-identical token sequences).
#   GREEN, w3 tier (committed fixtures/w3/{tkt,redb}; the KMM/spseg
#   corpora live in the w3 token space, so the tables must be w3's):
#     - KMM gen+export, to-interpolation, merge, validate.
#     - spseg live parity.
#   GREEN, drop-in tier: tools/datagen/libpinyin-drop-in-differential.sh
#     tkrzw over the toned model — byte-exact chunk parity, the script's
#     own documented container use case.
#   EXCLUDED, with the reason the container run observed:
#     - KMM prune: the pin's gen_k_mixture_model SIGSEGVs ("unknown
#       token") on that gate's fixture input under w3 tables — an
#       upstream tool crash on out-of-space tokens, not an oxpinyin
#       divergence to chase here.
#     - mergeseq live: the pin emits a token outside the committed w3
#       phrase table, which our parser rejects as malformed.
#     - lambda / counter live + every committed golden and manifest:
#       their corpora (segmenter-ngseg.txt, the λ/counter manifests) are
#       model20-token-space goldens, and counter's live chain needs the
#       .table sources only a model dir carries. A toned/w3-space corpus
#       would be a NEW golden — policy work, not wiring.
#     - eval: needs an interpolation2.text consistent with the tables
#       under test; none is committed for w3, and on toned data the pin
#       reports 0 tested items (its NaN vs our 0.000000 at 0/0 is a real
#       formatting divergence worth its own small fix, not a lane).
#
# The model20 differentials remain the developer-machine workflow
# (docs/testing/oracle-environment.md); this lane is the nightly drift
# alarm over what committed data can carry.
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
	# A depth-1 fetch of a bare sha lands in FETCH_HEAD; HEAD is still
	# unborn in this fresh repo, so the pin check reads FETCH_HEAD.
	got=$(git -C "$src" rev-parse FETCH_HEAD)
	if [[ $got != "$pin_sha" ]]; then
		echo "pin mismatch: fetched $got, wanted $pin_sha" >&2
		exit 2
	fi
	git -C "$src" checkout --quiet --detach FETCH_HEAD
	touch "$src/.pin-ok"
fi

# Seed the build's data dir with the committed toned model so `make
# install` compiles and installs a data dir every toned-tier gate below
# works in (the build-oracle.sh recipe, model cache swapped for the
# fixture).
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
for f in table.conf phrase_index.bin pinyin_index.bin bigram.db; do
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

# ── 3. the gates ────────────────────────────────────────────────────────────
# `make install` installs only the three data-build utils; every other
# tool comes from the build tree, the same paths run-differentials.sh
# wires for the developer runs.
L=$src
feat=(--no-default-features --features redb)

status=0
run_suite() {
	local title=$1
	shift
	echo "== $title =="
	if ! "$@"; then
		status=1
		echo "FAILED: $title"
	fi
}

# Toned tier: ngseg live parity over raw han text — the corpus-free gate,
# so any consistent table set carries it. The Rust side resolves its
# tables through BOTH the export and the model dir (the segment tests'
# locate_model_dir), so both point at the toned model.
export PINYIN_NGSEG="$L/utils/segment/ngseg"
export PINYIN_NGSEG_DATA="$data"
export PINYIN_EXPORT_DIR="$export_dir"
export PINYIN_MODEL_DIR="$model"
run_suite "segment ngseg live parity (toned tables)" \
	cargo test --locked -q -p oxpinyin-segment "${feat[@]}" --test differential -- \
	--include-ignored rust_matches_live_ngseg

# w3 tier: the KMM and spseg corpora are w3-token-space, so the tables
# and the Rust-side export are the committed w3 pair (tkt for the pin —
# it is the Tkrzw container format this build opens; redb for us).
w3=$repo/fixtures/w3
export PINYIN_GEN_NGRAM_DATA="$w3/tkt"
export PINYIN_EXPORT_DIR="$w3/redb"
export PINYIN_SPSEG="$L/utils/segment/spseg"
export PINYIN_NGSEG_DATA="$w3/tkt"
export PINYIN_GEN_KMM="$L/utils/training/gen_k_mixture_model"
export PINYIN_EXPORT_KMM="$L/utils/training/export_k_mixture_model"
export PINYIN_MERGE_KMM="$L/utils/training/merge_k_mixture_model"
export PINYIN_VALIDATE_KMM="$L/utils/training/validate_k_mixture_model"
export PINYIN_KMM_TO_INTERP="$L/utils/training/k_mixture_model_to_interpolation"
for gate in gen_and_export to_interpolation merge validate; do
	run_suite "KMM live: $gate (w3 tables)" \
		cargo test --locked -q -p oxpinyin-kmm --test differential -- \
		--include-ignored "rust_kmm_matches_pin_$gate"
done
run_suite "segment spseg live parity (w3 tables)" \
	cargo test --locked -q -p oxpinyin-segment "${feat[@]}" --test spseg_mergeseq -- \
	--include-ignored rust_matches_live_spseg

# Drop-in tier: the flagship — oxpinyin-datagen's output against the
# pin's own writers, byte for byte, exactly as a developer runs it.
unset PINYIN_EXPORT_DIR PINYIN_NGSEG_DATA PINYIN_GEN_NGRAM_DATA
run_suite "drop-in differential (tkrzw, toned model)" \
	tools/datagen/libpinyin-drop-in-differential.sh tkrzw "$model" "$work/drop-in"

if ((status)); then
	echo "RESULT: at least one differential suite FAILED"
	exit 1
fi
echo "fixture-tier differentials: OK"
