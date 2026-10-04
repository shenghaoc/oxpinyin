#!/usr/bin/env bash
# Lane C contract differential: pin against subject, one case per behaviour.
# Usage: run-contract-diff.sh <bdb|kc|tkrzw> <oracle-prefix> <pinyin-so> \
#            [<zhuyin-so>] [-- contract-diff.py options]
# Examples:
#   run-contract-diff.sh bdb ~/.local/opt/pinyin-oracle target/debug/libpinyin_capi.so
#   ... -- --cases default-option-parse,default-option-single-key
#   ... -- --expect-parent      # against the parent build: fixes must differ
# Needs python3 and libglib-2.0.so.0. The case list and the comparison rules
# are in contract-diff.py; the exit status is 0 only when every selected case
# matches the pin (or, with --expect-parent, when every non-control case
# differs and every control case matches).
set -euo pipefail
cell=${1:?backend: bdb, kc or tkrzw}
prefix=${2:?oracle prefix}
pinyin_so=${3:?subject libpinyin}
shift 3
zhuyin=()
if [[ $# -gt 0 && $1 != -- ]]; then
    zhuyin=(--zhuyin-so "$1")
    shift
fi
[[ ${1:-} == -- ]] && shift
case "$cell" in bdb | kc | tkrzw) ;; *) echo "unknown backend: $cell" >&2; exit 1 ;; esac
script_dir=$(cd "$(dirname "$0")" && pwd)
exec python3 "$script_dir/contract-diff.py" "$cell" "$prefix" "$pinyin_so" \
    "${zhuyin[@]}" "$@"
