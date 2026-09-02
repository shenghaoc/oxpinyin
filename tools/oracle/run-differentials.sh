#!/usr/bin/env bash
# Not `set -e`: a gated differential that diverges (e.g. ngseg when the system
# bigram is unavailable) should be reported, not abort the whole run. Every
# suite's exit status is accumulated instead, and the script exits non-zero
# when any suite failed, so a later passing suite never masks an earlier
# failure.
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
#      compiled phrase_index.bin / pinyin_index.bin, built by the three
#      steps of libpinyin's data/Makefile.am — gen_binary_files,
#      import_interpolation, then gen_unigram (the freq-1 floor; without it
#      every zero-count phrase is unreachable and the pin's ngseg diverges
#      from the committed golden). The KMM/segment utils load these from
#      the cwd.
#      The two bigram-backed gates (ngseg-live, eval_correction_rate) also
#      need the compiled bigram.db there, and the evaluator gate needs an
#      evals2.text (a segmented, null_token-separated corpus in the system
#      token space — e.g. the pin's own `ngseg` over raw text) in that dir;
#      without them the evaluator gate is reported as skipped, not failed.
#   3. The oxpinyin-format model export (redb), produced by:
#        oxpinyin-datagen compile --backend redb \
#            --model-dir <model20 dir> --out-dir <export dir>
#
# Usage:
#   run-differentials.sh --libpinyin DIR --data DIR --export DIR [--model DIR]
#
# Every path is validated before a single gate runs: a typo would otherwise
# make the env-gated tests print `skipping` and the run look green.
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
#         --table-dir . < interpolation2.text && \
#     LD_LIBRARY_PATH=../src/.libs ../utils/training/gen_unigram --table-dir .
# `import_interpolation` has been observed to trip its `insert_freq`
# assertion intermittently on the full model20 (in both a Tkrzw and a Kyoto
# Cabinet build of the pin); a re-run on a fresh bigram.db completed. Delete
# a partial bigram.db before retrying.

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

# ---- prerequisite validation ------------------------------------------------
missing=0
need_dir() {
	[[ -d $1 ]] || { printf 'missing directory: %s (%s)\n' "$1" "$2" >&2; missing=1; }
}
need_file() {
	[[ -f $1 ]] || { printf 'missing file: %s (%s)\n' "$1" "$2" >&2; missing=1; }
}
need_exe() {
	[[ -x $1 ]] || { printf 'missing executable: %s (%s)\n' "$1" "$2" >&2; missing=1; }
}

need_dir "$libpinyin" "--libpinyin: built libpinyin 2.11.91 tree"
need_dir "$data" "--data: built system data dir"
need_dir "$export_dir" "--export: oxpinyin redb export"
[[ -n $model ]] && need_dir "$model" "--model: extracted model20 dir"
((missing)) && exit 2

L=$(cd "$libpinyin" && pwd)
data=$(cd "$data" && pwd)
export_dir=$(cd "$export_dir" && pwd)
[[ -n $model ]] && model=$(cd "$model" && pwd)

need_file "$L/src/.libs/libpinyin.so.15" "the built shared object (make)"
need_file "$data/table.conf" "system table.conf"
need_file "$data/phrase_index.bin" "gen_binary_files output"
need_file "$data/pinyin_index.bin" "gen_binary_files output"
need_file "$export_dir/datagen-manifest.txt" "oxpinyin-datagen compile --backend redb"
[[ -n $model ]] && need_file "$model/interpolation2.text" "model20 export"

# Segment utils.
PINYIN_SPSEG="$L/utils/segment/spseg"
PINYIN_MERGESEQ="$L/utils/segment/mergeseq"
PINYIN_NGSEG="$L/utils/segment/ngseg"
# KMM utils.
PINYIN_GEN_KMM="$L/utils/training/gen_k_mixture_model"
PINYIN_EXPORT_KMM="$L/utils/training/export_k_mixture_model"
PINYIN_MERGE_KMM="$L/utils/training/merge_k_mixture_model"
PINYIN_PRUNE_KMM="$L/utils/training/prune_k_mixture_model"
PINYIN_VALIDATE_KMM="$L/utils/training/validate_k_mixture_model"
PINYIN_KMM_TO_INTERP="$L/utils/training/k_mixture_model_to_interpolation"
# Legacy counting / interpolation utils (lambda + counter differentials).
PINYIN_GEN_BINARY_FILES="$L/utils/storage/gen_binary_files"
PINYIN_GEN_UNIGRAM="$L/utils/training/gen_unigram"
PINYIN_GEN_NGRAM="$L/utils/training/gen_ngram"
PINYIN_GEN_DELETED_NGRAM="$L/utils/training/gen_deleted_ngram"
PINYIN_ESTIMATE_INTERPOLATION="$L/utils/training/estimate_interpolation"
PINYIN_EXPORT_INTERPOLATION="$L/utils/storage/export_interpolation"
# eval_correction_rate: needs the compiled system bigram.db + an evals2.text.
PINYIN_EVAL_CORRECTION_RATE="$L/utils/training/eval_correction_rate"

tools=(PINYIN_SPSEG PINYIN_MERGESEQ PINYIN_NGSEG PINYIN_GEN_KMM PINYIN_EXPORT_KMM
	PINYIN_MERGE_KMM PINYIN_PRUNE_KMM PINYIN_VALIDATE_KMM PINYIN_KMM_TO_INTERP
	PINYIN_GEN_BINARY_FILES PINYIN_GEN_UNIGRAM PINYIN_GEN_NGRAM PINYIN_GEN_DELETED_NGRAM
	PINYIN_ESTIMATE_INTERPOLATION PINYIN_EXPORT_INTERPOLATION PINYIN_EVAL_CORRECTION_RATE)
for v in "${tools[@]}"; do
	need_exe "${!v}" "$v (built by make in the libpinyin tree)"
done
((missing)) && exit 2
export "${tools[@]}"

export LD_LIBRARY_PATH="$L/src/.libs${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

# The built system data dir (cwd for KMM/segment utils) and the oxpinyin export.
export PINYIN_GEN_NGRAM_DATA="$data"
export PINYIN_NGSEG_DATA="$data"
export PINYIN_EXPORT_DIR="$export_dir"
[[ -n $model ]] && export PINYIN_MODEL_DIR="$model"

# The evaluator gate: wired only when its two extra inputs exist. The pin
# side runs in the data dir (its .bin indexes, bigram.db, evals2.text); the
# native SystemDictionary reads the oxpinyin export of the same model, so
# the index paths point at the redb export, not the pin's .bin files.
eval_gate=
if [[ -f $data/bigram.db && -f $data/evals2.text ]]; then
	eval_gate=1
	need_file "$data/interpolation2.text" "the interpolation2.text bigram.db was imported from"
	need_file "$export_dir/pinyin_index.redb" "oxpinyin-datagen output"
	need_file "$export_dir/phrase_index.redb" "oxpinyin-datagen output"
	((missing)) && exit 2
	export PINYIN_EVAL_DATA="$data"
	export PINYIN_EVAL_INTERPOLATION2="$data/interpolation2.text"
	export PINYIN_EVAL_TABLE_CONF="$data/table.conf"
	export PINYIN_EVAL_PINYIN_INDEX="$export_dir/pinyin_index.redb"
	export PINYIN_EVAL_PHRASE_INDEX="$export_dir/phrase_index.redb"
fi

# ---- the suites -------------------------------------------------------------
# The backend-forwarding crates run with the pure-Rust redb backend; oxpinyin-kmm
# is backend-agnostic (no features).
feat=(--no-default-features --features redb)
report() {
	grep -E "live parity|parity:|value-identical|skipping|test result|diverges|stale|panicked|assertion|left:|right:" || true
}

status=0
run_suite() {
	local title=$1
	shift
	echo "== $title =="
	local out
	if out=$("$@" 2>&1); then
		printf '%s\n' "$out" | report
	else
		status=1
		printf '%s\n' "$out" | report
		printf 'FAILED: %s\n' "$title"
	fi
}

run_suite "KMM differentials (gen+export, to-interpolation, merge, prune, validate)" \
	cargo test -p oxpinyin-kmm --test differential -- --nocapture
run_suite "segment: spseg / mergeseq" \
	cargo test -p oxpinyin-segment "${feat[@]}" --test spseg_mergeseq -- --nocapture
run_suite "segment: ngseg (needs the system bigram)" \
	cargo test -p oxpinyin-segment "${feat[@]}" --test differential -- --nocapture
run_suite "lambda: estimate_interpolation" \
	cargo test -p oxpinyin-lambda "${feat[@]}" --test differential -- --nocapture
run_suite "counter: gen_ngram" \
	cargo test -p oxpinyin-counter "${feat[@]}" --test differential -- --nocapture
if [[ -n $eval_gate ]]; then
	run_suite "eval: eval_correction_rate" \
		cargo test -p oxpinyin-eval "${feat[@]}" --test differential -- --nocapture
else
	echo "== eval: eval_correction_rate =="
	echo "skipping: needs $data/bigram.db (import_interpolation) and $data/evals2.text"
fi

if ((status)); then
	echo "RESULT: at least one differential suite FAILED"
else
	echo "RESULT: every differential suite passed"
fi
exit "$status"
