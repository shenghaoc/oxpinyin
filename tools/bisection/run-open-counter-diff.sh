#!/usr/bin/env bash
# run-open-counter-diff.sh — the user.conf open-counter differential (#523).
#
# The pin's open counter lives across launches: pinyin_init adds one and
# writes it (pinyin.cpp:185-187), pinyin_save and pinyin_fini write it
# back (:1143, :1194-1200 — fini first lowers it: `counter > 1 ? counter - 1
# : 0`), and an init that reads a value above OPEN_COUNTER_LIMIT (6,
# table_info.cpp:32, :409-410) wipes the profile. libzhuyin never raises it
# (zhuyin.cpp:126-176). This runner drives the same launch sequences into
# the pin-built libraries and into oxpinyin's, one process per launch
# (tools/bisection/open-counter-diff.c), and diffs the two logs byte for
# byte: every launch's learned rows, then user.conf and the user dir's
# file inventory as that launch left them.
#
# Two protocols, per library (pinyin, zhuyin):
#
#   cycle  OPEN_COUNTER_CYCLES (default 10) clean launches: init -> learn
#          one phrase -> train -> save -> fini. The pin keeps every phrase
#          and returns the counter to 0 after each fini.
#   crash  two clean launches to seed the profile, then
#          OPEN_COUNTER_CRASHES (default 10) launches killed between init
#          and fini (SIGKILL: no save, no fini), then one clean launch.
#          The pin's counter stays raised by every killed launch and the
#          profile is wiped by the launch that reads a value above the
#          limit — the 8th consecutive killed launch from a counter of 0.
#
# Both sides open the pin prefix's own data directory (the drop-in
# contract) with a fresh user dir each; the oxpinyin libraries must be
# built with the store backend that matches the prefix's --with-dbm.
#
# Usage: run-open-counter-diff.sh
#
# Env:
#   OPEN_COUNTER_ORACLE_PREFIX  (required) a tools/oracle/build-oracle.sh
#                               prefix configured with --enable-libzhuyin:
#                               lib/libpinyin.so.15, lib/libzhuyin.so.15,
#                               lib/libpinyin/data. No default: a missing
#                               oracle fails the run, never skips it.
#   OPEN_COUNTER_PINYIN_SO      oxpinyin's libpinyin (default
#                               $REPO_ROOT/target/debug/libpinyin_capi.so)
#   OPEN_COUNTER_ZHUYIN_SO      oxpinyin's libzhuyin (default
#                               $REPO_ROOT/target/debug/libzhuyin_capi.so)
#   OPEN_COUNTER_LIBS           "pinyin zhuyin" (default) or either one
#   OPEN_COUNTER_CYCLES         clean launches in the cycle protocol (10)
#   OPEN_COUNTER_CRASHES        killed launches in the crash protocol (10)
#   OPEN_COUNTER_OUT            directory the logs are kept in (default: a
#                               temp dir, removed on exit)
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a required input is missing.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

PREFIX="${OPEN_COUNTER_ORACLE_PREFIX:-}"
if [[ -z "$PREFIX" || ! -d "$PREFIX" ]]; then
    echo "missing input: OPEN_COUNTER_ORACLE_PREFIX is unset or not a directory" >&2
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
    [pinyin]="${OPEN_COUNTER_PINYIN_SO:-$REPO_ROOT/target/debug/libpinyin_capi.so}"
    [zhuyin]="${OPEN_COUNTER_ZHUYIN_SO:-$REPO_ROOT/target/debug/libzhuyin_capi.so}"
)
read -r -a LIBS <<< "${OPEN_COUNTER_LIBS:-pinyin zhuyin}"
CYCLES="${OPEN_COUNTER_CYCLES:-10}"
CRASHES="${OPEN_COUNTER_CRASHES:-10}"

[[ -f "$DATA/table.conf" ]] || { echo "missing input: $DATA/table.conf" >&2; exit 3; }
for lib in "${LIBS[@]}"; do
    [[ -n "${ORACLE_SO[$lib]:-}" ]] || { echo "unknown library: $lib" >&2; exit 3; }
    for so in "${ORACLE_SO[$lib]}" "${OX_SO[$lib]}"; do
        [[ -f "$so" ]] || { echo "missing input: $so" >&2; exit 3; }
    done
done

if [[ -n "${OPEN_COUNTER_OUT:-}" ]]; then
    OUT="$OPEN_COUNTER_OUT"
    mkdir -p "$OUT"
    WORK="$(mktemp -d)"
    trap 'rm -rf "$WORK"' EXIT
else
    OUT="$(mktemp -d)"
    WORK="$OUT"
    trap 'rm -rf "$OUT"' EXIT
fi

echo "--- building open-counter-diff driver ---"
DRIVER="$WORK/open-counter-diff"
# shellcheck disable=SC2046  # pkg-config's flags are meant to split.
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$DRIVER" "$SCRIPT_DIR/open-counter-diff.c" \
    $(pkg-config --cflags --libs glib-2.0) -ldl
echo "build: ok"

# user.conf and the file inventory, as a launch left them. The counter is
# compared through the whole file, byte for byte (its length too).
snapshot() {
    local dir=$1
    if [[ -f "$dir/user.conf" ]]; then
        echo "user.conf: $(wc -c < "$dir/user.conf") bytes"
        sed 's/^/conf: /' "$dir/user.conf"
    else
        echo "user.conf: absent"
    fi
    echo "files: $(find "$dir" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort | tr '\n' ' ')"
}

# One launch: the driver's stdout, how the process ended, the snapshot.
# Each side runs with its own TMPDIR so a killed oxpinyin launch's
# scratch session cannot land in, or be counted against, the other side.
launch() {
    local lib=$1 so=$2 user=$3 n=$4 mode=$5 log=$6 err=$7 tmp=$8
    local rc=0
    # A subshell of its own reaps the driver, so the job-status line the
    # shell prints for a SIGKILLed child lands in the stderr log; the
    # launch's fate is read from the exit status alone.
    (
        TMPDIR="$tmp" "$DRIVER" "$lib" "$so" "$DATA" "$user" "$n" "$mode" >> "$log"
        exit $?
    ) 2>> "$err" || rc=$?
    case "$mode:$rc" in
    cycle:0) echo "exit: 0" >> "$log" ;;
    crash:137) echo "exit: SIGKILL" >> "$log" ;;
    *)
        echo "FAIL: launch $n ($mode) of $so exited $rc" >&2
        cat "$err" >&2
        return 1
        ;;
    esac
    snapshot "$user" >> "$log"
}

run_protocol() {
    local lib=$1 so=$2 protocol=$3 log=$4
    local user="$WORK/user-$(basename "$log" .log)"
    local tmp="$WORK/tmp-$(basename "$log" .log)"
    local err="$log.stderr"
    rm -rf "$user" "$tmp"
    mkdir -p "$user" "$tmp"
    : > "$log"
    : > "$err"
    local n=0
    case "$protocol" in
    cycle)
        for ((i = 0; i < CYCLES; i++)); do
            n=$((n + 1)); launch "$lib" "$so" "$user" "$n" cycle "$log" "$err" "$tmp" || return 1
        done
        ;;
    crash)
        for mode in cycle cycle $(for ((i = 0; i < CRASHES; i++)); do echo crash; done) cycle; do
            n=$((n + 1)); launch "$lib" "$so" "$user" "$n" "$mode" "$log" "$err" "$tmp" || return 1
        done
        ;;
    esac
}

# The per-launch trajectory, for the report. What a launch found learned
# at init: its phrase rows (pinyin's user dictionary) plus the training
# readings — target (pinyin) or rank (zhuyin) rows — that differ from
# launch 1's, which read a fresh dir and so the system state. A launch
# that found less than the previous launch left behind is a wipe. The
# counter is the one user.conf held after the launch.
summarize() {
    awk -F'\t' '
        function key() { return $2 "/" $3 "/" ($2 == "T" ? $4 : "") }
        function val() { return $2 == "T" ? $5 : $4 "|" $5 }
        /^launch: / { split($0, a, " "); n = a[2]; mode[n] = a[3]; p_init[n] = 0; t_init[n] = 0; p_saved[n] = 0; t_saved[n] = 0 }
        /^phrases@init: / { split($0, a, " "); p_init[n] = a[2] }
        /^phrases@saved: / { split($0, a, " "); p_saved[n] = a[2] }
        $1 == "row@init" && ($2 == "T" || $2 == "R") {
            if (n == 1) base[key()] = val(); else if (val() != base[key()]) t_init[n]++
        }
        $1 == "row@saved" && ($2 == "T" || $2 == "R") { if (val() != base[key()]) t_saved[n]++ }
        /^conf: open counter:/ { split($0, a, ":"); counter[n] = a[3] }
        /^user.conf: absent/ { counter[n] = "-" }
        END {
            left = 0
            for (i = 1; i <= n; i++) {
                found = p_init[i] + t_init[i]
                wipe = (i > 1 && found < left) ? "  WIPE" : ""
                printf "  launch %2d %-5s learned@init=%-2d (phrases %d, trained %d)  counter=%s%s\n",
                    i, mode[i], found, p_init[i], t_init[i], counter[i], wipe
                left = (mode[i] == "cycle") ? p_saved[i] + t_saved[i] : found
            }
        }' "$1"
}

status=0
for lib in "${LIBS[@]}"; do
    for protocol in cycle crash; do
        echo "=== $lib / $protocol ==="
        oracle_log="$OUT/$lib-$protocol-oracle.log"
        ox_log="$OUT/$lib-$protocol-oxpinyin.log"
        run_protocol "$lib" "${ORACLE_SO[$lib]}" "$protocol" "$oracle_log" || exit 1
        run_protocol "$lib" "${OX_SO[$lib]}" "$protocol" "$ox_log" || exit 1
        # A vacuous comparison is a failure: both sides must have launched,
        # and a clean launch must have learned and saved.
        if ! grep -q '^save: 1$' "$oracle_log" || ! grep -q '^save: 1$' "$ox_log"; then
            echo "FAIL: a side never saved a learned phrase; the comparison would be vacuous" >&2
            exit 1
        fi
        echo "pin:"
        summarize "$oracle_log"
        echo "oxpinyin:"
        summarize "$ox_log"
        diff_rc=0
        diff -u "$oracle_log" "$ox_log" > "$OUT/$lib-$protocol.diff" || diff_rc=$?
        if ((diff_rc > 1)); then
            echo "FAIL: could not compare $oracle_log and $ox_log" >&2
            exit 1
        fi
        if ((diff_rc == 0)); then
            echo "$lib/$protocol: IDENTICAL ($(wc -l < "$oracle_log") lines)"
        else
            echo "$lib/$protocol: DIVERGED"
            cat "$OUT/$lib-$protocol.diff"
            status=2
        fi
    done
done

if ((status == 0)); then
    echo "RESULT: every open-counter protocol is byte-identical to the pin"
else
    echo "RESULT: at least one open-counter protocol DIVERGED from the pin"
fi
exit "$status"
