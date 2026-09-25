#!/usr/bin/env bash
# run-user-file-modes-diff.sh — the user files' creation modes against the
# pin (#544).
#
# Every file the pin writes into a user dir gets its mode from how it is
# written, not from a chmod afterwards:
#
#   *.dbin logs, user.bin,   MemoryChunk::save: open(O_CREAT|O_WRONLY|
#   addon.bin, network.bin   O_TRUNC, 0644) on the .tmp, then rename
#                            (memory_chunk.h:536-537; pinyin.cpp:971, :988)
#   user_pinyin_index.bin,   unlink the .tmp, then the backend's save_db:
#   user_phrase_index.bin,   Berkeley DB DB->open(DB_CREATE, 0600)
#   user_bigram.db           (chewing_large_table2_bdb.cpp:148-149,
#                            phrase_large_table3_bdb.cpp:163-164,
#                            ngram_bdb.cpp:94-95); Kyoto Cabinet
#                            dump_snapshot, an std::ofstream (0666);
#                            tkrzw's own Open(OPEN_DEFAULT)
#   user.conf                fopen("w") over the existing file, in place
#                            (table_info.cpp:377-397)
#
# and the process umask then clears bits from the requested mode. So the
# comparison runs under several umasks: 022 (the usual), 002 (which tells
# a 0644 request from a 0666 one) and 077. After the first launch every
# file is chmod'ed to 0640 — a user tightening their own profile — and a
# second launch shows which writes replace a file (fresh mode) and which
# write into it in place (the 0640 stays).
#
# Each launch is one clean init -> learn -> train -> save -> fini of
# tools/bisection/open-counter-diff.c, the same driver as the open-counter
# differential. Both sides open the pin prefix's own data directory with a
# fresh user dir; oxpinyin's libraries must be built with the store backend
# that matches the prefix's --with-dbm.
#
# Usage: run-user-file-modes-diff.sh
#
# Env:
#   MODES_ORACLE_PREFIX  (required) a tools/oracle/build-oracle.sh prefix
#                        configured with --enable-libzhuyin (as for
#                        run-open-counter-diff.sh). No default: a missing
#                        oracle fails the run, never skips it.
#   MODES_PINYIN_SO      oxpinyin's libpinyin (default
#                        $REPO_ROOT/target/debug/libpinyin_capi.so)
#   MODES_ZHUYIN_SO      oxpinyin's libzhuyin (default
#                        $REPO_ROOT/target/debug/libzhuyin_capi.so)
#   MODES_LIBS           "pinyin zhuyin" (default) or either one
#   MODES_UMASKS         "022 002 077" (default)
#   MODES_OUT            directory the logs are kept in (default: a temp
#                        dir, removed on exit)
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a required input is missing.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

PREFIX="${MODES_ORACLE_PREFIX:-}"
if [[ -z "$PREFIX" || ! -d "$PREFIX" ]]; then
    echo "missing input: MODES_ORACLE_PREFIX is unset or not a directory" >&2
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
    [pinyin]="${MODES_PINYIN_SO:-$REPO_ROOT/target/debug/libpinyin_capi.so}"
    [zhuyin]="${MODES_ZHUYIN_SO:-$REPO_ROOT/target/debug/libzhuyin_capi.so}"
)
read -r -a LIBS <<< "${MODES_LIBS:-pinyin zhuyin}"
read -r -a UMASKS <<< "${MODES_UMASKS:-022 002 077}"

[[ -f "$DATA/table.conf" ]] || { echo "missing input: $DATA/table.conf" >&2; exit 3; }
for lib in "${LIBS[@]}"; do
    [[ -n "${ORACLE_SO[$lib]:-}" ]] || { echo "unknown library: $lib" >&2; exit 3; }
    for so in "${ORACLE_SO[$lib]}" "${OX_SO[$lib]}"; do
        [[ -f "$so" ]] || { echo "missing input: $so" >&2; exit 3; }
    done
done

if [[ -n "${MODES_OUT:-}" ]]; then
    OUT="$MODES_OUT"
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

# Every file's mode, by name. Nothing else about the files is compared
# here — their contents are the open-counter differential's business.
modes() {
    find "$1" -mindepth 1 -maxdepth 1 -printf '%m %f\n' | LC_ALL=C sort -k2
}

# One clean launch under umask $1. The driver must finish (a clean launch
# saves), so any other exit fails the run.
launch() {
    local mask=$1 lib=$2 so=$3 user=$4 n=$5 log=$6 tmp=$7
    if ! (umask "$mask" && TMPDIR="$tmp" "$DRIVER" "$lib" "$so" "$DATA" "$user" "$n" cycle \
        > "$log.launch" 2>> "$log.stderr"); then
        echo "FAIL: launch $n of $so under umask $mask" >&2
        cat "$log.stderr" >&2
        return 1
    fi
    grep -q '^save: 1$' "$log.launch" || {
        echo "FAIL: launch $n of $so under umask $mask saved nothing" >&2
        return 1
    }
}

run_side() {
    local lib=$1 so=$2 log=$3
    : > "$log"
    : > "$log.stderr"
    for mask in "${UMASKS[@]}"; do
        local user="$WORK/user-$mask-$(basename "$log" .log)"
        local tmp="$WORK/tmp-$mask-$(basename "$log" .log)"
        rm -rf "$user" "$tmp"
        mkdir -p "$user" "$tmp"
        launch "$mask" "$lib" "$so" "$user" 1 "$log" "$tmp" || return 1
        modes "$user" | sed "s/^/umask $mask launch 1: /" >> "$log"
        find "$user" -mindepth 1 -maxdepth 1 -type f -exec chmod 0640 {} +
        launch "$mask" "$lib" "$so" "$user" 2 "$log" "$tmp" || return 1
        modes "$user" | sed "s/^/umask $mask launch 2 after chmod 640: /" >> "$log"
    done
}

status=0
for lib in "${LIBS[@]}"; do
    echo "=== $lib ==="
    oracle_log="$OUT/$lib-oracle.log"
    ox_log="$OUT/$lib-oxpinyin.log"
    run_side "$lib" "${ORACLE_SO[$lib]}" "$oracle_log" || exit 1
    run_side "$lib" "${OX_SO[$lib]}" "$ox_log" || exit 1
    [[ -s "$oracle_log" && -s "$ox_log" ]] || { echo "FAIL: an empty mode listing" >&2; exit 1; }
    diff_rc=0
    diff -u "$oracle_log" "$ox_log" > "$OUT/$lib.diff" || diff_rc=$?
    if ((diff_rc > 1)); then
        echo "FAIL: could not compare $oracle_log and $ox_log" >&2
        exit 1
    fi
    if ((diff_rc == 0)); then
        echo "$lib: IDENTICAL ($(wc -l < "$oracle_log") file modes)"
    else
        echo "$lib: DIVERGED"
        cat "$OUT/$lib.diff"
        status=2
    fi
done

if ((status == 0)); then
    echo "RESULT: every user file's mode matches the pin"
else
    echo "RESULT: at least one user file's mode DIVERGED from the pin"
fi
exit "$status"
