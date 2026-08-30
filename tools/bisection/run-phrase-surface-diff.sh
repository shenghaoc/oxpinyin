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
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a probe family is inactive.
#
# Env:
#   PINYIN_ORACLE_PREFIX  pin-built oracle prefix
#                         (default ~/.local/opt/pinyin-oracle)
#   PHRASE_SYSTEM         real-unigram capi system dir (5 redb/text
#                         files). When unset, one is assembled into
#                         target/datagen/redb from the pinned model20
#                         archive (fetch-model.sh + oxpinyin-datagen).

set -u
cd "$(dirname "$0")"

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
CAPI_SO="$REPO_ROOT/target/release/libpinyin_capi.so"
if [[ ! -f "$CAPI_SO" ]]; then
    echo "building libpinyin_capi.so (release)..."
    cargo build --release -p oxpinyin-capi || exit 1
fi

echo "--- cc phrase-surface-diff.c ---"
DRIVER="$REPO_ROOT/target/phrase-surface-diff"
cc -O2 -Wall -o "$DRIVER" phrase-surface-diff.c -ldl || exit 1

SYSTEM="${PHRASE_SYSTEM:-$REPO_ROOT/target/datagen/redb}"
if [[ ! -f "$SYSTEM/interpolation2.text" || ! -f "$SYSTEM/pinyin_index.redb" \
    || ! -f "$SYSTEM/phrase_index.redb" || ! -f "$SYSTEM/bigram.redb" ]]; then
    echo "assembling real-unigram system dir at $SYSTEM (pinned model20)..."
    export PINYIN_MODEL_DIR="${PINYIN_MODEL_DIR:-$REPO_ROOT/target/model20/extracted}"
    if [[ ! -f "$PINYIN_MODEL_DIR/table.conf" ]]; then
        (cd "$REPO_ROOT" && tools/model/fetch-model.sh) || exit 1
    fi
    (cd "$REPO_ROOT" && cargo run --release -p oxpinyin-datagen -- compile \
        --out-dir "$SYSTEM") || exit 1
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
        grep -q '^reset|n=0' "$1"
}
if ! probe_surfaces "$CAPI_LOG" || ! probe_surfaces "$ORACLE_LOG"; then
    echo "FAIL: a probe family is inactive (segment / prefix-guess / reset)"
    exit 3
fi

if diff -u "$ORACLE_LOG" "$CAPI_LOG"; then
    echo "IDENTICAL: $(wc -l < "$CAPI_LOG") probe lines agree with the pin"
else
    echo
    echo "DIVERGENCE: the logs differ (above)"
    exit 2
fi
