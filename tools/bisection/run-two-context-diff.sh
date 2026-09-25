#!/usr/bin/env bash
# run-two-context-diff.sh — two contexts on one user dir in one process,
# against the pin (#538).
#
# The pin holds nothing per user dir beyond each context: a second
# pinyin_init / zhuyin_init on a dir another live context has open reads
# the profile afresh into its own memory. It does not see the first
# context's unsaved learning, each context's save answers for its own
# modifications, libpinyin raises and lowers the open counter once per
# init and fini on each context's own copy, and when both save, the
# later save's files are the profile. tools/bisection/two-context-diff.c
# drives three scenarios into the pin-built libraries and into oxpinyin's
# and this runner diffs the logs byte for byte:
#
#   one-learns     A learns; each context's view; save B, save A; fini A,
#                  fini B.
#   fini-reversed  the same with fini B first — the counter each fini
#                  writes back depends on the order, as upstream's does.
#   both-learn     A and B learn different items and both save; a third
#                  context shows which one the profile kept.
#
# Both sides open the pin prefix's own data directory with a fresh user
# dir per scenario; oxpinyin's libraries must be built with the store
# backend that matches the prefix's --with-dbm.
#
# Usage: run-two-context-diff.sh
#
# Env:
#   TWO_CONTEXT_ORACLE_PREFIX  (required) a tools/oracle/build-oracle.sh
#                              prefix configured with --enable-libzhuyin
#                              (as for run-open-counter-diff.sh). No
#                              default: a missing oracle fails the run.
#   TWO_CONTEXT_PINYIN_SO      oxpinyin's libpinyin (default
#                              $REPO_ROOT/target/debug/libpinyin_capi.so)
#   TWO_CONTEXT_ZHUYIN_SO      oxpinyin's libzhuyin (default
#                              $REPO_ROOT/target/debug/libzhuyin_capi.so)
#   TWO_CONTEXT_LIBS           "pinyin zhuyin" (default) or either one
#   TWO_CONTEXT_OUT            directory the logs are kept in (default: a
#                              temp dir, removed on exit)
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a required input is missing.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

PREFIX="${TWO_CONTEXT_ORACLE_PREFIX:-}"
if [[ -z "$PREFIX" || ! -d "$PREFIX" ]]; then
    echo "missing input: TWO_CONTEXT_ORACLE_PREFIX is unset or not a directory" >&2
    echo "  build it with tools/oracle/build-oracle.sh, configure line plus --enable-libzhuyin" >&2
    exit 3
fi
if ! grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$PREFIX/oracle-pin.txt" 2>/dev/null; then
    echo "missing input: $PREFIX is not a prefix of the 074a2219 pin (oracle-pin.txt)" >&2
    exit 3
fi
DATA="$PREFIX/lib/libpinyin/data"
declare -A ORACLE_SO=([pinyin]="$PREFIX/lib/libpinyin.so.15" [zhuyin]="$PREFIX/lib/libzhuyin.so.15")
declare -A OX_SO=(
    [pinyin]="${TWO_CONTEXT_PINYIN_SO:-$REPO_ROOT/target/debug/libpinyin_capi.so}"
    [zhuyin]="${TWO_CONTEXT_ZHUYIN_SO:-$REPO_ROOT/target/debug/libzhuyin_capi.so}"
)
read -r -a LIBS <<< "${TWO_CONTEXT_LIBS:-pinyin zhuyin}"
SCENARIOS=(one-learns fini-reversed both-learn)

[[ -f "$DATA/table.conf" ]] || { echo "missing input: $DATA/table.conf" >&2; exit 3; }
for lib in "${LIBS[@]}"; do
    [[ -n "${ORACLE_SO[$lib]:-}" ]] || { echo "unknown library: $lib" >&2; exit 3; }
    for so in "${ORACLE_SO[$lib]}" "${OX_SO[$lib]}"; do
        [[ -f "$so" ]] || { echo "missing input: $so" >&2; exit 3; }
    done
done

if [[ -n "${TWO_CONTEXT_OUT:-}" ]]; then
    OUT="$TWO_CONTEXT_OUT"
    mkdir -p "$OUT"
    WORK="$(mktemp -d)"
    trap 'rm -rf "$WORK"' EXIT
else
    OUT="$(mktemp -d)"
    WORK="$OUT"
    trap 'rm -rf "$OUT"' EXIT
fi

echo "--- building two-context-diff driver ---"
DRIVER="$WORK/two-context-diff"
# shellcheck disable=SC2046  # pkg-config's flags are meant to split.
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$DRIVER" "$SCRIPT_DIR/two-context-diff.c" \
    $(pkg-config --cflags --libs glib-2.0) -ldl
echo "build: ok"

run_side() {
    local lib=$1 so=$2 scenario=$3 log=$4
    local user="$WORK/user-$(basename "$log" .log)"
    local tmp="$WORK/tmp-$(basename "$log" .log)"
    rm -rf "$user" "$tmp"
    mkdir -p "$user" "$tmp"
    if ! TMPDIR="$tmp" "$DRIVER" "$lib" "$so" "$DATA" "$user" "$scenario" \
        > "$log" 2> "$log.stderr"; then
        echo "FAIL: $scenario on $so" >&2
        cat "$log.stderr" >&2
        return 1
    fi
}

status=0
for lib in "${LIBS[@]}"; do
    for scenario in "${SCENARIOS[@]}"; do
        oracle_log="$OUT/$lib-$scenario-oracle.log"
        ox_log="$OUT/$lib-$scenario-oxpinyin.log"
        run_side "$lib" "${ORACLE_SO[$lib]}" "$scenario" "$oracle_log" || exit 1
        run_side "$lib" "${OX_SO[$lib]}" "$scenario" "$ox_log" || exit 1
        # Vacuity guard: both sides must have learned something the
        # comparison can see.
        if ! grep -q ' ok$' "$oracle_log" || ! grep -q ' ok$' "$ox_log"; then
            echo "FAIL: $lib/$scenario learned nothing on a side; the comparison would be vacuous" >&2
            exit 1
        fi
        diff_rc=0
        diff -u "$oracle_log" "$ox_log" > "$OUT/$lib-$scenario.diff" || diff_rc=$?
        if ((diff_rc > 1)); then
            echo "FAIL: could not compare $oracle_log and $ox_log" >&2
            exit 1
        fi
        if ((diff_rc == 0)); then
            echo "$lib/$scenario: IDENTICAL ($(wc -l < "$oracle_log") lines)"
        else
            echo "$lib/$scenario: DIVERGED"
            cat "$OUT/$lib-$scenario.diff"
            status=2
        fi
    done
done

if ((status == 0)); then
    echo "RESULT: two contexts on one user dir behave as the pin's do"
else
    echo "RESULT: at least one two-context scenario DIVERGED from the pin"
fi
exit "$status"
