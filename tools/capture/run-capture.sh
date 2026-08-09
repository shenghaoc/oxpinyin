#!/usr/bin/env bash
set -euo pipefail

if (($# != 2)); then
	printf 'usage: %s ORACLE_PREFIX OUTPUT_DIR\n' "$0" >&2
	exit 2
fi

prefix=$(realpath "$1")
output_dir=$2
script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repo_root=$(cd "$script_dir/../.." && pwd)
build_dir=$repo_root/target/capture
system_dir=$prefix/lib/libpinyin/data
header=$prefix/include/libpinyin-2.11.91/pinyin.h
shared_object=$prefix/lib/libpinyin.so.15.0.0

[[ -f $header ]] || {
	printf 'pinned pinyin.h not found: %s\n' "$header" >&2
	exit 1
}
[[ -f $shared_object ]] || {
	printf 'pinned shared object not found: %s\n' "$shared_object" >&2
	exit 1
}
[[ -d $system_dir ]] || {
	printf 'pinned data directory not found: %s\n' "$system_dir" >&2
	exit 1
}

mkdir -p "$build_dir" "$output_dir"
rm -rf "$build_dir/user-f-a" "$build_dir/user-f-c"
mkdir -p "$build_dir/user-f-a" "$build_dir/user-f-c"

export PKG_CONFIG_PATH=$prefix/lib/pkgconfig
cc -std=c11 -Wall -Wextra -Werror \
	$(pkg-config --cflags libpinyin) \
	"$script_dir/capture.c" \
	-o "$build_dir/pinyin-capture" \
	$(pkg-config --libs libpinyin)

LD_LIBRARY_PATH=$prefix/lib \
	"$build_dir/pinyin-capture" F-A "$system_dir" "$build_dir/user-f-a" \
	>"$output_dir/f-a.nvr"
LD_LIBRARY_PATH=$prefix/lib \
	"$build_dir/pinyin-capture" F-C "$system_dir" "$build_dir/user-f-c" \
	>"$output_dir/f-c.nvr"
