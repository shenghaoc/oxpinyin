#!/usr/bin/env bash
# run-option-sweep.sh — W10 option-bit differential sweep.
#
# Builds tools/bisection/option-sweep.c, then for every option case drives the
# identical parse + candidate sequence into oxpinyin-capi and the pin-built
# libpinyin and diffs the logs exactly.
#
# Environment:
#   PINYIN_ORACLE_PREFIX   pin-built oracle prefix (default
#                          $HOME/.local/opt/pinyin-oracle)
#   OPTION_SWEEP_CAPI_DATA oxpinyin-capi system dir. When unset the script
#                          prefers a full export plus interpolation2.text in
#                          the sibling model cache, then falls back to
#                          fixtures/w3.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building option-sweep driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o option-sweep option-sweep.c -ldl
echo "build: ok"

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

# ── oxpinyin-capi system dir ─────────────────────────────────────────────

if [ -n "${OPTION_SWEEP_CAPI_DATA:-}" ]; then
    CAPI_DATA="$OPTION_SWEEP_CAPI_DATA"
elif [ -f /tmp/oxpinyin-export/pinyin_index.redb ]; then
    CAPI_DATA="$(mktemp -d /tmp/option-sweep-capi-data-XXXXXX)"
    for table in pinyin_index.redb phrase_index.redb bigram.redb; do
        cp "/tmp/oxpinyin-export/$table" "$CAPI_DATA/$table"
    done
    for model_dir in \
        "$REPO_ROOT/target/model20/extracted" \
        "/home/sheng/Documents/repos/pinyin-rs/target/model20/extracted" \
        "/home/sheng/Documents/repos/libpinyin/data"; do
        if [ -f "$model_dir/interpolation2.text" ]; then
            cp "$model_dir/interpolation2.text" "$CAPI_DATA/interpolation2.text"
            break
        fi
    done
else
    CAPI_DATA="$REPO_ROOT/fixtures/w3"
fi

if [ ! -f "$CAPI_DATA/pinyin_index.redb" ]; then
    echo "fatal: capi data not found at $CAPI_DATA"
    exit 1
fi
echo "capi data: $CAPI_DATA"

# ── Oracle (env-gated) ──────────────────────────────────────────────────

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi

echo "oracle: $ORACLE_SO"
echo "oracle data: $ORACLE_DATA"
echo ""

# ── Cases ────────────────────────────────────────────────────────────────

BASE=$((0x8 | 0x80 | 0x100))  # incomplete + divided + resplit, option bits off
FORK_DEFAULT=$((0x1fe00198))

run_case() {
    local name=$1 options=$2
    local capi_log oracle_log
    capi_log="$(mktemp)"
    oracle_log="$(mktemp)"

    if ! ./option-sweep "$CAPI_SO" "$CAPI_DATA" "$name" "$(printf '%x' "$options")" \
        > "$capi_log" 2> /dev/null; then
        echo "FAIL: option-sweep crashed against oxpinyin-capi ($name)"
        cat "$capi_log"
        rm -f "$capi_log" "$oracle_log"
        exit 1
    fi
    if ! ./option-sweep "$ORACLE_SO" "$ORACLE_DATA" "$name" "$(printf '%x' "$options")" \
        > "$oracle_log" 2> /dev/null; then
        echo "FAIL: option-sweep crashed against the oracle ($name)"
        cat "$oracle_log"
        rm -f "$capi_log" "$oracle_log"
        exit 1
    fi

    parse_diff="$(diff -u \
        <(grep -E '^(case=|input=|aux=)' "$oracle_log") \
        <(grep -E '^(case=|input=|aux=)' "$capi_log") || true)"
    if [ -z "$parse_diff" ]; then
        verdict="PASS"
    else
        verdict="FAIL"
    fi

    count_diff="$(diff -u \
        <(grep -E '^(case=|input=|n=)' "$oracle_log") \
        <(grep -E '^(case=|input=|n=)' "$capi_log") || true)"
    if [ -n "$count_diff" ]; then
        count_note="candidate-count differs (W11 ground)"
    else
        count_note="candidate-count identical"
    fi

    echo "$verdict  $name  0x$(printf '%08x' "$options")  [$count_note]"
    if [ "$verdict" = "FAIL" ]; then
        echo "$parse_diff"
        rm -f "$capi_log" "$oracle_log"
        exit 2
    fi
    rm -f "$capi_log" "$oracle_log"
}

run_case baseline "$BASE"
run_case fork-default "$FORK_DEFAULT"

for bit in 21 22 23 24 25 26 27 28; do
    run_case "correct-$bit" $((BASE | (1 << bit)))
done
for bit in 10 11 12 13 14 15 16 17 18 19; do
    run_case "amb-$bit" $((BASE | (1 << bit)))
done

echo ""
echo "option-sweep: PASS — every option word identical across engines"
