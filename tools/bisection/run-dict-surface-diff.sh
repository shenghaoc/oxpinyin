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
#   DICT_SYSTEM           real-unigram capi system dir (5 redb/text
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
# Always rebuild — cargo is a no-op when the artifact is current, and a
# stale libpinyin_capi.so on disk would otherwise mask the change under
# test.
echo "building libpinyin_capi.so (release)..."
(cd "$REPO_ROOT" && cargo build --release -p oxpinyin-capi) || exit 1

echo "--- cc dict-surface-diff.c ---"
DRIVER="$REPO_ROOT/target/dict-surface-diff"
if ! GLIB_LIBS="$(pkg-config --libs glib-2.0 2>/dev/null)"; then
    echo "FAIL: pkg-config could not resolve glib-2.0; install libglib2.0-dev (Debian/Ubuntu) or glib2-devel (Fedora)" >&2
    exit 1
fi
# shellcheck disable=SC2086
cc -O2 -Wall -o "$DRIVER" dict-surface-diff.c -ldl $GLIB_LIBS || exit 1

SYSTEM="${DICT_SYSTEM:-$REPO_ROOT/target/datagen/redb}"
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
    echo "FAIL: dict-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
if ! "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG"; then
    echo "FAIL: dict-surface-diff crashed against oracle"
    cat "$ORACLE_LOG"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: every probe family is active on both sides ---"
probe_surfaces() {
    grep -q '^lookup|你好|1|n=1|' "$1" && \
        grep -q '^token|你好|phrase=1|len=2|' "$1" && \
        grep -q '^absent-add=0' "$1" && \
        grep -q '^unload|2|1' "$1" && \
        grep -q '^load|2|1' "$1"
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
