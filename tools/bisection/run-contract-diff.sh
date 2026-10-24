#!/usr/bin/env bash
# Lane C contract differential: pin against subject, one case per behaviour.
# Usage: run-contract-diff.sh <bdb|kc|tkrzw> <oracle-prefix> <pinyin-so> \
#            [<zhuyin-so>] [-- contract-diff.py options]
# Examples:
#   run-contract-diff.sh bdb ~/.local/opt/pinyin-oracle target/debug/libpinyin_capi.so
#   ... -- --cases default-option-parse,default-option-single-key
#   ... -- --issue 640           # the exact cases registered under issue 640
#   ... -- --family zero-key     # every zero-key/... case
#   ... -- --issue 640 --plan-only   # print the selection plan and stop
#   ... -- --expect-parent      # against the parent build: fixes must differ
#   ... -- --jobs 4             # maximum cases at once, within heavy admission
#   ... -- --run-platform-excluded --cases ...  # run host-excluded cases; reasons on stderr
# Needs uv 0.13.1 or newer, managed CPython 3.15+gil and libglib-2.0.so.0. The case list, the tag index
# (`contract-diff.py --list-tags`) and the comparison rules are in
# contract-diff.py; an unknown tag or an empty selection is an error, never
# the default suite. The plan (case IDs, why each was selected, facades and
# artifacts) is printed to stderr before the run, and the last stdout line is
# the run's phase timing. The exit status is 0 only when every selected case
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
for name in UV_PYTHON PYTHONHOME PYTHONPATH; do
	if [[ -n ${!name:-} ]]; then
		printf 'FAIL: %s is not supported; unset it before running repository Python\n' "$name" >&2
		exit 2
	fi
done
command -v uv >/dev/null 2>&1 || {
	echo 'missing uv 0.13.1 or newer; install it and managed CPython 3.15+gil (docs/runbooks/setup.md)' >&2
	exit 127
}
exec uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal "$script_dir/contract-diff.py" "$cell" "$prefix" "$pinyin_so" \
    "${zhuyin[@]}" "$@"
