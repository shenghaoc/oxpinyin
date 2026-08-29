#!/usr/bin/env bash
# run-dynamic-adjust-diff.sh — DYNAMIC_ADJUST candidate-ranking differential.
#
# The frozen corpus cannot exercise this bit: it is single-shot at offset 0
# and every frozen option word leaves DYNAMIC_ADJUST (1<<9) clear, so the
# bigram term is absent by construction. This drives the shape that does
# exercise it — parse, guess a sentence, choose, then guess again at the
# offset the choose advanced to, where _get_previous_token returns the
# chosen token and Gates 2 and 3 fire.
#
# NON-VACUITY IS ENFORCED, NOT ASSUMED. Both engines are driven twice, with
# the bit set and clear. If an engine's two outputs are identical the probe
# never reached the feature, and a passing comparison would prove nothing —
# that is a failure (exit 3), not a pass.
#
# Env-gated on the pin-built oracle, mirroring the other differentials:
# PINYIN_ORACLE_PREFIX (default $HOME/.local/opt/pinyin-oracle) must hold the
# pin-verified prefix from tools/oracle/build-oracle.sh. Absent -> skip with
# a diagnostic, exit 0.
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure;
#             2 = divergence; 3 = probe is vacuous.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building dynamic-adjust-diff driver ---"
DRIVER="$(mktemp -d)/dynamic-adjust-diff"
trap 'rm -rf "$(dirname "$DRIVER")"' EXIT
gcc -Wall -Wextra -Werror -O2 -o "$DRIVER" dynamic-adjust-diff.c -ldl

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --locked --manifest-path "$REPO_ROOT/Cargo.toml"
OX_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
OX_DATA="$REPO_ROOT/fixtures/w3"
[ -f "$OX_SO" ] || { echo "fatal: $OX_SO not found"; exit 1; }

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    echo "  NOTE: this differential did NOT run. It is the only probe that"
    echo "  compares DYNAMIC_ADJUST against the pin. The in-tree test"
    echo "  dynamic_adjust_merges_one_row_per_guess_and_lifts_only_the_credited_token"
    echo "  covers the wiring (one merge per guess, sentence_start at offset"
    echo "  0, the term lifting only the credited token) but agrees with"
    echo "  nothing external; a green suite without this run says nothing"
    echo "  about whether the bit matches upstream."
    exit 0
fi

OUT="$(mktemp -d)"
trap 'rm -rf "$(dirname "$DRIVER")" "$OUT"' EXIT

run() { # $1 = so, $2 = datadir, $3 = on|off, $4 = out file
    local userdir="$OUT/user"
    rm -rf "$userdir"
    mkdir -p "$userdir"
    "$DRIVER" "$1" "$2" "$3" "$userdir" > "$4"
}

echo "--- driving oxpinyin (bit on / off) ---"
run "$OX_SO" "$OX_DATA" on  "$OUT/ox-on"
run "$OX_SO" "$OX_DATA" off "$OUT/ox-off"

echo "--- driving the pin (bit on / off) ---"
run "$ORACLE_SO" "$ORACLE_DATA" on  "$OUT/pin-on"
run "$ORACLE_SO" "$ORACLE_DATA" off "$OUT/pin-off"

# Non-vacuity first: if the bit changes nothing, the comparison below is
# meaningless whatever it reports.
for engine in ox pin; do
    if cmp -s "$OUT/$engine-on" "$OUT/$engine-off"; then
        echo "VACUOUS: $engine produced identical output with the bit set and clear."
        echo "  The probe is not reaching DYNAMIC_ADJUST; a passing comparison"
        echo "  would prove nothing. Fix the probe before trusting this gate."
        exit 3
    fi
done
echo "non-vacuity: both engines' output changes with the bit — probe is live"

status=0
for mode in on off; do
    if cmp -s "$OUT/ox-$mode" "$OUT/pin-$mode"; then
        echo "DYNAMIC_ADJUST=$mode: IDENTICAL"
    else
        echo "DYNAMIC_ADJUST=$mode: DIVERGENT"
        diff -u "$OUT/pin-$mode" "$OUT/ox-$mode" | head -40 || true
        status=2
    fi
done
exit $status
