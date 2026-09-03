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
#                          takes OXPINYIN_SYSTEM_DIR, then a full export plus
#                          interpolation2.text from the sibling model cache,
#                          then the conventional build locations. Unresolvable
#                          is FATAL, not a silent fixtures/w3 run -- see
#                          system-dir.sh. A directory that resolves but lacks
#                          any of the four required tables is equally FATAL,
#                          whichever of the three sources it came from.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=tools/bisection/system-dir.sh
. ./system-dir.sh
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

# Three sources, in the order system-dir.sh documents: OPTION_SWEEP_CAPI_DATA,
# then OXPINYIN_SYSTEM_DIR, then the conventional locations. The
# /tmp/oxpinyin-export cache gets a branch of its own only because it is the
# one that needs interpolation2.text grafted on from the model cache -- and
# that graft must not outrank a directory the operator named, so it stands
# down when OXPINYIN_SYSTEM_DIR is set and lets resolve_system_dir answer.
#
# Every branch validates. The first and third check an explicit path through
# system_dir_require_complete -- directly here, inside resolve_system_dir
# there -- while the second checks the two directories it assembles from,
# since the export cache never holds interpolation2.text. The one exception
# is deliberate: the opted-in mini fixture comes back from resolve_system_dir
# unvalidated, being incomplete on purpose.
#
# An incomplete directory scored against a real oracle reports DIVERGENCE
# from the data mismatch, the failure this whole file exists to stop. Every
# refusal here exits 3: 2 is this script's divergence code, 1 a build failure.

if [ -n "${OPTION_SWEEP_CAPI_DATA:-}" ]; then
    CAPI_DATA="$OPTION_SWEEP_CAPI_DATA"
    system_dir_require_complete "$CAPI_DATA" OPTION_SWEEP_CAPI_DATA option-sweep
elif [ -z "${OXPINYIN_SYSTEM_DIR:-}" ] &&
     export_ext="$(system_dir_detect_ext /tmp/oxpinyin-export)"; then
    # Resolve both sources before creating or copying anything. The
    # detector guarantees the three core tables in one extension (the
    # capi's, see system-dir.sh), so what remains unchecked is
    # interpolation2.text; letting a cp fail under `set -e` would exit 1
    # with a bare `cp: cannot stat` -- when this is the same incomplete-data
    # refusal every other branch answers with 3. interpolation2.text is not
    # an export-cache file at all: the export never holds it, which is the
    # reason this branch exists, so it is resolved from the model cache and
    # reported under its own heading.
    #
    # Reported together rather than through system_dir_require_complete,
    # whose provenance line lists the variables -- not the two directories
    # that actually build this one.
    missing_tables=()
    for table in pinyin_index phrase_index bigram; do
        [ -f "/tmp/oxpinyin-export/$table.$export_ext" ] || missing_tables+=("$table.$export_ext")
    done
    interp_src=
    for model_dir in \
        ${PINYIN_MODEL_DIR:+"$PINYIN_MODEL_DIR"} \
        "$REPO_ROOT/target/model20/extracted"; do
        if [ -n "$model_dir" ] && [ -f "$model_dir/interpolation2.text" ]; then
            interp_src="$model_dir/interpolation2.text"
            break
        fi
    done
    if [ ${#missing_tables[@]} -ne 0 ] || [ -z "$interp_src" ]; then
        {
            echo "fatal: option-sweep: cannot assemble a system directory."
            if [ ${#missing_tables[@]} -ne 0 ]; then
                echo ""
                echo "Missing from /tmp/oxpinyin-export:"
                printf '  %s\n' "${missing_tables[@]}"
            fi
            if [ -z "$interp_src" ]; then
                echo ""
                echo "No interpolation2.text in:"
                echo "  \$PINYIN_MODEL_DIR"
                echo "  $REPO_ROOT/target/model20/extracted"
            fi
            echo ""
            echo "All four are required: the three tables plus interpolation2.text,"
            echo "whose real unigrams are what the oracle scores against. Short of"
            echo "them the comparison is flat-export unigrams versus the pin's real"
            echo "ones -- a data mismatch that reports as a divergence. Fetch the"
            echo "model with tools/model/fetch-model.sh, or point"
            echo "OPTION_SWEEP_CAPI_DATA at a directory that already holds all four."
        } >&2
        exit 3
    fi
    CAPI_DATA="$(mktemp -d /tmp/option-sweep-capi-data-XXXXXX)"
    system_dir_copy_tables /tmp/oxpinyin-export "$CAPI_DATA"
    cp "$interp_src" "$CAPI_DATA/interpolation2.text"
else
    # No explicit dir and no export cache: resolve or refuse. Falling back
    # to fixtures/w3 here used to make a real-oracle run report DIVERGENCE
    # from a data mismatch.
    CAPI_DATA="$(resolve_system_dir OPTION_SWEEP_CAPI_DATA option-sweep)"
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

# W12 corpus-tail residuals (#98 control): these TEXT-set tails diverge
# under ALL-BITS-OFF (or share a native canonical that does). Not W10.
# See docs/findings/option-bits.md.
is_w12_residual() {
    case $1 in
        cang|sang|lve|lue|agn|amg|ang) return 0 ;;
        *) return 1 ;;
    esac
}

# Compare one case's candidate TEXT/ORDER. Prints notes to stdout.
# Sets global compare_status to: identical | tie-order | w12-residual | stop
# $3 is the case name.
compare_text_order() {
    local oracle_log=$1 capi_log=$2 case_name=$3
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
            if is_w12_residual "$input"; then
                if [ "$compare_status" = "identical" ]; then
                    compare_status="w12-residual"
                fi
                echo "  W12  input=$input  TEXT-set tail; all-off residual, not W10 (docs/findings/option-bits.md)"
                echo "    oracle: ${oracle_seq:-<empty>}"
                echo "    capi:   ${capi_seq:-<empty>}"
            else
                compare_status="stop"
                echo "  STOP  input=$input  TEXT set differs (not W11 ground; ABI verification)"
                echo "    oracle: ${oracle_seq:-<empty>}"
                echo "    capi:   ${capi_seq:-<empty>}"
            fi
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
    compare_text_order "$oracle_log" "$capi_log" "$name"
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
# Initials then finals compose (phonetic_key_matrix.cpp:238-306).
run_case "amb-chain" $((BASE | (1 << 10) | (1 << 17)))

echo ""
if [ "$SWEEP_STOP" -ne 0 ]; then
    echo "option-sweep: STOP — candidate TEXT/ORDER diverged beyond a RankKey-1 tie"
    exit 2
fi
echo "option-sweep: PASS — parse/aux identical; TEXT/ORDER identical, tie-order-only, or W12-excluded"
