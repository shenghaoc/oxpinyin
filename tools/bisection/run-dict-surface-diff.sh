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
#                         four `.kct` tables (pinyin_index, phrase_index,
#                         bigram, punct) plus interpolation2.text are
#                         required — punct because the driver dlsyms
#                         `pinyin_guess_predicted_candidates_with_punctuations`.
#                         `.kct` because this script's own `cargo build
#                         -p oxpinyin-capi` uses default features → KC,
#                         so the built `.so`'s compiled-in backend can
#                         only read `.kct`.

set -u
cd "$(dirname "$0")" || exit 1

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 0
fi
# Full ORACLE_PIN_REF from tools/oracle/build-oracle.sh (LIBPINYIN_TAG,
# LIBPINYIN_SHA, MODEL_SHA256, dbm). Exact whole-line match so an oracle
# built against a different model checksum or a different DBM backend
# does not silently validate this differential.
EXPECTED_PIN_REF='pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw'
if ! grep -Fxq "$EXPECTED_PIN_REF" "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 3
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
# Honour CARGO_TARGET_DIR so a caller who redirects cargo output (a
# distro-package builder, a shared-target CI, a per-worktree target)
# ends up loading the .so cargo actually wrote instead of a stale one
# at $REPO_ROOT/target. Anchor a relative value to $REPO_ROOT rather
# than `$(pwd)` — the shell is `cd`'d into tools/bisection by this
# point, which is not what a caller means by a relative path.
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
case "$CARGO_TARGET_DIR" in
    /*) ;;
    *) CARGO_TARGET_DIR="$REPO_ROOT/$CARGO_TARGET_DIR" ;;
esac
CAPI_SO="$CARGO_TARGET_DIR/release/libpinyin_capi.so"
# Always rebuild — cargo is a no-op when the artifact is current, and a
# stale libpinyin_capi.so on disk would otherwise mask the change under
# test.
echo "building libpinyin_capi.so (release)..."
(cd "$REPO_ROOT" && CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --release -p oxpinyin-capi) || exit 1

echo "--- cc dict-surface-diff.c ---"
DRIVER="$CARGO_TARGET_DIR/dict-surface-diff"
if ! GLIB_LIBS="$(pkg-config --libs glib-2.0 2>/dev/null)"; then
    echo "FAIL: pkg-config could not resolve glib-2.0; install libglib2.0-dev (Debian/Ubuntu) or glib2-devel (Fedora)" >&2
    exit 1
fi
# shellcheck disable=SC2086
cc -O2 -Wall -o "$DRIVER" dict-surface-diff.c -ldl $GLIB_LIBS || exit 1

SYSTEM="${DICT_SYSTEM:-}"
# This script's `cargo build -p oxpinyin-capi` above uses default
# features → Kyoto Cabinet, so the `.so` under test only opens `.kct`
# tables. Accepting other peer extensions here would pass the gate on
# a dir the driver cannot actually load, and the failure would land
# mid-run rather than as this clean skip. `punct` is fourth: the driver
# dlsyms `pinyin_guess_predicted_candidates_with_punctuations` and a
# missing punct on the capi side would make its predicted list diverge
# from the oracle's for the wrong reason.
has_all_kct_tables() {
    local t
    for t in pinyin_index phrase_index bigram punct; do
        [[ -f "$SYSTEM/$t.kct" ]] || return 1
    done
}
if [[ -z "$SYSTEM" ]] || ! [[ -f "$SYSTEM/interpolation2.text" ]] \
    || ! has_all_kct_tables; then
    echo "SKIP: DICT_SYSTEM must name an oxpinyin-native KC system dir"
    echo "  (pinyin_index.kct, phrase_index.kct, bigram.kct and punct.kct"
    echo "  plus interpolation2.text)"
    exit 3
fi

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
trap 'rm -f "$CAPI_LOG" "$ORACLE_LOG"' EXIT
if ! "$DRIVER" "$CAPI_SO" "$SYSTEM" > "$CAPI_LOG"; then
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
if ! "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG"; then
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
