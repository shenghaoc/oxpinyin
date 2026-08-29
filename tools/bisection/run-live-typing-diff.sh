#!/usr/bin/env bash
# run-live-typing-diff.sh — post-choose live-typing + decoded-continuation
# train differential.
#
# Drives the scripted C-ABI sequence (tools/bisection/live-typing-diff.c)
# into libpinyin_capi.so and the pin-built libpinyin.so and diffs the FULL
# logs (per-keystroke post-choose surface + decoded-continuation train +
# user-store export). This is the W12 live-typing coverage item — the
# surfaces no frozen pin gates:
#
#   live typing after a choose — upstream's constrained re-decode keeps the
#   chosen 你 forced in the key matrix, so the oracle's mid-composition
#   sentence rows carry the real constrained trellis; the engine re-seeds
#   from the recorded history (the W6 surface, sentence-surface.md §3) with
#   the §10 text prefix bolted on.
#
#   decoded-continuation training — upstream's pinyin_train walks the
#   constrained decode (train_result3: user-chosen phrases plus the first
#   decoded phrase after each constrained run); the engine's record holds
#   only explicitly chosen tokens (user-store.md §2.1). Choose 你 for "ni",
#   let 好 decode, commit: the oracle trains 你→好, the engine does not.
#
# Env-gated on the pin-built oracle like the train diff
# (PINYIN_ORACLE_PREFIX, default $HOME/.local/opt/pinyin-oracle) and on a
# real-unigram capi system dir (LIVETYPING_SYSTEM, e.g. the matched model20
# tables with pinyin_index.redb/phrase_index.redb/bigram.redb and
# interpolation2.text). Rounds: LIVETYPING_ROUNDS (default 3).
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure;
# 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building live-typing-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o live-typing-diff live-typing-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

# Prerequisite, not a warning: the post-choose probes only mean anything
# while both sides actually advanced past the choose into a remaining
# input that offers candidates. A missing cursor or an empty after-choose
# window means the live-typing surface never engaged; the typed probes'
# contents are COMPARED surfaces (the engines answer differently there
# today), never prerequisites.
probe_live_surface() {
    grep -q '^live:cursor=' "$1" && \
    grep -q '^round:1 cursor=' "$1" && \
    grep -q '^probe:after-choose n=[1-9]' "$1" && \
    grep -q '^cr:cursor=' "$1" && \
    grep -q '^cr:clear0=1' "$1"
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

# LIVETYPING_SYSTEM first, then OXPINYIN_SYSTEM_DIR -- the one name that
# works across every differential, so a whole sweep needs one export
# rather than a different variable per runner (see system-dir.sh).
CAPI_SYSTEM="${LIVETYPING_SYSTEM:-${OXPINYIN_SYSTEM_DIR:-}}"
# The tables' extension names the backend the capi was compiled with
# (.redb default; .tkt/.lmdb behind their features); accept any
# provisioned form.
has_table() { [ -f "$CAPI_SYSTEM/$1.redb" ] || [ -f "$CAPI_SYSTEM/$1.tkt" ] || [ -f "$CAPI_SYSTEM/$1.lmdb" ]; }
if [[ -z "$CAPI_SYSTEM" ]] || ! [[ -f "$CAPI_SYSTEM/interpolation2.text" ]] || ! has_table pinyin_index || ! has_table phrase_index || ! has_table bigram; then
    echo "SKIP: LIVETYPING_SYSTEM must name a real-unigram system dir"
    echo "  (pinyin_index.{redb|tkt|lmdb}, phrase_index.…, bigram.…, interpolation2.text)"
    exit 0
fi
# The four-file presence check catches half-assembled dirs; it does NOT bind
# the tables' identity to the oracle pin (a content hash or manifest belongs
# to the parked oracle-provisioning work, where the dir is assembled
# mechanically instead of by hand).

ROUNDS="${LIVETYPING_ROUNDS:-3}"

echo "--- capi side (rounds=$ROUNDS) ---"
CAPI_LOG="$(mktemp)"
CAPI_ERR="$(mktemp)"
if ! LIVETYPING_ROUNDS="$ROUNDS" \
    ./live-typing-diff "$CAPI_SO" "$CAPI_SYSTEM" > "$CAPI_LOG" 2> "$CAPI_ERR"; then
    echo "FAIL: live-typing-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$CAPI_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
ORACLE_LOG="$(mktemp)"
ORACLE_ERR="$(mktemp)"
if ! LIVETYPING_ROUNDS="$ROUNDS" \
    ./live-typing-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> "$ORACLE_ERR"; then
    echo "FAIL: live-typing-diff crashed against oracle"
    cat "$ORACLE_LOG"
    echo "--- driver diagnostics (stderr) ---"
    cat "$ORACLE_ERR"
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: the post-choose surface is active on both sides ---"
if ! probe_live_surface "$CAPI_LOG" || ! probe_live_surface "$ORACLE_LOG"; then
    echo "FAIL: live-typing surface inactive (cursor / core round / after-choose window)"
    echo "  A missing cursor or an empty after-choose candidate window means the run"
    echo "  compared only the pre-choose surface and never exercised the"
    echo "  post-choose decode or the decoded-continuation train."
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 1
fi
echo "post-choose surface active on both sides"

echo "--- differential (full log: live-typing surface + train + export) ---"
diff_status=0
diff -u "$ORACLE_LOG" "$CAPI_LOG" > /dev/null || diff_status=$?
if (( diff_status > 1 )); then
    # diff's own failure (an unreadable log, an I/O error) is a harness
    # failure, not a measured divergence — never the intentional exit 2.
    echo "FAIL: could not compare the differential logs (diff status $diff_status)" >&2
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 1
fi
if (( diff_status == 0 )); then
    echo "live-typing-diff: IDENTICAL"
    grep -E '^(phrase|bigram):' "$CAPI_LOG"
    rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
    exit 0
fi
echo "DIVERGENCE (live-typing surface or train/export)"
diff -u "$ORACLE_LOG" "$CAPI_LOG" || true
rm -f "$CAPI_LOG" "$CAPI_ERR" "$ORACLE_LOG" "$ORACLE_ERR"
exit 2
