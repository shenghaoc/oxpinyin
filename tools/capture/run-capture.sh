#!/usr/bin/env bash
set -euo pipefail

EXPECTED_PIN_REF='libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw'

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
header=$prefix/include/libpinyin-2.11.92/pinyin.h
shared_object=$prefix/lib/libpinyin.so.15.0.0
manifest=$prefix/oracle-pin.txt
data_manifest=$prefix/oracle-data.sha256
data_unstable_manifest=$prefix/oracle-data-unstable.sha256

fail() {
	printf '%s\n' "$1" >&2
	exit 1
}

manifest_value() {
	local wanted=$1 key value
	while IFS='=' read -r key value; do
		if [[ $key == "$wanted" ]]; then
			printf '%s\n' "$value"
			return 0
		fi
	done <"$manifest"
	return 1
}

check_file_sha256() {
	local key=$1 path=$2 expected
	expected=$(manifest_value "$key") || fail "missing manifest field: $key"
	printf '%s  %s\n' "$expected" "$path" | sha256sum --check --status ||
		fail "oracle checksum mismatch: $path"
}

[[ -f $manifest ]] || fail "oracle pin manifest not found: $manifest"
[[ -f $header ]] || fail "pinned pinyin.h not found: $header"
[[ -f $shared_object ]] || fail "pinned shared object not found: $shared_object"
[[ -d $system_dir ]] || fail "pinned data directory not found: $system_dir"
[[ -f $data_manifest ]] || fail "oracle data manifest not found: $data_manifest"
# A prefix built before the manifest split records no
# data_unstable_manifest_sha256 and has no unstable manifest: its
# oracle-data.sha256 covers all 23 files, so the gate check below still
# covers the whole payload. The unstable checks apply only when the pin
# records the field.
has_unstable_manifest=0
if manifest_value data_unstable_manifest_sha256 >/dev/null; then
	has_unstable_manifest=1
	[[ -f $data_unstable_manifest ]] ||
		fail "oracle unstable data manifest not found: $data_unstable_manifest"
fi

[[ $(manifest_value schema) == pinyin-oracle-v1 ]] ||
	fail 'unsupported oracle manifest schema'
oracle_pin_ref=$(manifest_value pin_ref) || fail 'missing oracle pin ref'
[[ $oracle_pin_ref == "$EXPECTED_PIN_REF" ]] ||
	fail 'oracle pin ref does not match capture pin'
[[ $(manifest_value dbm) == Tkrzw ]] || fail 'oracle DBM is not Tkrzw'
check_file_sha256 header_sha256 "$header"
check_file_sha256 shared_object_sha256 "$shared_object"
check_file_sha256 data_manifest_sha256 "$data_manifest"
(
	cd "$prefix"
	sha256sum --check --status oracle-data.sha256
) || fail 'oracle data payload checksum mismatch'
# The unstable manifest covers the six files libpinyin does not generate
# reproducibly. Checking it here keeps the payload tamper-evident within
# this prefix; it says nothing about whether another build of the same pin
# would agree, and nothing compares it across prefixes.
if ((has_unstable_manifest)); then
	check_file_sha256 data_unstable_manifest_sha256 "$data_unstable_manifest"
	(
		cd "$prefix"
		sha256sum --check --status oracle-data-unstable.sha256
	) || fail 'oracle unstable data payload checksum mismatch'
fi

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
	"$oracle_pin_ref" >"$output_dir/f-a.txt"
LD_LIBRARY_PATH=$prefix/lib \
	"$build_dir/pinyin-capture" F-C "$system_dir" "$build_dir/user-f-c" \
	"$oracle_pin_ref" >"$output_dir/f-c.txt"
