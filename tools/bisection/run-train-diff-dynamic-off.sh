#!/usr/bin/env bash
# run-train-diff-dynamic-off.sh — W10 DYNAMIC_ADJUST off-behaviour check.
#
# Runs one round of the W6-T7 training differential with DYNAMIC_ADJUST
# clear, then diffs the exported (phrase, pinyin, count) rows exactly. The
# pin's `pinyin_train` has no DYNAMIC_ADJUST gate, so the expected off
# behaviour is that both engines still write; this script proves they write
# identically.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
# shellcheck source=tools/bisection/oracle-cell.sh
source ./oracle-cell.sh

echo "--- building train-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o train-diff train-diff.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
oracle_cell_artifact OXPINYIN_CAPI_SO libpinyin_capi.so oxpinyin-capi
CAPI_SO=$OXPINYIN_CAPI_SO
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"
if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 77
fi

DRIVER_ERROR=$(mktemp)
trap 'rm -f "$DRIVER_ERROR"' EXIT
OPTIONS="${TRAINDIFF_OFF_OPTIONS:-0x00000188}"
echo "options: $OPTIONS"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"

if TRAINDIFF_ROUNDS=1 TRAINDIFF_OPTIONS="$OPTIONS" \
    ./train-diff "$CAPI_SO" "$REPO_ROOT/fixtures/w3/$OXPINYIN_CAPI_BACKEND_EXT" > "$CAPI_LOG" 2> "$DRIVER_ERROR"; then
    :
else
    driver_status=$?
    cat "$DRIVER_ERROR" >&2
    echo "FAIL: train-diff failed against oxpinyin-capi (exit $driver_status)"
    cat "$CAPI_LOG"
    exit 1
fi
if TRAINDIFF_ROUNDS=1 TRAINDIFF_OPTIONS="$OPTIONS" \
    ./train-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> "$DRIVER_ERROR"; then
    :
else
    driver_status=$?
    cat "$DRIVER_ERROR" >&2
    echo "FAIL: train-diff failed against the oracle (exit $driver_status)"
    cat "$ORACLE_LOG"
    exit 1
fi

if ! diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") > /dev/null; then
    echo "FAIL: DYNAMIC_ADJUST-off training exports differ"
    diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 2
fi

rows="$(grep -cE '^(bigram|phrase):' "$CAPI_LOG" || true)"
echo "DYNAMIC_ADJUST off: exports identical; wrote $rows rows (pin writes, no no-op)"
rm -f "$CAPI_LOG" "$ORACLE_LOG"

# ── Populated-store candidate TEXT/ORDER (read-side gate) ──────────────
# Same training into both engines, DYNAMIC_ADJUST clear, then top-10 at
# offset 0 and after choosing 你. The cited sites skip the bigram term of
# candidate frequency when the bit is clear; user unigrams stay (phrase
# index). Full system tables on both sides so ranking is the real-frequency
# construction, not the mini-fixture cost fallback.

if [ -n "${OPTION_SWEEP_CAPI_DATA:-${OXPINYIN_SYSTEM_DIR:-}}" ]; then
    CAPI_DATA="${OPTION_SWEEP_CAPI_DATA:-$OXPINYIN_SYSTEM_DIR}"
elif [ -f /tmp/oxpinyin-export/pinyin_index.bin ] \
	&& grep -Fxq "backend=$OXPINYIN_CAPI_BACKEND_EXT" /tmp/oxpinyin-export/datagen-manifest.txt; then
    CAPI_DATA="$(mktemp -d /tmp/traindiff-capi-data-XXXXXX)"
    for table in pinyin_index.bin phrase_index.bin bigram.db; do
        cp "/tmp/oxpinyin-export/$table" "$CAPI_DATA/$table"
    done
    for model_dir in \
        ${PINYIN_MODEL_DIR:+"$PINYIN_MODEL_DIR"} \
        "$REPO_ROOT/target/model20/extracted"; do
        if [ -n "$model_dir" ] && [ -f "$model_dir/interpolation2.text" ]; then
            cp "$model_dir/interpolation2.text" "$CAPI_DATA/interpolation2.text"
            break
        fi
    done
else
    CAPI_DATA=""
fi

if [ -z "$CAPI_DATA" ] || [ ! -f "$CAPI_DATA/interpolation2.text" ]; then
    echo "SKIP: no full capi tables + interpolation2.text; cannot run populated-store candidate dump"
    echo "train-diff dynamic-off: PASS (exports only)"
    exit 77
fi

echo "--- populated-store candidate dump (DYNAMIC_ADJUST clear) ---"
echo "capi data: $CAPI_DATA"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
if TRAINDIFF_ROUNDS=1 TRAINDIFF_OPTIONS="$OPTIONS" TRAINDIFF_DUMP_CANDIDATES=1 \
    TRAINDIFF_REOPEN=1 \
    ./train-diff "$CAPI_SO" "$CAPI_DATA" > "$CAPI_LOG" 2> "$DRIVER_ERROR"; then
    :
else
    driver_status=$?
    cat "$DRIVER_ERROR" >&2
    echo "FAIL: train-diff candidate dump failed against oxpinyin-capi (exit $driver_status)"
    cat "$CAPI_LOG"
    exit 1
fi
if TRAINDIFF_ROUNDS=1 TRAINDIFF_OPTIONS="$OPTIONS" TRAINDIFF_DUMP_CANDIDATES=1 \
    TRAINDIFF_REOPEN=1 \
    ./train-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> "$DRIVER_ERROR"; then
    :
else
    driver_status=$?
    cat "$DRIVER_ERROR" >&2
    echo "FAIL: train-diff candidate dump failed against the oracle (exit $driver_status)"
    cat "$ORACLE_LOG"
    exit 1
fi

# Vacuity guard: the populated phase must actually execute on both sides —
# the reopened probe, the subsequent-session train, and the reopened
# export, not just the in-memory dump.
for side in "$ORACLE_LOG" "$CAPI_LOG"; do
    if ! grep -q '^cand:reopened@0 n=' "$side" || \
       ! grep -q '^train-reopened:1$' "$side" || \
       ! grep -q '^reopen-bigram:' "$side" || \
       ! grep -q '^reopen-phrase:' "$side"; then
        echo "FAIL: populated persistence phase did not execute"
        echo "  reopened cand / train / export lines in $(basename "$side"):"
        grep -cE '^cand:reopened' "$side" || true
        rm -f "$CAPI_LOG" "$ORACLE_LOG"
        exit 1
    fi
done

if ! diff -u <(grep -E '^cand:' "$ORACLE_LOG") <(grep -E '^cand:' "$CAPI_LOG") > /dev/null; then
    echo "DIVERGENCE: populated-store candidate TEXT/ORDER with DYNAMIC_ADJUST clear"
    diff -u <(grep -E '^cand:' "$ORACLE_LOG") <(grep -E '^cand:' "$CAPI_LOG") || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 2
fi
echo "populated-store candidates identical (bit-clear; unigram term ungated, bigram term skipped)"

# The persistence round-trip, per side: the reopened candidate window must
# equal the in-memory one (labels normalized) — the remembered user phrases
# must survive the save/reopen on each engine.
normalize_reopened() {
    grep -E '^cand:reopened' "$1" | \
        sed 's/^cand:reopened@0/cand:nihao@0/; s/^cand:reopened-after-ni/cand:after-ni/'
}
for side in "$ORACLE_LOG" "$CAPI_LOG"; do
    if ! diff -u <(grep -E '^cand:(nihao@0|after-ni)' "$side") \
                  <(normalize_reopened "$side") > /dev/null; then
        echo "DIVERGENCE: reopened candidate window differs from the in-memory one"
        diff -u <(grep -E '^cand:(nihao@0|after-ni)' "$side") \
                <(normalize_reopened "$side") || true
        rm -f "$CAPI_LOG" "$ORACLE_LOG"
        exit 2
    fi
done
echo "reopened candidate window equals the in-memory one (both sides)"

# The persisted-state export + the subsequent-session training attempt:
# cross-engine, sorted (the store's iteration order is not a contract).
if ! diff -u <(grep -E '^reopen-(phrase|bigram):' "$ORACLE_LOG" | sort) \
             <(grep -E '^reopen-(phrase|bigram):' "$CAPI_LOG" | sort) > /dev/null; then
    echo "DIVERGENCE: reopened export triples differ (persistence / subsequent session)"
    diff -u <(grep -E '^reopen-(phrase|bigram):' "$ORACLE_LOG" | sort) \
            <(grep -E '^reopen-(phrase|bigram):' "$CAPI_LOG" | sort) || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 2
fi
echo "reopened export identical (subsequent-session train included):"
grep -E '^reopen-(phrase|bigram):' "$CAPI_LOG" | sort
grep -E '^cand:' "$CAPI_LOG"
rm -f "$CAPI_LOG" "$ORACLE_LOG"
echo "train-diff dynamic-off: PASS"
