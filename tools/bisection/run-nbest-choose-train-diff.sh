#!/usr/bin/env bash
# Lane I: every offered n-best candidate, both facades, fresh profiles.
# Usage: <bdb|kc|tkrzw> <oracle-prefix> <pinyin-so> <zhuyin-so> <new-out-dir>
# Snapshots and dumps remain in out for review and revert-and-check.
set -euo pipefail
REPO_ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
PINYIN_ORACLE_DBM=${1:?backend}
# shellcheck source=tools/bisection/oracle-cell.sh
source "$REPO_ROOT/tools/bisection/oracle-cell.sh"
cell=${1:?backend}
prefix=${2:?oracle prefix}
pinyin_so=${3:?subject libpinyin}
zhuyin_so=${4:?subject libzhuyin}
out=${5:?new output directory}
case "$cell" in bdb|kc|tkrzw) ;; *) exit 1;; esac
[[ -f $prefix/oracle-pin.txt && -f $prefix/lib/libpinyin.so ]] || {
    echo "SKIP: missing oracle: $prefix" >&2; exit 77;
}
for f in "$prefix/oracle-pin.txt" "$prefix/lib/libpinyin.so" \
    "$prefix/lib/libzhuyin.so" "$prefix/lib/libpinyin/data/bigram.db" \
    "$pinyin_so" "$zhuyin_so"; do
    test -f "$f" || { echo "missing input: $f" >&2; exit 1; }
done
grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$prefix/oracle-pin.txt"
mkdir "$out"
out=$(cd "$out" && pwd)
script_dir=$(cd "$(dirname "$0")" && pwd)
gcc -std=gnu11 -Wall -Wextra -Werror -O2 \
    "$script_dir/candidate-assembly-diff.c" -ldl -o "$out/driver"
status=0
for mode in pinyin zhuyin; do
    for side in pin ox; do
        dest="$out/$mode-$side"
        mkdir "$dest"
        if [[ $side == pin ]]; then
            so="$prefix/lib/lib$mode.so"
        elif [[ $mode == pinyin ]]; then
            so=$pinyin_so
        else
            so=$zhuyin_so
        fi
        TMPDIR="$dest" CAND_ASSEMBLY_TRAIN_DIR="$dest" \
            "$out/driver" "$mode" "$so" "$prefix/lib/libpinyin/data" \
            > "$dest/driver.log" 2> "$dest/driver.err"
    done
    diff -u "$out/$mode-pin/driver.log" "$out/$mode-ox/driver.log" || status=2
    python3 "$script_dir/compare-nbest-training-state.py" "$cell" \
        "$out/$mode-pin" "$out/$mode-ox" --pin-so "$prefix/lib/libpinyin.so" \
        --ox-so "$pinyin_so" --data "$prefix/lib/libpinyin/data" \
        --facade "$mode" || status=$?
done
exit "$status"
