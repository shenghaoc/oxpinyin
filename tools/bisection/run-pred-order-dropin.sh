#!/usr/bin/env bash
# run-pred-order-dropin.sh — the R1 drop-in differential, dual-dlopen mode.
#
# Loads two libpinyin-ABI libraries in turn — ORACLE_LIB (the distro's real
# libpinyin) and SUBJECT_LIB (the oxpinyin build) — runs the same eight
# predicted-prefix probes over the SAME system data directory through each,
# and diffs the PREDICTED_PREFIX rows.
#
# Environment (all required):
#   ORACLE_LIB          the real libpinyin .so (e.g. /usr/lib64/libpinyin.so.15)
#   SUBJECT_LIB         oxpinyin's libpinyin_capi.so
#   OXPINYIN_SYSTEM_DIR the libpinyin data dir both sides open
#
# The comparison strips the absolute [index] from each row before diffing:
# the subject's _with_punctuations API prepends punctuation rows (a
# different candidate type, filtered out), which shifts absolute indices
# while leaving the PREDICTED_PREFIX order — the R1 subject — intact. The
# driver itself falls back to plain pinyin_guess_predicted_candidates on a
# library too old for the punctuations variant (libpinyin < 2.11).
#
# No sudo, no ldconfig, no system changes: both .so files are dlopened at
# the paths given.
#
# Exit: 0 IDENTICAL; 1 DIVERGE (count reported); 2 setup failure.

set -uo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

: "${ORACLE_LIB:?set ORACLE_LIB to the real libpinyin .so}"
: "${SUBJECT_LIB:?set SUBJECT_LIB to the oxpinyin libpinyin_capi.so}"
: "${OXPINYIN_SYSTEM_DIR:?set OXPINYIN_SYSTEM_DIR to the libpinyin data dir}"

for f in "$ORACLE_LIB" "$SUBJECT_LIB"; do
    [ -f "$f" ] || { echo "missing library: $f" >&2; exit 2; }
done
[ -d "$OXPINYIN_SYSTEM_DIR" ] || { echo "missing data dir: $OXPINYIN_SYSTEM_DIR" >&2; exit 2; }

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

DRIVER="$WORK/pred-order-diff"
"${CC:-cc}" -O2 -Wall -o "$DRIVER" "$REPO_ROOT/tools/bisection/pred-order-diff.c" -ldl \
    || { echo "driver build failed" >&2; exit 2; }

dump() { # $1 = library, $2 = out file, $3 = label
    if ! "$DRIVER" "$1" "$OXPINYIN_SYSTEM_DIR" > "$2" 2> "$WORK/$3.err"; then
        echo "FAIL: $3 produced no dump (init or accessor failure):" >&2
        sed 's/^/  /' "$WORK/$3.err" >&2
        exit 2
    fi
    if ! [ -s "$2" ]; then
        echo "FAIL: $3 produced zero candidates over $OXPINYIN_SYSTEM_DIR" >&2
        sed 's/^/  /' "$WORK/$3.err" >&2
        exit 2
    fi
}

dump "$ORACLE_LIB"  "$WORK/oracle.raw"  oracle
dump "$SUBJECT_LIB" "$WORK/subject.raw" subject

# Strip the absolute [index]; row ORDER is the comparison.
sed 's/\[[0-9]*\]//' "$WORK/oracle.raw"  > "$WORK/oracle.norm"
sed 's/\[[0-9]*\]//' "$WORK/subject.raw" > "$WORK/subject.norm"

echo "oracle rows:  $(wc -l < "$WORK/oracle.norm")  ($ORACLE_LIB)"
echo "subject rows: $(wc -l < "$WORK/subject.norm")  ($SUBJECT_LIB)"

command -v diff >/dev/null || { echo "diff(1) is required" >&2; exit 2; }
diff -u "$WORK/oracle.norm" "$WORK/subject.norm" > "$WORK/rows.diff"
case $? in
    0)
        echo "R1: IDENTICAL — every PREDICTED_PREFIX row matches in order"
        exit 0
        ;;
    1)
        count=$(grep -c '^[+-]pred-' "$WORK/rows.diff" || true)
        echo "R1: DIVERGE — $count differing row lines"
        # Attribution: identical row SETS mean a pure ordering divergence —
        # the pin emits its DBM's physical bucket order, oxpinyin the
        # defined text-ascending order (upstream-divergences.md,
        # "Predicted-candidate tie order").
        sort "$WORK/oracle.norm" > "$WORK/oracle.sorted"
        sort "$WORK/subject.norm" > "$WORK/subject.sorted"
        if cmp -s "$WORK/oracle.sorted" "$WORK/subject.sorted"; then
            echo "attribution: row SETS are identical — ORDER-ONLY divergence"
            echo "  (pin physical/bucket order vs oxpinyin's defined text-ascending order)"
        else
            echo "attribution: row sets DIFFER — first set differences:"
            diff "$WORK/oracle.sorted" "$WORK/subject.sorted" | head -20
        fi
        echo "first 40 diff lines:"
        head -40 "$WORK/rows.diff"
        exit 1
        ;;
    *)
        echo "diff failed to run" >&2
        exit 2
        ;;
esac
