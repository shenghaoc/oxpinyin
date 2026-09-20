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

# ── Backend-extension helpers ────────────────────────────────────────────
#
# The peer-backend tables the capi opens carry the compiled backend's own
# extension (.kct/.tkt/.db). Runners that copy or gate on those
# tables use these helpers instead of hard-coding an extension.

# The native table extensions the capi can be compiled against, in the
# compile-time precedence order of oxpinyin-store's DefaultStore cfg chain
# (kyotocabinet > tkrzw > bdb; see "Native data-file naming under
# the compile-time backend" in docs/findings/upstream-divergences.md). The
# order matters only when one directory holds complete sets in several
# extensions (the old fixtures/w3 flat layout did): the first match wins.
# Since 2026-09-20 the workspace default is bdb, so `db` leads — a
# default-built capi opens BDB containers, and all three flavours name
# their tables identically, so a wrong pick is silent nonsense rather
# than a missing file.
SYSTEM_DIR_BACKEND_EXTS="db kct tkt"

# The peer-backend table stems the capi opens from a system directory.
# The first three are mandatory and must share one extension: the engine
# opens every table through the single compiled-in backend, so a directory
# mixing extensions is half-assembled for each backend. punct and the addon
# libraries (addon_<n>_pinyin_index / addon_<n>_phrase_index) are opened on
# demand and are optional here, but when present they travel with the core
# three (system_dir_copy_tables) so a driver that loads an addon or asks
# for punctuation sees the same directory the operator pointed at.
SYSTEM_DIR_CORE_STEMS="pinyin_index phrase_index bigram"

# The P6 native layout: since the runtime reads libpinyin's own data
# files directly (docs/findings/runtime-direct-libpinyin-data-2026-09-02.md),
# a Kyoto Cabinet or tkrzw build also opens an unmodified libpinyin
# install's data/ — the core trio under libpinyin's own names
# (pinyin_index.bin, phrase_index.bin, bigram.db), not peer-extension
# tables. detect_ext reports this layout as the pseudo-extension "bin";
# every peer has the native path. The
# backend set is the three libpinyin-native DBMs: Kyoto Cabinet, tkrzw
# and Berkeley DB (the store's bdb feature; datagen's out-dir "db").
SYSTEM_DIR_NATIVE_CORE="pinyin_index.bin phrase_index.bin bigram.db"

# Names the table extension the built capi opens.
#
#   system_dir_capi_ext
#
# Echoes one of $SYSTEM_DIR_BACKEND_EXTS, or nothing when it cannot tell.
# Three sources, in order:
#
#   1. OXPINYIN_CAPI_BACKEND_EXT, the override run-cpp-smoke.sh already
#      honours, for a capi built with an explicit backend feature. An
#      invalid value is fatal: it is a typo, not a data problem.
#   2. cargo's own feature resolution for `cargo build -p oxpinyin-capi`
#      -- the exact command every runner builds the capi with, so the
#      answer is the backend that build compiled in, not a guess from
#      whatever files happen to sit in the directory. Run offline: the
#      capi is already built by the time any runner resolves its data.
#   3. Nothing. The caller then falls back to scanning the directory in
#      precedence order (system_dir_detect_ext).
#
# A runner that builds the capi with --features must set the override;
# the cargo query here deliberately mirrors the plain build.
system_dir_capi_ext() {
	local repo_root feature ext=
	if [[ -n ${OXPINYIN_CAPI_BACKEND_EXT:-} ]]; then
		# "db" rides beside the peer extensions: the bdb build's
		# selector, whose tables live in the native layout.
		case " $SYSTEM_DIR_BACKEND_EXTS db " in
		*" $OXPINYIN_CAPI_BACKEND_EXT "*) ;;
		*)
			printf 'fatal: OXPINYIN_CAPI_BACKEND_EXT=%q is not one of: %s\n' \
				"$OXPINYIN_CAPI_BACKEND_EXT" "$SYSTEM_DIR_BACKEND_EXTS" >&2
			exit 3
			;;
		esac
		printf '%s\n' "$OXPINYIN_CAPI_BACKEND_EXT"
		return 0
	fi
	command -v cargo >/dev/null 2>&1 || return 0
	repo_root=${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}
	for feature in $(cargo tree --offline --manifest-path "$repo_root/Cargo.toml" \
		-p oxpinyin-capi -e features -i oxpinyin-store --prefix none 2>/dev/null |
		sed -n 's/^oxpinyin-store feature "\([a-z]*\)".*/\1/p' | sort -u); do
		case $feature in
		kyotocabinet) [[ -z $ext ]] && ext=kct ;;
		tkrzw) [[ -z $ext ]] && ext=tkt ;;
		bdb) [[ -z $ext ]] && ext=db ;;
		esac
	done
	[[ -n $ext ]] && printf '%s\n' "$ext"
	return 0
}

# Names the backend extension of a system directory.
#
#   system_dir_detect_ext <dir>
#
# Echoes the extension under which all three core tables exist. When the
# capi's backend is known (system_dir_capi_ext) only that extension
# counts: a complete .tkt set is no use to a .kct capi. When it is not,
# the directory is scanned in precedence order and the first complete set
# wins. A directory holding both a peer set and the P6 native trio
# resolves to the peer set. With no peer set, a kct-, tkt- or db-built
# capi falls back to the native layout ($SYSTEM_DIR_NATIVE_CORE) and
# the answer is the pseudo-extension "bin"; the fallback needs a known
# backend because P6 covers the three libpinyin-native DBMs — Kyoto
# Cabinet, tkrzw and Berkeley DB (the store's bdb feature; datagen's
# out-dir name: "db"), and without cargo there is no built capi to
# speak of. Echoes
# nothing and returns 1 when there is no complete core set.
system_dir_detect_ext() {
	local dir=$1 ext stem exts capi_ext
	exts=$(system_dir_capi_ext)
	capi_ext=$exts
	[[ -z $exts ]] && exts=$SYSTEM_DIR_BACKEND_EXTS
	for ext in $exts; do
		for stem in $SYSTEM_DIR_CORE_STEMS; do
			[[ -f $dir/$stem.$ext ]] || continue 2
		done
		printf '%s\n' "$ext"
		return 0
	done
	case $capi_ext in
	kct | tkt | db)
		for stem in $SYSTEM_DIR_NATIVE_CORE; do
			[[ -f $dir/$stem ]] || return 1
		done
		printf 'bin\n'
		return 0
		;;
	esac
	return 1
}

# Copies the capi-side tables of one system directory into another.
#
#   system_dir_copy_tables <src> <dst>
#
# The three core tables in the detected extension, plus punct and every
# addon_<n>_* table in that extension when present. For the P6 native
# layout ("bin") the whole flat directory travels instead: the chunk
# libraries (gb_char.bin … merged.bin), table.conf, punct.bin and the
# addon pair are all files the readers open, and there is no extension
# to select them by. interpolation2.text is NOT copied: the runners
# resolve the real-unigram source themselves (an override may replace
# the directory's own copy). Exits 3 when <src> has no complete core
# set, which system_dir_require_complete would have reported already on
# every path that reaches here.
system_dir_copy_tables() {
	local src=$1 dst=$2 ext stem file
	ext=$(system_dir_detect_ext "$src") || {
		printf 'fatal: %s holds no complete core-table set in one extension\n' "$src" >&2
		exit 3
	}
	if [[ $ext == bin ]]; then
		find "$src" -mindepth 1 -maxdepth 1 -type f \
			! -name interpolation2.text -exec cp {} "$dst"/ \;
		return 0
	fi
	for stem in $SYSTEM_DIR_CORE_STEMS; do
		cp "$src/$stem.$ext" "$dst/"
	done
	for file in "$src/punct.$ext" "$src"/addon_*_pinyin_index."$ext" "$src"/addon_*_phrase_index."$ext"; do
		[[ -f $file ]] && cp "$file" "$dst/"
	done
	return 0
}

# ── Resolution and validation ───────────────────────────────────────────

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
	#    oxpinyin-datagen's default --out-dir is target/datagen/<ext>, one
	#    per backend, so every backend's directory is a candidate; the
	#    extension order is the compile-time precedence (see
	#    system_dir_detect_ext) so the default build's tables win.
	if [[ -z $resolved ]]; then
		for candidate in \
			"$repo_root/target/datagen/kct" \
			"$repo_root/target/datagen/tkt" \
			"$repo_root/target/datagen/db" \
			/tmp/oxpinyin-export; do
			if [[ -f $candidate/gb_char.bin ]]; then
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
		printf '%s\n' "$repo_root/fixtures/w3/kct"
		return 0
	fi

	{
		printf 'fatal: %s has an oracle but no system data directory.\n' "$label"
		printf '\n'
		printf 'Looked at, in order:\n'
		printf '  $%s          (this runner'"'"'s own variable)\n' "$var_name"
		printf '  $OXPINYIN_SYSTEM_DIR   (set once for a whole sweep)\n'
		printf '  %s/target/datagen/{kct,tkt,db}\n' "$repo_root"
		printf '  /tmp/oxpinyin-export\n'
		printf '\n'
		printf 'A usable directory is a system data directory for the compiled-in\n'
		printf 'backend: the chunk files, table.conf, and the DBMs.\n'
		printf '\n'
		printf 'Build one from the pinned model (the default build writes .kct\n'
		printf 'under target/datagen/kct):\n'
		printf '  tools/model/fetch-model.sh\n'
		printf '  PINYIN_MODEL_DIR=$PWD/target/model20/extracted \\\n'
		printf '    cargo run --release -p oxpinyin-datagen -- compile\n'
		printf '  cp target/model20/extracted/interpolation2.text target/datagen/kct/\n'
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
	# A system data directory holds the chunk files, table.conf, and the
	# DBMs under the compiled-in backend's names: libpinyin's own
	# (pinyin_index.bin, bigram.db, ...) on Kyoto Cabinet, tkrzw and
	# Berkeley DB, each under the backend's own names.
	for file in gb_char.bin table.conf; do
		[[ -f $dir/$file ]] || missing+=("$file")
	done
	local found_index=
	# `db` first: the default build is Berkeley DB since 2026-09-20.
	# FIRST match wins, hence the break — without it the last match would
	# win and the effective precedence would be the reverse of this list.
	# A directory holding several complete sets (the pre-split flat
	# fixtures/w3 layout) therefore resolves to `db`: the peer trio
	# pinyin_index.db / phrase_index.db / bigram.db. Note the collision
	# that shape has on this backend: its bigram is spelled `bigram.db`,
	# the same name the native layout uses on every backend, so the two
	# are indistinguishable by name — the check accepts either, and on
	# Berkeley DB both open through the same backend.
	for file in pinyin_index.db pinyin_index.bin pinyin_index.kct pinyin_index.tkt; do
		if [[ -f $dir/$file ]]; then
			found_index=$file
			break
		fi
	done
	[[ -n $found_index ]] || missing+=("pinyin_index.{bin,kct,tkt,db}")
	# The core trio must be complete in ONE layout: an index table whose
	# siblings are missing is a half-assembled directory, refused here
	# rather than at the first mid-run open failure. The native trio
	# carries libpinyin's own bigram name; a peer trio shares the
	# detected extension.
	if [[ $found_index == pinyin_index.bin ]]; then
		[[ -f $dir/phrase_index.bin ]] || missing+=("phrase_index.bin")
		[[ -f $dir/bigram.db ]] || missing+=("bigram.db")
	else
		local peer=${found_index##*.}
		if [[ -n $peer ]]; then
			[[ -f $dir/phrase_index.$peer ]] || missing+=("phrase_index.$peer")
			[[ -f $dir/bigram.$peer ]] || missing+=("bigram.$peer")
		fi
	fi
	((${#missing[@]} == 0)) && return 0
	{
		printf 'fatal: %s: the system directory is incomplete.\n' "$label"
		printf '  %s\n' "$dir"
		printf '  (from $%s, $OXPINYIN_SYSTEM_DIR, or a conventional location)\n' "$var_name"
		printf '\nMissing:\n'
		printf '  %s\n' "${missing[@]}"
		printf '\nA system data directory is an oxpinyin-datagen compile output, or\n'
		printf 'a libpinyin install'"'"'s data/ on Kyoto Cabinet, tkrzw or Berkeley DB:\n'
		printf 'the chunk files, table.conf, and the DBMs under the backend'"'"'s names.\n'
	} >&2
	exit 3
}
