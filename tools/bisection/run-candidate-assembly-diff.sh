#!/usr/bin/env bash
# run-candidate-assembly-diff.sh — the candidate-list assembly
# differential (candidate-assembly-diff.c): issue #582 and register rows
# 34 and 37, full lists byte for byte, libpinyin and libzhuyin, both
# sides opened on the oracle prefix's own data/ (same-dir).
#
# usage: run-candidate-assembly-diff.sh <oracle-prefix> <libpinyin_capi.so> <libzhuyin_capi.so> [out-dir]
#
#   <oracle-prefix>  a build-oracle.sh prefix configured with
#                    --enable-libzhuyin (lib/libpinyin.so, lib/libzhuyin.so,
#                    lib/libpinyin/data, oracle-pin.txt at the pin)
#   <out-dir>        where the four logs land (default: a mktemp dir,
#                    removed when the result is as declared)
#
# The capi builds must carry the prefix's backend (tkrzw, bdb or
# kyotocabinet). A missing input is a failure, never a skip.
#
# The logs are compared case by case (each `== ` header of the driver):
# every case must be byte-identical except those the declared table
# below names, and each of those must still diverge.
#
# Exit: 0 = as declared; 1 = missing input or a crash; 2 = a divergence
# outside the table, or a declared case that no longer diverges.
set -euo pipefail

# Resolved before the `cd` below: the usage text allows relative paths, and
# the existence checks, the dlopen of the shared objects and the logs all
# have to mean the same file as the caller's directory did.
prefix=$(realpath -- "${1:?oracle prefix}")
capi_so=$(realpath -- "${2:?path to libpinyin_capi.so}")
zhuyin_so=$(realpath -- "${3:?path to libzhuyin_capi.so}")
out=${4:-}
[[ -z "$out" ]] || out=$(realpath -m -- "$out")

for f in "$prefix/oracle-pin.txt" "$prefix/lib/libpinyin.so" "$prefix/lib/libzhuyin.so" \
    "$prefix/lib/libpinyin/data/bigram.db" "$capi_so" "$zhuyin_so"; do
    [[ -e "$f" ]] || { echo "FAIL: missing input $f" >&2; exit 1; }
done
if ! grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$prefix/oracle-pin.txt"; then
    echo "FAIL: $prefix is not a build of the pin" >&2
    exit 1
fi
data="$prefix/lib/libpinyin/data"

cd "$(dirname "$0")"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o candidate-assembly-diff \
    candidate-assembly-diff.c -ldl

if [[ -z "$out" ]]; then
    out=$(mktemp -d)
    clean=1
else
    mkdir -p "$out"
    clean=0
fi

run() {
    local mode=$1 side=$2 so=$3
    if ! ./candidate-assembly-diff "$mode" "$so" "$data" \
        > "$out/$mode-$side.log" 2> "$out/$mode-$side.err"; then
        echo "FAIL: $mode driver crashed against $side ($so)" >&2
        tail -5 "$out/$mode-$side.err" >&2
        exit 1
    fi
}
run pinyin pin "$prefix/lib/libpinyin.so"
run pinyin ox "$capi_so"
run zhuyin pin "$prefix/lib/libzhuyin.so"
run zhuyin ox "$zhuyin_so"

# The declared divergences: `mode<TAB>case-header regex<TAB>reason`. A
# declared case must diverge and every other case must be identical —
# a declared case that stops diverging fails too, so the table cannot
# go stale silently (for the §12 rows that is a pin move: re-measure,
# do not just edit the table).
declared=$(cat <<'TABLE'
pinyin	^A word=0x(0|1e) import=(true|false) input=ba'kua$	§12 frozen sentence-surface residual? (pin 2 n-best rows, oxpinyin 3 — #594)
pinyin	^N word=0x(0|1e) import=(true|false) input=li'shi->ba'kua$	§12 frozen sentence-surface residual? (the re-guessed ba'kua rows — #594)
pinyin	^C[12] word=0x(0|1e) 	open: register row 37 (window behind the composition offset)
zhuyin	^C[12] 	open: register row 37 (window behind the composition offset)
TABLE
)

set +e
DECLARED="$declared" python3 - "$out" <<'PY'
import os, re, sys
out = sys.argv[1]
table = [line.split("\t") for line in os.environ["DECLARED"].splitlines() if line]
status = 0
for mode in ("pinyin", "zhuyin"):
    def cases(side):
        blocks, name = {}, None
        with open(f"{out}/{mode}-{side}.log", "rb") as log:
            for raw in log:
                if raw.startswith(b"== "):
                    name = raw[3:].decode("utf-8").rstrip("\n")
                    blocks[name] = []
                elif name is not None:
                    blocks[name].append(raw)
        return blocks
    pin, ox = cases("pin"), cases("ox")
    if list(pin) != list(ox):
        print(f"{mode}: FAIL — the two logs walk different cases")
        status = 2
        continue
    counts = {"identical": 0, "declared": 0}
    for name in pin:
        reason = next((r for m, rx, r in table if m == mode and re.search(rx, name)), None)
        same = pin[name] == ox[name]
        if same and reason is None:
            counts["identical"] += 1
        elif not same and reason is not None:
            counts["declared"] += 1
            print(f"{mode}: declared  {name}  [{reason}]")
        elif same:
            print(f"{mode}: STALE     {name}  now identical, declared [{reason}]")
            status = 2
        else:
            print(f"{mode}: DIVERGENT {name}")
            status = 2
    print(f"{mode}: {len(pin)} cases, {counts['identical']} identical, "
          f"{counts['declared']} declared divergent")
sys.exit(status)
PY
status=$?
set -e

if [[ $status -eq 0 ]]; then
    echo "RESULT: as declared"
    [[ $clean -eq 1 ]] && rm -rf "$out"
else
    echo "RESULT: FAIL (logs: $out)"
fi
exit "$status"
