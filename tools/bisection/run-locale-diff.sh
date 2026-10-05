#!/usr/bin/env bash
# run-locale-diff.sh — the process-locale differential (#539, register row 39),
# over the forms of the user-dir argument (#619, register row 46).
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
# Which of those sites a context reaches turns on its user dir. The inits
# keep `g_strdup(userdir)` (pinyin.cpp:332, zhuyin.cpp:276), the guards
# test the pointer (pinyin.cpp:1133, :2671; zhuyin.cpp:548, :1697), and
# every user file's path is `g_build_filename(m_user_dir, <name>, NULL)`,
# built when the file is touched; g_build_filename drops an empty element
# and stops at a NULL one. So a "" user dir is the working directory — the
# one current at each file operation, not the one current at init — and
# the context trains and saves there, while a NULL user dir trains nothing
# and writes nothing. The two are different arguments and must stay
# different.
#
# This runner plays a consumer that adopted a UTF-8 environment
# (LC_ALL=zh_CN.UTF-8 by default) and drives tools/bisection/locale-diff.c
# into the pin-built libraries and into oxpinyin's, over five forms of the
# user-dir argument:
#
#   abs          an absolute, existing directory
#   empty        "", in an empty working directory
#   empty-chdir  "", and the consumer changes directory after init
#   null         NULL
#   dot          "."
#
# Each form is two consecutive processes in the same directories — a fresh
# profile, then the profile the first process left — with the same failure
# fixtures: a missing system dir, a dir without table.conf, a table.conf
# whose first line is not the format line, and one cut after its two
# version lines. Each step re-adopts the environment and prints LC_NUMERIC,
# the LC_ALL composite, and each watched directory's inventory
# (name:size:mode) and user.conf text; before the train the driver prints
# the watched word's unigram frequency, which shows whether the second
# process read the first one's training back. No path is printed: each
# side works in directories of its own and the two logs are diffed byte
# for byte.
#
# stderr is captured per side and kept beside the logs but not compared:
# the pin's raw messages there are #545's.
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
#   LOCALE_DIFF_FORMS          "abs empty empty-chdir null dot" (default) or
#                              any of them
#   LOCALE_DIFF_LOCALE         the environment adopted (zh_CN.UTF-8); it
#                              must be generated on the host (`locale -a`)
#   LOCALE_DIFF_OUT            directory the logs and the directories each
#                              side left are kept in (default: a temp dir,
#                              removed on exit)
#
# Exit codes: 0 = identical; 1 = build/run failure; 2 = divergence;
# 3 = a required input is missing.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"
# shellcheck source=tools/bisection/oracle-cell.sh
source "$REPO_ROOT/tools/bisection/oracle-cell.sh"

PREFIX="${LOCALE_DIFF_ORACLE_PREFIX:-${PINYIN_ORACLE_PREFIX:-}}"
if [[ -z "$PREFIX" || ! -d "$PREFIX" ]]; then
    echo "missing input: LOCALE_DIFF_ORACLE_PREFIX is unset or not a directory" >&2
    echo "  build it with tools/oracle/build-oracle.sh, configure line plus --enable-libzhuyin" >&2
    exit 77
fi
if ! grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$PREFIX/oracle-pin.txt" 2>/dev/null; then
    echo "missing input: $PREFIX is not a prefix of the 074a2219 pin (oracle-pin.txt)" >&2
    exit 3
fi
# The driver starts in a directory of its own, so every path it is given
# must be absolute.
PREFIX="$(cd "$PREFIX" && pwd)"
DATA="$PREFIX/lib/libpinyin/data"
declare -A ORACLE_SO=([pinyin]="$PREFIX/lib/libpinyin.so.15" [zhuyin]="$PREFIX/lib/libzhuyin.so.15")
declare -A OX_SO=(
    [pinyin]="${LOCALE_DIFF_PINYIN_SO:-${OXPINYIN_CAPI_SO:-$CARGO_TARGET_DIR/debug/libpinyin_capi.so}}"
    [zhuyin]="${LOCALE_DIFF_ZHUYIN_SO:-${OXPINYIN_ZHUYIN_SO:-$CARGO_TARGET_DIR/debug/libzhuyin_capi.so}}"
)
read -r -a LIBS <<< "${LOCALE_DIFF_LIBS:-pinyin zhuyin}"
read -r -a FORMS <<< "${LOCALE_DIFF_FORMS:-abs empty empty-chdir null dot}"
LOCALE_NAME="${LOCALE_DIFF_LOCALE:-zh_CN.UTF-8}"

for form in "${FORMS[@]}"; do
    case $form in
    abs | empty | empty-chdir | null | dot) ;;
    *)
        echo "missing input: unknown form in LOCALE_DIFF_FORMS: $form" >&2
        exit 3
        ;;
    esac
done

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
    OX_SO[$lib]="$(cd "$(dirname "${OX_SO[$lib]}")" && pwd)/$(basename "${OX_SO[$lib]}")"
done
[[ -f $DATA/table.conf ]] || { echo "missing input: $DATA holds no table.conf" >&2; exit 3; }

if [[ -n "${LOCALE_DIFF_OUT:-}" ]]; then
    mkdir -p "$LOCALE_DIFF_OUT"
    OUT="$(cd "$LOCALE_DIFF_OUT" && pwd)"
else
    OUT="$(mktemp -d)"
    trap 'rm -rf "$OUT"' EXIT
fi

read -r -a glib_flags <<< "$(pkg-config --cflags --libs glib-2.0)"
cc -std=c11 -O1 -Wall -Wextra -o "$OUT/locale-diff" "$SCRIPT_DIR/locale-diff.c" \
    "${glib_flags[@]}" -ldl

# The failure fixtures, shared by both sides (nothing writes into them).
scratch="$OUT/scratch"
mkdir -p "$scratch/empty" "$scratch/garbage" "$scratch/truncated"
printf 'not a table.conf\n' > "$scratch/garbage/table.conf"
head -n 2 "$DATA/table.conf" > "$scratch/truncated/table.conf"

echo "data dir: $DATA"
echo "locale:   $LOCALE_NAME"
status=0
for lib in "${LIBS[@]}"; do
    for form in "${FORMS[@]}"; do
        for side in oracle ox; do
            so=${ORACLE_SO[$lib]}
            [[ $side == ox ]] && so=${OX_SO[$lib]}
            # Each side's own directories: the working directory both
            # processes start in, and the form's second directory.
            base="$OUT/$lib-$form-$side"
            rm -rf "$base"
            mkdir -p "$base/cwd"
            case $form in
            abs)
                mkdir -p "$base/userdir"
                args=("$base/userdir" "$scratch")
                ;;
            empty) args=("" "$scratch") ;;
            empty-chdir)
                mkdir -p "$base/moved-to"
                args=("" "$scratch" "$base/moved-to")
                ;;
            null) args=(NULL "$scratch") ;;
            dot) args=(. "$scratch") ;;
            esac
            : > "$base.log"
            : > "$base.err"
            for process in 1 2; do
                echo "== process $process ==" >> "$base.log"
                if ! (cd "$base/cwd" && env LC_ALL="$LOCALE_NAME" \
                    "$OUT/locale-diff" "$lib" "$so" "$DATA" "${args[@]}") \
                    >> "$base.log" 2>> "$base.err"; then
                    echo "run failure: $lib/$form/$side process $process (see $base.err)" >&2
                    status=1
                fi
            done
        done
        # The dirty-save step only measures the pin's `mark_version` write
        # (pinyin.cpp:1143, zhuyin.cpp:695) when the train set m_modified.
        # Every form but null has a user dir on the pin, so both of its
        # processes train (pinyin.cpp:2671, zhuyin.cpp:1697); an oracle log
        # that says otherwise never reached that save, and two matching
        # logs would pass without it. That is a run failure, not a result.
        if [[ $form != null ]] &&
            [[ $(grep -c '^train: ok$' "$OUT/$lib-$form-oracle.log") != 2 ]]; then
            echo "run failure: $lib/$form: the oracle's train did not set m_modified in both processes; save-modified is not measured" >&2
            status=1
        fi
        if diff -u "$OUT/$lib-$form-oracle.log" "$OUT/$lib-$form-ox.log" > "$OUT/$lib-$form.diff"; then
            echo "$lib $form: identical ($(wc -l < "$OUT/$lib-$form-oracle.log") lines)"
        else
            echo "$lib $form: DIVERGENT"
            cat "$OUT/$lib-$form.diff"
            (( status == 0 )) && status=2
        fi
    done
done
exit $status
