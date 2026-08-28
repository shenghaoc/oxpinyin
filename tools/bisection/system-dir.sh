#!/usr/bin/env bash
# system-dir.sh — shared resolution of a differential's oxpinyin system
# data directory. Sourced, never executed.
#
# WHY THIS EXISTS
#
# Several runners took a system-dir variable under their own name and,
# when it was unset, quietly used the committed fixtures/w3 mini tables
# instead. Against a real oracle that is not a smaller measurement, it is
# a meaningless one: the mini tables hold a few hundred phrases, so the
# capi side answers with an empty or tiny candidate list and the runner
# reports DIVERGENCE. The failure looks exactly like a parity regression
# and is nothing of the sort.
#
# That misfired for real. A 2026-08-28 sweep read `scheme` and
# `option-sweep` as DIVERGENT; both were IDENTICAL once their variables
# were set. The names differ per runner and one of them
# (CAPI_W11_SYSTEM_DIR) was not documented at all, so setting "the"
# variable is guesswork without reading each script.
#
# THE RULE
#
# A differential that has a real oracle must never score against the mini
# fixture by accident. So:
#
#   * every runner accepts OXPINYIN_SYSTEM_DIR, one name for a whole
#     sweep, and still honours its own historical variable as an override;
#   * an unresolvable system dir is FATAL (exit 3), naming the variables
#     it looked at and what a valid directory contains;
#   * the mini fixture is still reachable, but only by asking for it:
#     OXPINYIN_ALLOW_MINI_FIXTURE=1, which prints a loud banner saying the
#     run is not a parity measurement.
#
# WHAT THIS DOES NOT CHANGE
#
# The oracle-absent skip. A runner with no pin-built oracle still exits 0
# with SKIP, because that is the path CI takes and it is deliberate: CI
# never provisions the oracle (see .github/workflows/store-backends.yml).
# The fatal case here is the other one — an oracle IS present, so a real
# measurement is about to run, and its input data is not.

# Resolves the system data directory for a runner.
#
#   resolve_system_dir <RUNNER_VARIABLE_NAME> <runner-label>
#
# Echoes the resolved directory on stdout. Exits 3 if none can be found.
resolve_system_dir() {
	local var_name=$1 label=$2
	local repo_root candidate resolved=
	# Every runner computes REPO_ROOT before sourcing this; deriving it
	# again from $BASH_SOURCE would depend on the caller's cwd, and each
	# runner cd's into tools/bisection first.
	repo_root=${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}

	# 1. the runner's own variable, 2. the sweep-wide one.
	resolved=${!var_name:-}
	[[ -z $resolved ]] && resolved=${OXPINYIN_SYSTEM_DIR:-}

	# 3. the conventional build locations, newest convention first.
	if [[ -z $resolved ]]; then
		for candidate in \
			"$repo_root/target/datagen/redb" \
			/tmp/oxpinyin-export; do
			if [[ -f $candidate/pinyin_index.redb ]]; then
				resolved=$candidate
				break
			fi
		done
	fi

	if [[ -n $resolved ]]; then
		system_dir_require_complete "$resolved" "$var_name" "$label"
		printf '%s\n' "$resolved"
		return 0
	fi

	if [[ ${OXPINYIN_ALLOW_MINI_FIXTURE:-} == 1 ]]; then
		printf '\n' >&2
		printf '################################################################\n' >&2
		printf '# %s is running against fixtures/w3 -- the MINI tables.\n' "$label" >&2
		printf '# This is NOT a parity measurement. The mini tables hold a\n' >&2
		printf '# few hundred phrases, so a divergence here says nothing about\n' >&2
		printf '# agreement with the pin. Unset OXPINYIN_ALLOW_MINI_FIXTURE to\n' >&2
		printf '# make this fatal again.\n' >&2
		printf '################################################################\n' >&2
		printf '\n' >&2
		printf '%s\n' "$repo_root/fixtures/w3"
		return 0
	fi

	{
		printf 'fatal: %s has an oracle but no system data directory.\n' "$label"
		printf '\n'
		printf 'Looked at, in order:\n'
		printf '  $%s          (this runner'"'"'s own variable)\n' "$var_name"
		printf '  $OXPINYIN_SYSTEM_DIR   (set once for a whole sweep)\n'
		printf '  %s/target/datagen/redb\n' "$repo_root"
		printf '  /tmp/oxpinyin-export\n'
		printf '\n'
		printf 'A usable directory holds the real-unigram tables:\n'
		printf '  pinyin_index.redb  phrase_index.redb  bigram.redb  interpolation2.text\n'
		printf '\n'
		printf 'Build one from the pinned model:\n'
		printf '  tools/model/fetch-model.sh\n'
		printf '  PINYIN_MODEL_DIR=$PWD/target/model20/extracted \\\n'
		printf '    cargo run --release -p oxpinyin-datagen -- compile --out-dir target/datagen/redb\n'
		printf '  cp target/model20/extracted/interpolation2.text target/datagen/redb/\n'
		printf '\n'
		printf 'Refusing rather than falling back to fixtures/w3: scoring a real\n'
		printf 'oracle against the mini tables reports DIVERGENCE that means\n'
		printf 'nothing. Set OXPINYIN_ALLOW_MINI_FIXTURE=1 if you deliberately\n'
		printf 'want the mini-fixture run.\n'
	} >&2
	exit 3
}

# Refuses a directory that is missing the tables a real measurement needs.
#
# Being pointed at the wrong directory is the same failure as not being
# pointed at one, and it is easier to make: a path typo resolves to
# something, and the runner would score against whatever it holds.
system_dir_require_complete() {
	local dir=$1 var_name=$2 label=$3
	local missing=() file
	for file in pinyin_index.redb phrase_index.redb bigram.redb interpolation2.text; do
		[[ -f $dir/$file ]] || missing+=("$file")
	done
	((${#missing[@]} == 0)) && return 0
	{
		printf 'fatal: %s: the system directory is incomplete.\n' "$label"
		printf '  %s\n' "$dir"
		printf '  (from $%s, $OXPINYIN_SYSTEM_DIR, or a conventional location)\n' "$var_name"
		printf '\nMissing:\n'
		printf '  %s\n' "${missing[@]}"
		printf '\nAll four are required: the three tables plus interpolation2.text,\n'
		printf 'whose real unigrams are what the oracle scores against. Without it\n'
		printf 'the comparison is flat-export unigrams versus the pin'"'"'s real ones\n'
		printf '-- a data mismatch that reports as a divergence.\n'
	} >&2
	exit 3
}
