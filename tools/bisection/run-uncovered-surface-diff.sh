#!/usr/bin/env bash
# run-uncovered-surface-diff.sh — W12 parked live-typing coverage which no
# frozen pin gates, part 2: the four surfaces beyond choose/typing/backspace.
#
# Drives the scripted C-ABI sequence (tools/bisection/uncovered-surface-diff.c)
# into libpinyin_capi.so and the pin-built libpinyin.so and diffs the FULL
# logs (deep paging + punct prediction/parse + FORCE_TONE / DYNAMIC_ADJUST
# profiles + mid-composition cursor moves). The W12 surfaces measured:
#
#   deep paging — the frontend pages its own LookupTable (page size 5,
#   PYPConfig.cc:148) over the candidate array one guess returns; the driver
#   walks pages 0..11 plus the last page and chooses from a deep page.
#
#   punctuation modes — full/half width and Chinese/English punct toggles
#   are ibus-frontend state, not this ABI (no such exports in the pinned
#   pinyin.h); the ABI surface is the punct-table prediction path plus
#   punctuation bytes inside the composition.
#
#   option profiles — FORCE_TONE and DYNAMIC_ADJUST, the two bit classes
#   the corpus never exercised (option-bits.md).
#
#   cursor moves — the ABI readouts the frontend drives on Left/Right:
#   auxiliary text, lookup offset, word-level left/right offsets, the
#   window at each moved cursor, and one mid-buffer choose.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure;
# 2 = divergence (the expected, measured state — do not wire into CI green
# until a later PR closes the classes in
# docs/findings/uncovered-surface-differentials.md).
#
# Env-gated on the pin-built oracle (PINYIN_ORACLE_PREFIX, default
# $HOME/.local/opt/pinyin-oracle) and on a real-unigram capi system dir
# (UNCOVERED_SYSTEM) holding pinyin_index.redb, phrase_index.redb,
# bigram.redb, interpolation2.text, AND punct.redb — the Option A export
# (token LE → NUL-terminated UTF-8, docs/findings/prediction-punct.md) of
# the SAME model20 punct.table the oracle's punct.bin was built from, so
# the punct rows are compared over matched tables (370 rows / 272 tokens).

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building uncovered-surface-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o uncovered-surface-diff \
    uncovered-surface-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

# Prerequisite, not a warning: every phase's probes only mean something
# when both sides actually produced that phase's surfaces. A missing page
# walk, punct prediction, profile probe, cursor table, or raw-offset probe
# means the run compared nothing for that surface and must not report
# IDENTICAL.
probe_surfaces() {
    grep -q '^page-r0-shi:page=0' "$1" && \
    grep -q '^punct-hao:predict=' "$1" && \
    grep -q '^opt:0x60-ni3hao3@0:parsed=' "$1" && \
    grep -q '^cur:0 aux=' "$1" && \
    grep -q '^raw:nihao@3:guess=1' "$1" && \
    grep -q '^raw:nihao@3:n=0' "$1"
}

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    echo "  expected libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c"
    exit 0
fi
if [[ ! -f "$ORACLE_DATA/bigram.db" ]]; then
    echo "SKIP: oracle data not found at $ORACLE_DATA"
    exit 0
fi

SYSTEM="${UNCOVERED_SYSTEM:-}"
if [[ -z "$SYSTEM" ]] || ! [[ -f "$SYSTEM/interpolation2.text"          && -f "$SYSTEM/pinyin_index.redb"          && -f "$SYSTEM/phrase_index.redb"          && -f "$SYSTEM/bigram.redb"          && -f "$SYSTEM/punct.redb" ]]; then
    echo "SKIP: UNCOVERED_SYSTEM must name a real-unigram system dir"
    echo "  (pinyin_index.redb, phrase_index.redb, bigram.redb, interpolation2.text,"
    echo "   plus the Option A punct.redb of the same model20 punct.table)"
    exit 0
fi
# The five-file presence check catches half-assembled dirs; it does NOT bind
# the tables' identity to the oracle pin (same parked oracle-provisioning
# caveat as run-live-typing-diff.sh).

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
CAPI_ERR="$(mktemp)"
ORACLE_LOG="$(mktemp)"
ORACLE_ERR="$(mktemp)"
trap 'rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"' EXIT
if ! ./uncovered-surface-diff "$CAPI_SO" "$SYSTEM" > "$CAPI_LOG" 2> "$CAPI_ERR"; then
    echo "FAIL: uncovered-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$CAPI_ERR"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
if ! ./uncovered-surface-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> "$ORACLE_ERR"; then
    echo "FAIL: uncovered-surface-diff crashed against oracle"
    cat "$ORACLE_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$ORACLE_ERR"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: every phase's surface is active on both sides ---"
if ! probe_surfaces "$CAPI_LOG" || ! probe_surfaces "$ORACLE_LOG"; then
    echo "FAIL: a phase surface is inactive (paging / punct / profiles / cursor / raw offsets)"
    echo "  A missing page walk, punct prediction, profile probe, cursor"
    echo "  table, or raw-offset probe means that phase compared nothing."
    exit 1
fi
echo "all five phase surfaces active on both sides"

echo "--- differential (full log: paging + punct + profiles + cursor + raw offsets) ---"
diff_status=0
diff -u "$ORACLE_LOG" "$CAPI_LOG" > /dev/null || diff_status=$?
if (( diff_status > 1 )); then
    # diff's own failure (an unreadable log, an I/O error) is a harness
    # failure, not a measured divergence — never the intentional exit 2.
    echo "FAIL: could not compare the differential logs (diff status $diff_status)" >&2
    exit 1
fi
if (( diff_status == 0 )); then
    echo "uncovered-surface-diff: IDENTICAL"
    exit 0
fi
echo "DIVERGENCE (paging / punct / option profiles / cursor surfaces)"
diff -u "$ORACLE_LOG" "$CAPI_LOG" || true
exit 2
