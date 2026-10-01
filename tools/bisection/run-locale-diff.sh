#!/usr/bin/env bash
# run-locale-diff.sh — the process-locale differential (#539, register row 39).
#
# The pin's table.conf and user.conf codecs each run `char * locale =
# setlocale(LC_NUMERIC, "C"); ... setlocale(LC_NUMERIC, locale);`
# (storage/table_info.cpp:197,291; :328,372; :378,394). setlocale answers
# the name of the locale it installs, so the "restore" installs "C" again
# and every early return skips it with the same result: each call leaves
# the process's LC_NUMERIC at "C", on success and on failure. The entry
# points that reach a codec inherit that — pinyin_init/zhuyin_init
# (pinyin.cpp:337, :178, :187; zhuyin.cpp:281, :132), pinyin_save and
# zhuyin_save past their guards (pinyin.cpp:1133-1143; zhuyin.cpp:548-552,
# :695) and pinyin_fini (pinyin.cpp:1200); zhuyin_fini writes nothing
# (zhuyin.cpp:741-757). The ruling is to reproduce it bug-for-bug.
#
# This runner plays a consumer that adopted a UTF-8 environment
# (LC_ALL=zh_CN.UTF-8 by default) and drives tools/bisection/locale-diff.c
# into the pin-built libraries and into oxpinyin's, one process per
# library per side, with fresh user dirs and the same failure fixtures:
# a missing system dir, a dir without table.conf, a table.conf whose first
# line is not the format line, and one cut after its two version lines.
# Each step re-adopts the environment and prints LC_NUMERIC and the
# LC_ALL composite; the two logs are diffed byte for byte.
#
# Both sides open the pin prefix's own data directory (the drop-in
# contract); the oxpinyin libraries must be built with the store backend
# that matches the prefix's --with-dbm.
#
# Usage: run-locale-diff.sh
#
# Env:
#   LOCALE_DIFF_ORACLE_PREFIX  (required) a tools/oracle/build-oracle.sh
#                              prefix configured with --enable-libzhuyin:
#                              lib/libpinyin.so.15, lib/libzhuyin.so.15,
#                              lib/libpinyin/data. No default: a missing
#                              oracle fails the run, never skips it.
#   LOCALE_DIFF_PINYIN_SO      oxpinyin's libpinyin (default
#                              $REPO_ROOT/target/debug/libpinyin_capi.so)
#   LOCALE_DIFF_ZHUYIN_SO      oxpinyin's libzhuyin (default
#                              $REPO_ROOT/target/debug/libzhuyin_capi.so)
#   LOCALE_DIFF_LIBS           "pinyin zhuyin" (default) or either one
#   LOCALE_DIFF_LOCALE         the environment adopted (zh_CN.UTF-8); it
#                              must be generated on the host (`locale -a`)
#   LOCALE_DIFF_OUT            directory the logs are kept in (default: a
#                              temp dir, removed on exit)
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a required input is missing.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

PREFIX="${LOCALE_DIFF_ORACLE_PREFIX:-}"
if [[ -z "$PREFIX" || ! -d "$PREFIX" ]]; then
    echo "missing input: LOCALE_DIFF_ORACLE_PREFIX is unset or not a directory" >&2
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
    [pinyin]="${LOCALE_DIFF_PINYIN_SO:-$REPO_ROOT/target/debug/libpinyin_capi.so}"
    [zhuyin]="${LOCALE_DIFF_ZHUYIN_SO:-$REPO_ROOT/target/debug/libzhuyin_capi.so}"
)
read -r -a LIBS <<< "${LOCALE_DIFF_LIBS:-pinyin zhuyin}"
LOCALE_NAME="${LOCALE_DIFF_LOCALE:-zh_CN.UTF-8}"

# glibc lists a generated locale with its codeset normalised (zh_CN.utf8),
# so compare the normalised forms.
normalise() { printf '%s' "$1" | tr '[:upper:]' '[:lower:]' | tr -d '-'; }
want=$(normalise "$LOCALE_NAME")
found=0
while IFS= read -r have; do
    [[ $(normalise "$have") == "$want" ]] && found=1 && break
done < <(locale -a)
if (( ! found )); then
    echo "missing input: locale $LOCALE_NAME is not generated on this host (locale -a)" >&2
    exit 3
fi

for lib in "${LIBS[@]}"; do
    for f in "${ORACLE_SO[$lib]}" "${OX_SO[$lib]}"; do
        [[ -f $f ]] || { echo "missing input: $f not found" >&2; exit 3; }
    done
done
[[ -f $DATA/table.conf ]] || { echo "missing input: $DATA holds no table.conf" >&2; exit 3; }

if [[ -n "${LOCALE_DIFF_OUT:-}" ]]; then
    OUT="$LOCALE_DIFF_OUT"
    mkdir -p "$OUT"
else
    OUT="$(mktemp -d)"
    trap 'rm -rf "$OUT"' EXIT
fi

cc -std=c11 -O1 -Wall -Wextra -o "$OUT/locale-diff" "$SCRIPT_DIR/locale-diff.c" \
    $(pkg-config --cflags glib-2.0) $(pkg-config --libs glib-2.0) -ldl

# The failure fixtures, shared by both sides (nothing writes into them).
scratch="$OUT/scratch"
mkdir -p "$scratch/empty" "$scratch/garbage" "$scratch/truncated"
printf 'not a table.conf\n' > "$scratch/garbage/table.conf"
head -n 2 "$DATA/table.conf" > "$scratch/truncated/table.conf"

echo "data dir: $DATA"
echo "locale:   $LOCALE_NAME"
status=0
for lib in "${LIBS[@]}"; do
    for side in oracle ox; do
        so=${ORACLE_SO[$lib]}
        [[ $side == ox ]] && so=${OX_SO[$lib]}
        user="$OUT/$lib-$side-user"
        rm -rf "$user"
        mkdir -p "$user"
        if ! env LC_ALL="$LOCALE_NAME" "$OUT/locale-diff" "$lib" "$so" "$DATA" "$user" "$scratch" \
            > "$OUT/$lib-$side.log" 2> "$OUT/$lib-$side.err"; then
            echo "run failure: $lib/$side (see $OUT/$lib-$side.err)" >&2
            status=1
        fi
    done
    if diff -u "$OUT/$lib-oracle.log" "$OUT/$lib-ox.log" > "$OUT/$lib.diff"; then
        echo "$lib: identical ($(wc -l < "$OUT/$lib-oracle.log") lines)"
    else
        echo "$lib: DIVERGENT"
        cat "$OUT/$lib.diff"
        (( status == 0 )) && status=2
    fi
done
exit $status
