#!/usr/bin/env bash
# Not `set -e`: a gated differential that diverges (e.g. ngseg when the system
# bigram is unavailable) should be reported, not abort the whole run.
set -uo pipefail

# run-differentials.sh — run the oxpinyin↔libpinyin oracle differentials
# against a built pinned oracle (libpinyin 2.11.91, Tkrzw backend, verified
# model20 data). Wires the PINYIN_* env vars the env-gated differential tests
# read, then runs them with the pure-Rust `redb` backend so no C DBM is needed
# on the Rust side.
#
# Prerequisites (see docs/testing/oracle-environment.md and the build recipe
# below):
#   1. A built libpinyin 2.11.91 tree (Tkrzw backend). Its utils and shared
#      object are used directly from the build tree — no `make install`.
#   2. A built system data dir: the model20 tables + table.conf plus the
#      compiled phrase_index.bin / pinyin_index.bin (gen_binary_files +
#      import_interpolation). The KMM/segment utils load these from the cwd.
#   3. The oxpinyin-format model export (redb), produced by:
#        oxpinyin-datagen compile --backend redb \
#            --model-dir <model20 dir> --out-dir <export dir>
#
# Usage:
#   run-differentials.sh --libpinyin DIR --data DIR --export DIR [--model DIR]
#
# The build recipe used to produce (1)+(2) from the pinned source (when the
# release tarball is reachable, tools/oracle/build-oracle.sh is canonical; in a
# network-restricted host build from a local checkout of the pinned commit):
#   cd libpinyin && ./autogen.sh && \
#     ./configure --with-dbm=Tkrzw --prefix=$PWD/../prefix && make -j
#   cd data && cp <model20>/*.table <model20>/interpolation2.text . && \
#     LD_LIBRARY_PATH=../src/.libs ../utils/storage/gen_binary_files \
#         --gen-punct-table --table-dir . && \
#     LD_LIBRARY_PATH=../src/.libs ../utils/storage/import_interpolation \
#         --table-dir . < interpolation2.text

libpinyin=
data=
export_dir=
model=

while (($#)); do
	case $1 in
	--libpinyin) libpinyin=$2; shift 2 ;;
	--data) data=$2; shift 2 ;;
	--export) export_dir=$2; shift 2 ;;
	--model) model=$2; shift 2 ;;
	-h | --help)
		grep '^#' "$0" | sed 's/^# \{0,1\}//'
		exit 0
		;;
	*) printf 'unknown option: %s\n' "$1" >&2; exit 2 ;;
	esac
done

for v in libpinyin data export_dir; do
	if [[ -z ${!v} ]]; then
		printf 'missing --%s\n' "${v/_dir/}" >&2
		exit 2
	fi
done

L=$(cd "$libpinyin" && pwd)
export LD_LIBRARY_PATH="$L/src/.libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# The built system data dir (cwd for KMM/segment utils) and the oxpinyin export.
export PINYIN_GEN_NGRAM_DATA="$data"
export PINYIN_NGSEG_DATA="$data"
export PINYIN_EXPORT_DIR="$export_dir"
[[ -n $model ]] && export PINYIN_MODEL_DIR="$model"

# Segment utils.
export PINYIN_SPSEG="$L/utils/segment/spseg"
export PINYIN_MERGESEQ="$L/utils/segment/mergeseq"
export PINYIN_NGSEG="$L/utils/segment/ngseg"

# KMM utils.
export PINYIN_GEN_KMM="$L/utils/training/gen_k_mixture_model"
export PINYIN_EXPORT_KMM="$L/utils/training/export_k_mixture_model"
export PINYIN_MERGE_KMM="$L/utils/training/merge_k_mixture_model"
export PINYIN_PRUNE_KMM="$L/utils/training/prune_k_mixture_model"
export PINYIN_VALIDATE_KMM="$L/utils/training/validate_k_mixture_model"
export PINYIN_KMM_TO_INTERP="$L/utils/training/k_mixture_model_to_interpolation"

# Legacy counting / interpolation utils (lambda + counter differentials).
export PINYIN_GEN_BINARY_FILES="$L/utils/storage/gen_binary_files"
export PINYIN_GEN_UNIGRAM="$L/utils/training/gen_unigram"
export PINYIN_GEN_NGRAM="$L/utils/training/gen_ngram"
export PINYIN_GEN_DELETED_NGRAM="$L/utils/training/gen_deleted_ngram"
export PINYIN_ESTIMATE_INTERPOLATION="$L/utils/training/estimate_interpolation"
export PINYIN_EXPORT_INTERPOLATION="$L/utils/storage/export_interpolation"

# eval_correction_rate: needs the compiled system bigram.db + an evals2.text
# corpus; wired here for completeness, gated in the test.
export PINYIN_EVAL_CORRECTION_RATE="$L/utils/training/eval_correction_rate"

# The backend-forwarding crates run with the pure-Rust redb backend; oxpinyin-kmm
# is backend-agnostic (no features).
feat="--no-default-features --features redb"
report() { grep -E "live parity|parity:|value-identical|skipping|test result|diverges|stale" || true; }

echo "== KMM differentials (gen+export, to-interpolation, merge, prune, validate) =="
cargo test -p oxpinyin-kmm --test differential -- --nocapture 2>&1 | report
echo "== segment: spseg / mergeseq =="
cargo test -p oxpinyin-segment $feat --test spseg_mergeseq -- --nocapture 2>&1 | report
echo "== segment: ngseg (needs the system bigram) =="
cargo test -p oxpinyin-segment $feat --test differential -- --nocapture 2>&1 | report
echo "== lambda: estimate_interpolation =="
cargo test -p oxpinyin-lambda $feat --test differential -- --nocapture 2>&1 | report
echo "== counter: gen_ngram =="
cargo test -p oxpinyin-counter $feat --test differential -- --nocapture 2>&1 | report
