#!/usr/bin/env bash
# run-open-counter-diff.sh — the user.conf differential (#523, #583, #589, #590).
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
#          The same protocol carries the whole-marker cases (`conf-*`,
#          CONF_CASES): the pin reads user.conf as four fscanf calls
#          (table_info.cpp:338-359), so the version lines take a sign and
#          the low 32 bits, `%255s` reads the format token across
#          newlines, and a failed directive is what the next one meets.
#   abort  the expectation channel (ABORT_CASES), for the markers the pin
#          cannot survive: `to_table_database_format_type` `abort()`s on
#          any token it does not know (table_info.cpp:122-133, reached
#          from :353-354, including the matching failure that leaves its
#          `str` unwritten). No byte diff is possible — the oracle dies
#          with SIGABRT — so each case asserts the pair instead: oracle
#          exit 134, this side's `*_init` NULL, exactly one class-(c) log
#          line (CLASS_C_LINE), and a user dir byte-identical to the
#          seeded one on both sides. Any other outcome fails the run.
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
read -r -a PROTOCOLS <<< "${OPEN_COUNTER_PROTOCOLS:-cycle crash seeded abort}"
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
    [missing]=''                                         # 0: no line (EOF)
)

# The `user.conf` sequence protocol's cases: the whole marker is written
# from the seed launch's own three identity lines (`conf_body`), so the
# cell's `database format:` token comes from the launch, not from this
# script. Two families:
#
#   conf-*   the marker parses (or fails to) the same way on both sides;
#            these are compared byte for byte like the counter seeds.
#   abort-*  the marker reaches `to_table_database_format_type`, which
#            `abort()`s the pin's process on any token it does not know
#            (`table_info.cpp:122-133`, called at `:353-354`). No byte
#            diff is possible — the pin dies — so `run_abort_case`
#            asserts the pair instead: oracle exit 134, this side's
#            `*_init` NULL, exactly one class-(c) log line, and a user
#            dir byte-identical to the seeded one on both sides.
CONF_CASES=(binver-plus binver-wrap binver-neg binver-junk binver-space
    modelver-plus modelver-wrap modelver-neg
    foreign-line reordered token-junk-tail no-newline empty)
ABORT_CASES=(unknown-token long-token trunc-counter modelver-junk no-dbformat)

# The class-(c) line both C facades log for an abort shape
# (`oxpinyin_facade::UNKNOWN_DATABASE_FORMAT_WARNING`); exactly one per
# attempt, and the only thing that says why `*_init` answered NULL.
CLASS_C_LINE='check_format: unknown database format in user.conf'

# The case's `user.conf`, from the three identity lines the seed launch
# wrote: `$2`/`$3`/`$4` are its `binary format version:`, `model data
# version:` and `database format:` lines verbatim, and each case keeps
# the ones it does not replace. Line 4 is `open counter:5` unless the
# case moves the counter (or removes it).
conf_body() {
    local case=$1 l1=$2 l2=$3 l3=$4
    case $case in
    binver-plus)     printf 'binary format version:+7\n%s\n%s\nopen counter:5\n' "$l2" "$l3" ;;
    binver-wrap)     printf 'binary format version:4294967303\n%s\n%s\nopen counter:5\n' "$l2" "$l3" ;;
    binver-neg)      printf 'binary format version:-1\n%s\n%s\nopen counter:5\n' "$l2" "$l3" ;;
    binver-junk)     printf 'binary format version:7x\n%s\n%s\nopen counter:5\n' "$l2" "$l3" ;;
    binver-space)    printf 'binary format version: 7\n%s\n%s\nopen counter:5\n' "$l2" "$l3" ;;
    modelver-plus)   printf '%s\nmodel data version:+14\n%s\nopen counter:5\n' "$l1" "$l3" ;;
    modelver-wrap)   printf '%s\nmodel data version:4294967310\n%s\nopen counter:5\n' "$l1" "$l3" ;;
    modelver-neg)    printf '%s\nmodel data version:-1\n%s\nopen counter:5\n' "$l1" "$l3" ;;
    foreign-line)    printf '%s\n%s\n%s\njunk line\nopen counter:5\n' "$l1" "$l2" "$l3" ;;
    reordered)       printf '%s\n%s\n%s\nopen counter:5\n' "$l2" "$l1" "$l3" ;;
    token-junk-tail) printf '%s\n%s\n%s extra\nopen counter:5\n' "$l1" "$l2" "$l3" ;;
    no-newline)      printf '%s\n%s\n%s\nopen counter:5' "$l1" "$l2" "$l3" ;;
    empty)           : ;;
    unknown-token)   printf '%s\n%s\ndatabase format:NotADbmLibrary\nopen counter:5\n' "$l1" "$l2" ;;
    long-token)      printf '%s\n%s\ndatabase format:%s\nopen counter:5\n' "$l1" "$l2" \
                         "$(printf 'A%.0s' {1..300})" ;;
    trunc-counter)   printf '%s\n%s\ndatabase format:%sopen counter:5\n' "$l1" "$l2" \
                         "$(printf 'A%.0s' {1..255})" ;;
    modelver-junk)   printf '%s\nmodel data version:14x\n%s\nopen counter:5\n' "$l1" "$l3" ;;
    no-dbformat)     printf '%s\n%s\nopen counter:5\n' "$l1" "$l2" ;;
    *) echo "unknown case: $case" >&2; return 1 ;;
    esac
}

# The seed launch's three identity lines: what the pin wrote for its own
# backend, which every case builds on.
seed_identity() {
    local dir=$1
    L1=$(sed -n '1p' "$dir/user.conf")
    L2=$(sed -n '2p' "$dir/user.conf")
    L3=$(sed -n '3p' "$dir/user.conf")
}

# The user dir's whole content: every entry, sorted, with its hash. The
# abort protocol compares this before and after a launch on both sides —
# the no-wipe assertion, inventory and bytes.
dir_digest() {
    (cd "$1" && find . -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort | \
        while IFS= read -r name; do
            printf '%s  ' "$name"
            sha256sum < "$name"
        done)
}

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

# Rewrite user.conf for a `conf-*` case: the whole marker, built from the
# seed launch's own identity lines.
conf_conf() {
    local case=$1 dir=$2
    seed_identity "$dir"
    conf_body "$case" "$L1" "$L2" "$L3" > "$dir/user.conf"
}

# One abort shape, the expectation channel: the pin's
# `to_table_database_format_type` `abort()`s (`table_info.cpp:132`), so no
# byte diff is possible. Both sides run from the same seeded bytes and
# must leave them alone; the oracle must die with SIGABRT, this side's
# `*_init` must answer NULL with exactly one class-(c) line, and neither
# side may wipe. Anything else fails the run (`status=2`).
run_abort_case() {
    local lib=$1 case=$2
    local seed="$WORK/abort-$lib-$case"
    local oracle_log="$OUT/$lib-abort-$case-oracle.log"
    local ox_log="$OUT/$lib-abort-$case-oxpinyin.log"
    local oracle_err="$oracle_log.stderr" ox_err="$ox_log.stderr"
    local fail=0

    echo "=== $lib / abort-$case ==="
    rm -rf "$seed"; mkdir -p "$seed/user" "$seed/tmp-seed"
    # The seeded dir both sides start from: one clean oracle launch (the
    # pin's own eleven files, counter 0), then the case's marker.
    launch "$lib" "${ORACLE_SO[$lib]}" "$seed/user" 1 cycle "$seed/seed.log" \
        "$seed/seed.stderr" "$seed/tmp-seed" || return 1
    conf_conf "$case" "$seed/user"
    mkdir -p "$seed/template"; cp -a "$seed/user/." "$seed/template/"
    local want; want=$(dir_digest "$seed/template")

    # The oracle side: `abort()`, and nothing touched.
    local user="$seed/user-oracle" tmp="$seed/tmp-oracle" rc=0
    mkdir -p "$user" "$tmp"; cp -a "$seed/template/." "$user/"
    : > "$oracle_log"; : > "$oracle_err"
    ( TMPDIR="$tmp" "$DRIVER" "$lib" "${ORACLE_SO[$lib]}" "$DATA" "$user" 2 look \
        >> "$oracle_log"; exit $? ) 2>> "$oracle_err" || rc=$?
    if [[ $rc == 134 ]]; then
        echo "  oracle: exit 134 (SIGABRT)"
    else
        echo "  oracle: FAIL - expected exit 134, got $rc" >&2; fail=1
    fi
    if [[ $(dir_digest "$user") == "$want" ]]; then
        echo "  oracle: user dir byte-identical to the seeded dir"
    else
        echo "  oracle: FAIL - the user dir moved" >&2; fail=1
    fi

    # This side: the open refused, one class-(c) line, nothing touched.
    user="$seed/user-oxpinyin"; tmp="$seed/tmp-oxpinyin"; rc=0
    mkdir -p "$user" "$tmp"; cp -a "$seed/template/." "$user/"
    : > "$ox_log"; : > "$ox_err"
    ( TMPDIR="$tmp" "$DRIVER" "$lib" "${OX_SO[$lib]}" "$DATA" "$user" 2 look \
        >> "$ox_log"; exit $? ) 2>> "$ox_err" || rc=$?
    if grep -q '^init: NULL$' "$ox_log"; then
        echo "  oxpinyin: init answered NULL"
    else
        echo "  oxpinyin: FAIL - expected 'init: NULL', got: \
              $(tr '\n' ' ' < "$ox_log" | head -c 200)" >&2
        fail=1
    fi
    if [[ $rc == 1 ]]; then
        echo "  oxpinyin: exit 1 (the open failed, as the C ABI says)"
    else
        echo "  oxpinyin: FAIL - expected exit 1, got $rc" >&2; fail=1
    fi
    local lines=0
    lines=$(grep -c -F "$CLASS_C_LINE" "$ox_err" || true)
    if [[ $lines == 1 ]]; then
        echo "  oxpinyin: one class-(c) line ($CLASS_C_LINE)"
    else
        echo "  oxpinyin: FAIL - expected exactly one class-(c) line, got $lines" >&2
        sed 's/^/    /' "$ox_err" >&2
        fail=1
    fi
    if [[ $(dir_digest "$user") == "$want" ]]; then
        echo "  oxpinyin: user dir byte-identical to the seeded dir"
    else
        echo "  oxpinyin: FAIL - the user dir moved (a wipe is not this class)" >&2; fail=1
    fi
    if ((fail)); then
        echo "FAIL: abort-$case did not hold its expectation" >&2
        status=2
    fi
    return 0
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
    conf-*)
        n=1; launch "$lib" "$so" "$user" "$n" cycle "$log" "$err" "$tmp" || return 1
        conf_conf "${protocol#conf-}" "$user"
        echo "seeded: ${protocol#conf-}" >> "$log"
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
            for case in "${CONF_CASES[@]}"; do protocols+=("conf-$case"); done
        elif [[ $protocol == abort ]]; then
            : # the expectation channel, run below: no byte diff is possible
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

    if [[ " ${PROTOCOLS[*]} " == *" abort "* ]]; then
        for case in "${ABORT_CASES[@]}"; do
            run_abort_case "$lib" "$case" || exit 1
        done
    fi
done

if ((status == 0)); then
    echo "RESULT: every open-counter protocol is byte-identical to the pin"
else
    echo "RESULT: at least one open-counter protocol DIVERGED from the pin"
fi
exit "$status"
