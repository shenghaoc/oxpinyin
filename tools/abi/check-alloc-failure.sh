#!/usr/bin/env bash
# Debug-symbol, Linux/glibc C-consumer gate: eight logged class-(c) refusals.
# Pin allocations and GArray growth stay fatal; the disabled control must fail.
# Run through just heavy after building both capi packages with debug symbols.
# Usage: check-alloc-failure.sh PIN_PREFIX PINYIN_SO ZHUYIN_SO
set -euo pipefail
cd "$(dirname "$0")/../.."
[[ $# == 3 ]] || { echo 'expected pin prefix and two subject shared libraries' >&2; exit 2; }
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
read -ra glib_flags <<< "$(pkg-config --cflags --libs glib-2.0)"
g++ -std=c++17 -O0 -g -Wall -Wextra -Werror -fno-builtin -rdynamic \
 tools/abi/alloc-failure.cc "${glib_flags[@]}" -ldl -o "$work/probe"
g++ -std=c++17 -O0 -g -Wall -Wextra -Werror -fno-builtin -rdynamic \
 -DOXPINYIN_ALLOC_FAILURE_DISABLED tools/abi/alloc-failure.cc "${glib_flags[@]}" -ldl -o "$work/control"
for mode in pinyin zhuyin; do
 if [[ $mode == pinyin ]]; then subject=$2; else subject=$3; fi
 # Require exactly the owned_cstr and monomorphized owned_cstr_list function,
 # not their closures. Missing/stripped symbols cannot silently skip injection.
 nm -S -a "$subject" > "$work/symbols"
 mapfile -t strings < <(awk '$3 == "t" && $4 ~ /^_RNv.*3ffi10owned_cstr$/ {print $1, $2}' "$work/symbols")
 mapfile -t lists < <(awk '$3 == "t" && $4 ~ /^_RINvNt.*3ffi15owned_cstr_list/ {print $1, $2}' "$work/symbols")
 [[ ${#strings[@]} == 1 && ${#lists[@]} == 1 ]] || { echo "missing unique adapter symbols: $subject" >&2; exit 2; }
 read -r str_offset str_size <<< "${strings[0]}"
 read -r list_offset list_size <<< "${lists[0]}"
 # The reference prefix stores both facades' common system data under libpinyin.
 data=$1/lib/libpinyin/data
 "$work/probe" "$subject" "$data" "$mode" subject "$str_offset" "$str_size" "$list_offset" "$list_size"
 "$work/probe" "$1/lib/lib$mode.so" "$data" "$mode" pin 0 0 0 0
 if "$work/control" "$subject" "$data" "$mode" subject "$str_offset" "$str_size" "$list_offset" "$list_size" > "$work/control.out" 2>&1; then
  echo "failure-disabled control unexpectedly passed: $mode" >&2; exit 1
 fi
 echo "$mode injection-disabled negative control: rejected"
done
