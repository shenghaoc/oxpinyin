#!/usr/bin/env bash
# user-dir-round-trip.sh — drop-in task 9's differential: the user dir
# round-trips between the pin-built libpinyin and oxpinyin, seamlessly.
#
#   Phase A: the pin trains a profile; oxpinyin opens it and its §9
#            exports must be line-identical to the pin's own dump.
#   Phase B: oxpinyin trains and saves a fresh profile through its C
#            ABI; the pin opens it and its dump must be line-identical
#            to the pin's dump of its own training of the same inputs.
#
# Usage:
#   user-dir-round-trip.sh <oracle-prefix> [input ...]
#
#   <oracle-prefix>  a prefix built by tools/oracle/build-oracle.sh
#                    (default DBM tkrzw — build oxpinyin with the
#                    matching backend feature; the script uses the
#                    workspace default).
#   input            pinyin strings to train on (default: nihao nisha).
#
# Requires: cc, pkg-config, glib-2.0 dev files, and the rust toolchain.
set -euo pipefail

prefix=${1:?usage: user-dir-round-trip.sh <oracle-prefix> [input ...]}
shift
inputs=("$@")
[[ ${#inputs[@]} -gt 0 ]] || inputs=(nihao nisha)

script_dir=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$script_dir/../.." && pwd)
data_dir=$prefix/lib/libpinyin/data
[[ -d $data_dir ]] || { printf 'no data dir under prefix: %s\n' "$data_dir" >&2; exit 1; }

header_dir=$(find "$prefix/include" -maxdepth 1 -type d -name 'libpinyin-*' | head -1)
[[ -n $header_dir ]] || { printf 'no libpinyin header dir under %s\n' "$prefix/include" >&2; exit 1; }
so=$(find "$prefix/lib" -maxdepth 1 -name 'libpinyin.so.15*' | head -1)
[[ -n $so ]] || { printf 'no libpinyin.so.15 under %s\n' "$prefix/lib" >&2; exit 1; }

work=$(mktemp -d "${TMPDIR:-/tmp}/oxpinyin-user-rt.XXXXXX")
trap 'rm -rf "$work"' EXIT

# ---- the pin-side driver -------------------------------------------------
cc -O2 -o "$work/user_driver" "$script_dir/user_driver.c" \
   -I"$header_dir" $(pkg-config --cflags glib-2.0) \
   -L"$prefix/lib" -lpinyin $(pkg-config --libs glib-2.0) \
   -Wl,-rpath,"$(dirname "$so")"

mkdir "$work/pin" "$work/ox"

printf '== Phase A: the pin trains, oxpinyin reads ==\n'
"$work/user_driver" train "$data_dir" "$work/pin" "${inputs[@]}"
"$work/user_driver" dump "$data_dir" "$work/pin" | sort > "$work/pin.dump"
[[ -s $work/pin.dump ]] || { printf 'the pin dump is empty\n' >&2; exit 1; }

printf '== Phase B: oxpinyin trains, the pin reads ==\n'
OX_SYSTEM_DIR="$data_dir" \
OX_PIN_TRAINED_DIR="$work/pin" \
OX_PIN_DUMP="$work/pin.dump" \
OX_OX_TRAINED_DIR="$work/ox" \
OX_OX_DUMP="$work/ox.dump" \
OX_INPUTS="${inputs[*]}" \
cargo test --locked --manifest-path "$root/Cargo.toml" -p oxpinyin-capi \
    --test user_dir_round_trip -- --ignored --nocapture

"$work/user_driver" dump "$data_dir" "$work/ox" | sort > "$work/pin-reads-ox.dump"

if diff -u "$work/pin.dump" "$work/pin-reads-ox.dump" > "$work/diff.txt"; then
    printf 'IDENTICAL: the pin reads oxpinyin'\''s profile as its own (%s rows)\n' \
        "$(wc -l < "$work/pin.dump" | tr -d ' ')"
else
    cat "$work/diff.txt" >&2
    printf 'DIVERGED: the pin'\''s export of oxpinyin'\''s profile differs\n' >&2
    exit 1
fi
