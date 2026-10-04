#!/usr/bin/env bash
# run-dict-surface-diff.sh — Tier-C ABI differential: the dictionary-
# introspection surface (`pinyin_lookup_tokens`, `pinyin_token_get_*`,
# `pinyin_token_add_unigram_frequency`) and the phrase-library
# load/unload pair.
#
# None of the eight Tier-C symbols has a consumer call site in either
# frontend, so this driver (dict-surface-diff.c) is their only oracle
# coverage: token sweeps feed the per-token reads (text, pronunciation
# counts, unigram frequencies — including the trainer's avoid-zero +1
# constant, gen_unigram.cpp:34-49), add-then-read sequences pin the
# overlay semantics, and the retval table pins the already-loaded /
# GBK-only / already-unloaded laws.
#
# Exclusions (pin aborts, documented in-driver): out-of-range library
# indexes assert at pinyin.cpp:466/:457 (no-abort refusals pinned by
# the Rust ABI suite), and a zero-length phrase lookup SIGFPEs in the
# pin's search (theirs-bug, recorded in upstream-divergences.md).
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a probe family is inactive.
#
# Env:
#   PINYIN_ORACLE_PREFIX  pin-built oracle prefix
#                         (default ~/.local/opt/pinyin-oracle)
#   DICT_SYSTEM           an oxpinyin-native system dir for the capi
#                         side (the oracle side always reads from
#                         PINYIN_ORACLE_PREFIX/lib/libpinyin/data). The
#                         core tables under libpinyin's own names
#                         (pinyin_index.bin, phrase_index.bin, bigram.db, punct.bin,
#                         the layout every datagen output and libpinyin
#                         install uses) plus interpolation2.text are
#                         required — punct because the driver dlsyms
#                         `pinyin_guess_predicted_candidates_with_punctuations`
#                         — and the directory must have been written
#                         by the cell's backend (its datagen-manifest.txt
#                         `backend=` or table.conf `database format:`); a
#                         directory from another backend FAILS (exit 1).
#   PINYIN_ORACLE_DBM     the backend cell: bdb (default — the reference
#                         build is a bare ./configure), kc or tkrzw. The
#                         oracle prefix must be built with the same
#                         `build-oracle.sh --dbm`, and the capi is built
#                         with the matching cargo feature
#                         (tools/bisection/oracle-cell.sh).

set -u
cd "$(dirname "$0")" || exit 1

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 0
fi
REPO_ROOT="$(git rev-parse --show-toplevel)"
# The backend cell (PINYIN_ORACLE_DBM, default bdb) fixes the expected
# pin ref (+dbm-<cell>), the capi's cargo feature and the data's
# backend. Exact whole-line match on the ref so an oracle built against
# a different model checksum or another cell's DBM does not silently
# validate this differential.
# shellcheck source=oracle-cell.sh
source ./oracle-cell.sh
# shellcheck source=system-dir.sh
source ./system-dir.sh
if ! grep -Fxq "$EXPECTED_PIN_REF" "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin for the $ORACLE_DBM cell"
    echo "  expected $EXPECTED_PIN_REF"
    echo "  (build it with build-oracle.sh --dbm $ORACLE_DBM, or set PINYIN_ORACLE_DBM)"
    exit 3
fi
# Supply OXPINYIN_CAPI_SO to run without Cargo; otherwise build dev opt-level 1.
oracle_cell_artifact OXPINYIN_CAPI_SO libpinyin_capi.so oxpinyin-capi || exit 1
CAPI_SO=$OXPINYIN_CAPI_SO
mkdir -p "$CARGO_TARGET_DIR"

echo "--- cc dict-surface-diff.c ---"
DRIVER="$CARGO_TARGET_DIR/dict-surface-diff"
if ! GLIB_LIBS="$(pkg-config --libs glib-2.0 2>/dev/null)"; then
    echo "FAIL: pkg-config could not resolve glib-2.0; install libglib2.0-dev (Debian/Ubuntu) or glib2-devel (Fedora)" >&2
    exit 1
fi
# shellcheck disable=SC2086
cc -O2 -Wall -o "$DRIVER" dict-surface-diff.c -ldl $GLIB_LIBS || exit 1

SYSTEM="${DICT_SYSTEM:-}"
# The capi under test is built for the cell's backend (CAPI_FEATURE), so
# the data must be that backend's: all three DBMs use libpinyin's own
# file names, so the names alone cannot tell a Kyoto Cabinet directory
# from a Berkeley DB one — the backend check below does.
# punct.bin is required: the driver dlsyms
# `pinyin_guess_predicted_candidates_with_punctuations` and a missing
# punct on the capi side would make its predicted list diverge from the
# oracle's for the wrong reason.
missing=()
for t in pinyin_index.bin phrase_index.bin bigram.db punct.bin interpolation2.text; do
    [[ -n "$SYSTEM" && -f "$SYSTEM/$t" ]] || missing+=("$t")
done
if ((${#missing[@]})); then
    echo "SKIP: DICT_SYSTEM must name a $ORACLE_DBM_NAME system data dir"
    echo "  missing: ${missing[*]}"
    exit 3
fi
if ! data_ext=$(system_dir_data_ext "$SYSTEM"); then
    echo "FAIL: cannot tell which backend wrote DICT_SYSTEM=$SYSTEM"
    echo "  (no datagen-manifest.txt backend= and no known table.conf database format:)"
    exit 1
fi
if [[ $data_ext != "$OXPINYIN_CAPI_BACKEND_EXT" ]]; then
    echo "FAIL: BACKEND MISMATCH — the $ORACLE_DBM cell's capi is $(system_dir_ext_name "$OXPINYIN_CAPI_BACKEND_EXT"),"
    echo "  but DICT_SYSTEM=$SYSTEM was written for $(system_dir_ext_name "$data_ext")"
    exit 1
fi

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
# The driver opens the context with an empty user dir (""), which both
# libraries resolve against the working directory: the pin writes
# user.conf there. Each side therefore runs in a fresh private directory
# (both start equal, and nothing lands in tools/bisection), removed on
# exit. The data paths are made absolute first, since the driver runs
# from elsewhere.
CAPI_CWD="$(mktemp -d)"
ORACLE_CWD="$(mktemp -d)"
trap 'rm -rf "$CAPI_LOG" "$ORACLE_LOG" "$CAPI_CWD" "$ORACLE_CWD"' EXIT
SYSTEM="$(cd "$SYSTEM" && pwd)"
ORACLE_DATA="$(cd "$ORACLE_DATA" && pwd)"
ORACLE_SO="$(cd "$(dirname "$ORACLE_SO")" && pwd)/$(basename "$ORACLE_SO")"
if ! (cd "$CAPI_CWD" && "$DRIVER" "$CAPI_SO" "$SYSTEM") > "$CAPI_LOG"; then
    echo "FAIL: dict-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
# The oracle reads its own libpinyin data dir; DICT_SYSTEM is
# oxpinyin-native and the oracle wouldn't know what to do with it.
# Both dirs derive from the same pinned model20, so the comparison is
# still same-source when the datagen convention is followed.
if ! (cd "$ORACLE_CWD" && "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA") > "$ORACLE_LOG"; then
    echo "FAIL: dict-surface-diff crashed against oracle"
    cat "$ORACLE_LOG"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: every probe family is active on both sides ---"
probe_surfaces() {
    # Lookup + per-token reads.
    grep -q '^lookup|你好|1|n=1|' "$1" && \
        grep -q '^token|你好|phrase=1|len=2|' "$1" && \
        # Read-after-write on both probed tokens must show the +11 add
        # actually landing (`add11=1`) AND the shift over the prior
        # freq being observed (`shift=1`); either 0 means the overlay
        # invariant this driver exists to pin is silently untested.
        grep -q '^token|你好|add11=1|.*shift=1$' "$1" && \
        grep -q '^token|中国|add11=1|.*shift=1$' "$1" && \
        grep -q '^absent-add=0' "$1" && \
        # Every unload / load row the driver emits — the retvals go
        # into the differential, but the ROWS themselves must be
        # present so a family that quietly stopped iterating is caught.
        grep -q '^unload|0|' "$1" && \
        grep -q '^unload|1|' "$1" && \
        grep -q '^unload|2|' "$1" && \
        grep -q '^unload|3|' "$1" && \
        grep -q '^unload|4|' "$1" && \
        grep -q '^unload|5|' "$1" && \
        grep -q '^unload|6|' "$1" && \
        grep -q '^unload|7|' "$1" && \
        grep -q '^load|1|' "$1" && \
        grep -q '^load|2|' "$1" && \
        grep -q '^load|4|' "$1" && \
        grep -q '^load|7|' "$1"
}
if ! probe_surfaces "$CAPI_LOG" || ! probe_surfaces "$ORACLE_LOG"; then
    echo "FAIL: a probe family is inactive (lookup / token / add / load-unload)"
    exit 3
fi

if diff -u "$ORACLE_LOG" "$CAPI_LOG"; then
    echo "IDENTICAL: $(wc -l < "$CAPI_LOG") probe lines agree with the pin"
else
    echo
    echo "DIVERGENCE: the logs differ (above)"
    exit 2
fi
