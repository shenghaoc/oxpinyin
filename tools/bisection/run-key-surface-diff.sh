#!/usr/bin/env bash
# run-key-surface-diff.sh — Tier-A ABI parity differential: single-key
# parsing (`pinyin_parse_full_pinyin` / `_double_` / `_chewing`) and the
# `ChewingKey` display getters, plus the `pinyin_get_context` and
# addon-unload contracts.
#
# Drives the scripted C-ABI sequence (key-surface-diff.c) into
# libpinyin_capi.so and the pin-built libpinyin.so and diffs the FULL
# logs. The two-byte key logged per probe is the byte-identity check of
# the packed `ChewingKey` bitfield (D1's cross-engine layout
# verification; the driver TU additionally static-asserts the mirror
# sizes).
#
# Option profiles: the parity word `0x18a`, `+USE_TONE` (0x1aa),
# `+USE_TONE|FORCE_TONE` (0x1ea), `FORCE_TONE` alone (0x1ca) — the D3
# FORCE_TONE-law differential for the double/zhuyin single-key seams.
# Scheme sweeps: double pinyin 1..6, chewing 1..6+8+9 (the pins abort on
# double 30 and zhuyin 7 — the recorded #109 contract slots).
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a probe surface is inactive (the run would have compared nothing).
#
# Env:
#   PINYIN_ORACLE_PREFIX  pin-built oracle prefix
#                         (default ~/.local/opt/pinyin-oracle)
#   KEY_SYSTEM            real-unigram capi system dir (5 redb/text
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
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
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

echo "--- cc key-surface-diff.c ---"
DRIVER="$REPO_ROOT/target/key-surface-diff"
cc -O2 -o "$DRIVER" key-surface-diff.c -ldl || exit 1

SYSTEM="${KEY_SYSTEM:-$REPO_ROOT/target/datagen/redb}"
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
    echo "FAIL: key-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
if ! "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG"; then
    echo "FAIL: key-surface-diff crashed against oracle"
    cat "$ORACLE_LOG"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: every probe family is active on both sides ---"
probe_surfaces() {
    grep -q '^full|0x18a|ni|1|' "$1" && \
        grep -q '^double|2|0x18a|ni|1|' "$1" && \
        grep -q '^chewing|1|0x18a|18|1|' "$1" && \
        grep -q '^render|full|zhang|zhuyin|1|' "$1" && \
        grep -q '^context|match|1' "$1" && \
        grep -q '^unload_addon|15|1' "$1"
}
if ! probe_surfaces "$CAPI_LOG" || ! probe_surfaces "$ORACLE_LOG"; then
    echo "FAIL: a probe family is inactive (full/double/chewing/render/context/addon)"
    echo "  An inactive family means that surface compared nothing."
    exit 3
fi

if diff -u "$ORACLE_LOG" "$CAPI_LOG"; then
    echo "IDENTICAL: $(wc -l < "$CAPI_LOG") probe lines agree with the pin"
else
    echo
    echo "DIVERGENCE: the logs differ (above)"
    exit 2
fi
