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
# Three protocols, per library (pinyin, zhuyin):
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
#   seeded one clean launch, then user.conf's counter line is rewritten to
#          a seed (below), then two launches that init and fini and learn
#          nothing. The pin reads the counter with fscanf's %d
#          (table_info.cpp:356-359, glibc): leading whitespace and a sign
#          are taken, the digits run until the first non-digit, a value
#          past int keeps the low 32 bits of strtol's long, and no digits
#          at all read 0. The value then decides the wipe (above 6) and
#          libpinyin's init write (the value plus one, through
#          get_open_counter, pinyin.cpp:185-187) and fini write (:1194-1200).
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
#   OPEN_COUNTER_PROTOCOLS      "cycle crash seeded" (default) or any of them
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
read -r -a PROTOCOLS <<< "${OPEN_COUNTER_PROTOCOLS:-cycle crash seeded}"
CYCLES="${OPEN_COUNTER_CYCLES:-10}"
CRASHES="${OPEN_COUNTER_CRASHES:-10}"

# The seeded protocol's cases: user.conf's fourth line as printf %b writes
# it, and what glibc's fscanf("open counter:%d\n") reads there. The %d
# column is glibc's, checked in debian:testing (glibc 2.43).
SEEDS=(control-5 control-7 negative plus plus-over garbage garbage-over garbage-byte invalid-prefix
    space tab vtab newline indent spaced joined overflow-int overflow-wrap overflow-long
    overflow-neg under-int empty sign missing)
declare -A SEED_LINE=(
    [control-5]='open counter:5\n'                       # 5
    [control-7]='open counter:7\n'                       # 7, above the limit
    [negative]='open counter:-3\n'                       # -3
    [plus]='open counter:+3\n'                           # 3
    [plus-over]='open counter:+7\n'                      # 7
    [garbage]='open counter:3x\n'                        # 3
    [garbage-over]='open counter:7x\n'                   # 7
    [garbage-byte]='open counter:3\xff\n'               # 3; non-UTF-8 after the digits
    [invalid-prefix]='open counter:5\n'                 # stray byte before the identity fields
    [space]='open counter: 5\n'                          # 5
    [tab]='open counter:\t5\n'                           # 5
    [vtab]='open counter:\v5\n'                          # 5: C's isspace has \v
    [newline]='open counter:\n5\n'                        # 5, from the next line
    [indent]='  open counter:5\n'                        # 5: the \n before skips it
    [spaced]='open   counter:5\n'                        # 5: the format's space
    [joined]='opencounter:5\n'                           # 5: ... matches none too
    [overflow-int]='open counter:2147483648\n'           # -2147483648
    [overflow-wrap]='open counter:4294967303\n'          # 7
    [overflow-long]='open counter:99999999999999999999\n' # -1 (LONG_MAX's low half)
    [overflow-neg]='open counter:-99999999999999999999\n' # 0 (LONG_MIN's low half)
    [under-int]='open counter:-2147483649\n'             # 2147483647
    [empty]='open counter:\n'                            # 0: no digits (EOF)
    [sign]='open counter:-\n'                            # 0: no digits
    [missing]=''                                         # 0: no line (EOF)
)

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
    cycle:0 | look:0) echo "exit: 0" >> "$log" ;;
    crash:137) echo "exit: SIGKILL" >> "$log" ;;
    *)
        echo "FAIL: launch $n ($mode) of $so exited $rc" >&2
        cat "$err" >&2
        return 1
        ;;
    esac
    snapshot "$user" >> "$log"
}

# Rewrite user.conf for seed $2: its first three lines as the last launch
# wrote them, then the seed's fourth line.
seed_conf() {
    local conf=$1/user.conf head
    head=$(head -n 3 "$conf")
    {
        if [[ $2 == invalid-prefix ]]; then printf '\xff\n'; fi
        printf '%s\n' "$head"
        printf '%b' "${SEED_LINE[$2]}"
    } > "$conf"
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
    seed-*)
        n=1; launch "$lib" "$so" "$user" "$n" cycle "$log" "$err" "$tmp" || return 1
        seed_conf "$user" "${protocol#seed-}"
        echo "seeded: ${protocol#seed-}" >> "$log"
        snapshot "$user" >> "$log"
        for n in 2 3; do
            launch "$lib" "$so" "$user" "$n" look "$log" "$err" "$tmp" || return 1
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

# A seeded run in one line per look launch: user.conf's counter as the
# init left it and as the fini left it, and whether the profile survived
# (a wipe leaves user.conf alone in the dir).
seed_summary() {
    awk '
        /^launch: / { split($0, a, " "); n = a[2]; look = (a[3] == "look"); init = "" }
        look && /^conf@init: / && index($0, "counter:") { init = substr($0, index($0, "counter:") + 8) }
        look && /^conf: / && index($0, "counter:") { fini = substr($0, index($0, "counter:") + 8) }
        look && /^files: / {
            kept = (NF > 2) ? "kept" : "wiped"
            printf "  launch %d: counter %s at init, %s at fini, profile %s\n", n, (init == "" ? "-" : init), fini, kept
        }' "$1"
}

status=0
for lib in "${LIBS[@]}"; do
    protocols=()
    for protocol in "${PROTOCOLS[@]}"; do
        if [[ $protocol == seeded ]]; then
            for seed in "${SEEDS[@]}"; do protocols+=("seed-$seed"); done
        else
            protocols+=("$protocol")
        fi
    done
    for protocol in "${protocols[@]}"; do
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
        if [[ $protocol == seed-* ]]; then seed_summary "$oracle_log"; else summarize "$oracle_log"; fi
        echo "oxpinyin:"
        if [[ $protocol == seed-* ]]; then seed_summary "$ox_log"; else summarize "$ox_log"; fi
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
