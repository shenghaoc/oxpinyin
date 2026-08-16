#!/usr/bin/env bash
# run-option-sweep.sh — W10 option-bit differential sweep.
#
# Builds tools/bisection/option-sweep.c, then for every option case drives the
# identical parse + candidate sequence into oxpinyin-capi and the pin-built
# libpinyin and diffs parse/aux and top-10 candidate TEXT/ORDER through the
# existing ABI. Tie-order-only (same 10-set, phrase_length tied) is documented
# against the RankKey contract. Any other TEXT/ORDER divergence is a STOP.
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

# Emit `input<TAB>t1|t2|...` for the top-10 candidate texts of each input.
cand_seq_table() {
    awk '
        /^input=/ {
            if (cur != "") print cur "\t" seq
            split($1, a, "=")
            cur = a[2]
            seq = ""
            first = 1
            next
        }
        /^cand\[/ {
            split($0, a, "=")
            text = substr($0, index($0, "=") + 1)
            if (first) { seq = text; first = 0 }
            else seq = seq "|" text
            next
        }
        END { if (cur != "") print cur "\t" seq }
    ' "$1"
}

# Unicode scalar count of $1 (locale-dependent; the driver emits UTF-8).
phrase_len() {
    printf '%s' "$1" | grep -o . | wc -l
}

# Compare one case's candidate TEXT/ORDER. Prints notes to stdout.
# Sets global compare_status to: identical | tie-order | stop
compare_text_order() {
    local oracle_log=$1 capi_log=$2
    local oracle_tbl capi_tbl
    oracle_tbl="$(cand_seq_table "$oracle_log")"
    capi_tbl="$(cand_seq_table "$capi_log")"
    compare_status="identical"

    if [ "$oracle_tbl" = "$capi_tbl" ]; then
        echo "  TEXT/ORDER identical (top-10)"
        return 0
    fi

    local input oracle_seq capi_seq
    while IFS=$'\t' read -r input oracle_seq; do
        capi_seq="$(printf '%s\n' "$capi_tbl" | awk -F '\t' -v k="$input" '$1==k {print $2; exit}')"
        if [ "$oracle_seq" = "$capi_seq" ]; then
            continue
        fi
        local oracle_set capi_set
        oracle_set="$(printf '%s\n' "$oracle_seq" | tr '|' '\n' | sort)"
        capi_set="$(printf '%s\n' "$capi_seq" | tr '|' '\n' | sort)"
        if [ "$oracle_set" != "$capi_set" ]; then
            compare_status="stop"
            echo "  STOP  input=$input  TEXT set differs (not W11 ground; ABI verification)"
            echo "    oracle: ${oracle_seq:-<empty>}"
            echo "    capi:   ${capi_seq:-<empty>}"
            continue
        fi
        # Same set, different order. Rank-key evidence: phrase_length is
        # the first of the three keys and is recoverable from the ABI text.
        # A length mismatch at any swapped position is a first-key ranking
        # divergence, not a collection-order tie.
        local -a o_items c_items
        IFS='|' read -r -a o_items <<< "$oracle_seq"
        IFS='|' read -r -a c_items <<< "$capi_seq"
        local i olen clen length_mismatch=0
        for i in "${!o_items[@]}"; do
            olen="$(phrase_len "${o_items[$i]}")"
            clen="$(phrase_len "${c_items[$i]}")"
            if [ "$olen" != "$clen" ]; then
                length_mismatch=1
                break
            fi
        done
        if [ "$length_mismatch" -eq 1 ]; then
            compare_status="stop"
            echo "  STOP  input=$input  order differs and phrase_length (RankKey 1) mismatches"
            echo "    oracle: $oracle_seq"
            echo "    capi:   $capi_seq"
        else
            if [ "$compare_status" = "identical" ]; then
                compare_status="tie-order"
            fi
            echo "  TIE-ORDER  input=$input  same 10-set, phrase_length tied; span/freq/collection-order not on the ABI"
            echo "    oracle: $oracle_seq"
            echo "    capi:   $capi_seq"
        fi
    done <<< "$oracle_tbl"
}

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
        parse_verdict="PASS"
    else
        parse_verdict="FAIL"
    fi

    compare_status="identical"
    echo "$parse_verdict  $name  0x$(printf '%08x' "$options")  [parse]"
    compare_text_order "$oracle_log" "$capi_log"
    echo "  TEXT/ORDER $compare_status"
    if [ "$parse_verdict" = "FAIL" ]; then
        echo "$parse_diff"
        rm -f "$capi_log" "$oracle_log"
        exit 2
    fi
    if [ "$compare_status" = "stop" ]; then
        echo "STOP: $name candidate TEXT/ORDER diverges beyond a RankKey-1 tie"
        rm -f "$capi_log" "$oracle_log"
        SWEEP_STOP=1
        return 0
    fi
    rm -f "$capi_log" "$oracle_log"
}

SWEEP_STOP=0

run_case baseline "$BASE"
run_case fork-default "$FORK_DEFAULT"

for bit in 21 22 23 24 25 26 27 28; do
    run_case "correct-$bit" $((BASE | (1 << bit)))
done
for bit in 10 11 12 13 14 15 16 17 18 19; do
    run_case "amb-$bit" $((BASE | (1 << bit)))
done

echo ""
if [ "$SWEEP_STOP" -ne 0 ]; then
    echo "option-sweep: STOP — candidate TEXT/ORDER diverged beyond a RankKey-1 tie"
    exit 2
fi
echo "option-sweep: PASS — parse/aux identical; TEXT/ORDER identical or tie-order-only"
