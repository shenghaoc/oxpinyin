#!/usr/bin/env bash
# run-phrase-surface-diff.sh — Tier-B ABI differential: the phrase-result
# surface (`pinyin_phrase_segment` + `pinyin_get_n_phrase` +
# `pinyin_get_phrase_token`) and the prefix-seeded sentence guess
# (`pinyin_guess_sentence_with_prefix`).
#
# None of the five Tier-B symbols has a consumer call site in either
# frontend, so this driver (phrase-surface-diff.c) is their only oracle
# coverage: segment probes over the real tables log retval plus the full
# token@position array shape (failed-match all-null arrays included),
# and prefix-seeded guesses log retval plus the row-0 sentence.
#
# Exit codes: 0 = identical (the oracle actually ran); 1 = build/run
# failure; 2 = divergence; 3 = the validation could not run (probe
# family inactive, or the pin-built oracle is unavailable or off-pin).
#
# Env:
#   PINYIN_ORACLE_PREFIX  pin-built oracle prefix
#                         (default ~/.local/opt/pinyin-oracle)
#   PHRASE_SYSTEM         an oxpinyin-native system dir for the capi
#                         side (the oracle side always reads from
#                         PINYIN_ORACLE_PREFIX/lib/libpinyin/data). The
#                         three `.kct` tables (pinyin_index, phrase_index,
#                         bigram) plus interpolation2.text are required.
#                         `.kct` because this script's own `cargo build
#                         -p oxpinyin-capi` uses default features → KC,
#                         so the built `.so`'s compiled-in backend can
#                         only read `.kct`. Point at a redb/lmdb/tkrzw
#                         dir and the driver will fail to open it.

set -u
cd "$(dirname "$0")" || exit 1

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 3
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

echo "--- cc phrase-surface-diff.c ---"
DRIVER="$CARGO_TARGET_DIR/phrase-surface-diff"
cc -O2 -Wall -o "$DRIVER" phrase-surface-diff.c -ldl || exit 1

SYSTEM="${PHRASE_SYSTEM:-}"
# Relative PHRASE_SYSTEM is anchored to $REPO_ROOT — the shell is
# `cd`'d into tools/bisection by this point, which is not what a
# caller means by a relative path.
case "$SYSTEM" in
    ''|/*) ;;
    *) SYSTEM="$REPO_ROOT/$SYSTEM" ;;
esac
# This script's `cargo build -p oxpinyin-capi` above uses default
# features → Kyoto Cabinet, so the `.so` under test only opens `.kct`
# tables. Accepting other peer extensions here would pass the gate on
# a dir the driver cannot actually load, and the failure would land
# mid-run rather than as this clean skip.
has_all_kct_tables() {
    local t
    for t in pinyin_index phrase_index bigram; do
        [[ -f "$SYSTEM/$t.kct" ]] || return 1
    done
}
if [[ -z "$SYSTEM" ]] || ! [[ -f "$SYSTEM/interpolation2.text" ]] \
    || ! has_all_kct_tables; then
    echo "SKIP: PHRASE_SYSTEM must name an oxpinyin-native KC system dir"
    echo "  (pinyin_index.kct, phrase_index.kct and bigram.kct plus"
    echo "  interpolation2.text)"
    exit 3
fi

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
trap 'rm -f "$CAPI_LOG" "$ORACLE_LOG"' EXIT
if ! "$DRIVER" "$CAPI_SO" "$SYSTEM" > "$CAPI_LOG"; then
    echo "FAIL: phrase-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
if ! "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG"; then
    echo "FAIL: phrase-surface-diff crashed against oracle"
    cat "$ORACLE_LOG"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: every probe family is active on both sides ---"
probe_surfaces() {
    grep -q '^segment|你好中国|1|n=4|' "$1" && \
        grep -q '^segment|你好，世界。|0|n=6|' "$1" && \
        grep -q '^prefix|nihaoshijie|你好|1|' "$1" && \
        grep -q '^reset|rok=1|n=0|' "$1"
}
if ! probe_surfaces "$CAPI_LOG" || ! probe_surfaces "$ORACLE_LOG"; then
    echo "FAIL: a probe family is inactive (segment / prefix-guess / reset)"
    exit 3
fi

diff -u "$ORACLE_LOG" "$CAPI_LOG"
diff_status=$?
case "$diff_status" in
    0)
        echo "IDENTICAL: $(wc -l < "$CAPI_LOG") probe lines agree with the pin"
        ;;
    1)
        echo
        echo "DIVERGENCE: the logs differ (above)"
        exit 2
        ;;
    *)
        echo
        echo "FAIL: diff exited with status $diff_status (comparison error, not a divergence)"
        exit 1
        ;;
esac
