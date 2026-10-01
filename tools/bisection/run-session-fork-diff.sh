#!/usr/bin/env bash
# run-session-fork-diff.sh — the session store across a fork and a crash,
# against the pin (#531, #546).
#
# The pin keeps a context's working state in process memory
# (pinyin.cpp:326-444 at 074a2219; the user bigram is an in-memory
# container, ngram_bdb.cpp:47-77), reads no temp variable and opens no
# temp file anywhere under src. Two consequences this runner checks:
#
#   * a forked child's learning and saving touches only the user dir's
#     files, and the parent's context, its later learning and its save
#     are what they would have been without the child;
#   * a process killed between init and fini leaves nothing outside the
#     user dir.
#
# tools/bisection/session-fork-diff.c drives the scenarios below into the
# pin-built libpinyin and into oxpinyin's, each run with TMPDIR set to a
# fresh, empty directory, and this runner diffs the logs byte for byte —
# the driver's step lines and learned state, then the process's exit
# status, the user dir's inventory and the number of entries left in
# TMPDIR:
#
#   fork-_exit   the child learns, saves and _exit()s; the parent learns
#                and saves after it.
#   fork-exit    the same, the child leaving through exit().
#   fork-nosave  the child learns but does not save.
#   crash        init, then SIGKILL before fini.
#
# A run that exceeds SESSION_FORK_TIMEOUT seconds is a failure on its own
# (the audited build never returned from the parent's step on Kyoto
# Cabinet, #531).
#
# Both sides open the pin prefix's own data directory with a fresh user
# dir per scenario; oxpinyin's library must be built with the store
# backend that matches the prefix's --with-dbm.
#
# Usage: run-session-fork-diff.sh
#
# Env:
#   SESSION_FORK_ORACLE_PREFIX  (required) a tools/oracle/build-oracle.sh
#                               prefix: lib/libpinyin.so.15 and
#                               lib/libpinyin/data. No default: a missing
#                               oracle fails the run, never skips it.
#   SESSION_FORK_PINYIN_SO      oxpinyin's libpinyin (default
#                               $REPO_ROOT/target/debug/libpinyin_capi.so)
#   SESSION_FORK_MODES          "fork-_exit fork-exit fork-nosave crash"
#                               (default) or any of them
#   SESSION_FORK_TIMEOUT        seconds a run may take (default 60)
#   SESSION_FORK_OUT            directory the logs are kept in (default: a
#                               temp dir, removed on exit)
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a required input is missing.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

PREFIX="${SESSION_FORK_ORACLE_PREFIX:-}"
if [[ -z "$PREFIX" || ! -d "$PREFIX" ]]; then
    echo "missing input: SESSION_FORK_ORACLE_PREFIX is unset or not a directory" >&2
    echo "  build it with tools/oracle/build-oracle.sh" >&2
    exit 3
fi
if ! grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$PREFIX/oracle-pin.txt" 2>/dev/null; then
    echo "missing input: $PREFIX is not a prefix of the 074a2219 pin (oracle-pin.txt)" >&2
    exit 3
fi
DATA="$PREFIX/lib/libpinyin/data"
ORACLE_SO="$PREFIX/lib/libpinyin.so.15"
OX_SO="${SESSION_FORK_PINYIN_SO:-$REPO_ROOT/target/debug/libpinyin_capi.so}"
read -r -a MODES <<< "${SESSION_FORK_MODES:-fork-_exit fork-exit fork-nosave crash}"
TIMEOUT="${SESSION_FORK_TIMEOUT:-60}"

[[ -f "$DATA/table.conf" ]] || { echo "missing input: $DATA/table.conf" >&2; exit 3; }
for so in "$ORACLE_SO" "$OX_SO"; do
    [[ -f "$so" ]] || { echo "missing input: $so" >&2; exit 3; }
done

if [[ -n "${SESSION_FORK_OUT:-}" ]]; then
    OUT="$SESSION_FORK_OUT"
    mkdir -p "$OUT"
    WORK="$(mktemp -d)"
    trap 'rm -rf "$WORK"' EXIT
else
    OUT="$(mktemp -d)"
    WORK="$OUT"
    trap 'rm -rf "$OUT"' EXIT
fi

echo "--- building session-fork-diff driver ---"
DRIVER="$WORK/session-fork-diff"
# shellcheck disable=SC2046  # pkg-config's flags are meant to split.
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$DRIVER" "$SCRIPT_DIR/session-fork-diff.c" \
    $(pkg-config --cflags --libs glib-2.0) -ldl
echo "build: ok"

# run_side <side> <mode> <lib.so> <log>: one run under a fresh TMPDIR and
# a fresh user dir; the log carries the driver's stdout, then the exit
# status, the user dir's inventory and TMPDIR's entry count.
run_side() {
    local side=$1 mode=$2 so=$3 log=$4
    local user="$WORK/user-$side-$mode"
    local tmp="$WORK/tmp-$side-$mode"
    rm -rf "$user" "$tmp"
    mkdir -p "$user" "$tmp"
    local rc=0
    # The subshell keeps the shell's own "Killed" notice for the crash
    # mode out of the runner's output; the driver's stderr is the log's.
    ( TMPDIR="$tmp" timeout "$TIMEOUT" "$DRIVER" "$so" "$DATA" "$user" "$mode" \
        > "$log" 2> "$log.stderr" ) 2> /dev/null || rc=$?
    if [[ $rc -eq 124 ]]; then
        echo "FAIL: $side/$mode did not finish within ${TIMEOUT}s" >&2
        return 1
    fi
    {
        echo "exit: $rc"
        echo "user dir:"
        (cd "$user" && find . -mindepth 1 | LC_ALL=C sort | sed 's/^/  /')
        echo "tmpdir entries: $(find "$tmp" -mindepth 1 | wc -l)"
    } >> "$log"
}

status=0
for mode in "${MODES[@]}"; do
    oracle_log="$OUT/oracle-$mode.log"
    ox_log="$OUT/oxpinyin-$mode.log"
    run_side oracle "$mode" "$ORACLE_SO" "$oracle_log" || { status=1; continue; }
    run_side oxpinyin "$mode" "$OX_SO" "$ox_log" || { status=1; continue; }
    if diff -u "$oracle_log" "$ox_log" > "$OUT/$mode.diff"; then
        echo "$mode: IDENTICAL"
    else
        echo "$mode: DIVERGENT"
        cat "$OUT/$mode.diff"
        [[ $status -eq 0 ]] && status=2
    fi
done
exit $status
