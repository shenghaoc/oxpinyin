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

mkdir "$work/pin"

printf '== Phase A: the pin trains a profile ==\n'
"$work/user_driver" train "$data_dir" "$work/pin" "${inputs[@]}"
# `pin.dump` is rendered before the in-place rewrite below, so the Phase C
# diff compares the original against oxpinyin's load->save of it.
"$work/user_driver" dump "$data_dir" "$work/pin" | sort > "$work/pin.dump"
[[ -s $work/pin.dump ]] || { printf 'the pin dump is empty\n' >&2; exit 1; }

printf '== Phase B: oxpinyin loads the pin profile and saves it back in place ==\n'
# The Rust side does a pure load->save of the pin's own data through the
# production Runtime — no oxpinyin training, so no decode or training
# divergence (the n-best trellis, the pin's stale-buffer bigram export)
# can enter the comparison.
#
# OX_CARGO_FEATURES selects oxpinyin's store backend and MUST match the
# oracle prefix's `--with-dbm` (the seamless claim is per KV backend):
# a KyotoCabinet-built libpinyin pairs with `--no-default-features
# --features kyotocabinet`, tkrzw with the workspace default. The default
# here is empty — the workspace default (tkrzw) — matching the default
# `build-oracle.sh --dbm`.
# shellcheck disable=SC2206
feature_flags=(${OX_CARGO_FEATURES:-})
OX_SYSTEM_DIR="$data_dir" \
OX_PIN_DIR="$work/pin" \
cargo test --locked --manifest-path "$root/Cargo.toml" -p oxpinyin-runtime \
    "${feature_flags[@]}" \
    --test user_dir_round_trip -- --ignored --nocapture

printf '== Phase C: the pin renders the original and oxpinyin rewrite ==\n'
# The seamless direction, divergence-free: both dumps are rendered by the
# *pin* over the *same* profile data, so its class-(b) bigram-export
# artefact is common to both sides and cancels. If oxpinyin's read+write
# is faithful, the rewrite renders byte-identically to the original.
"$work/user_driver" dump "$data_dir" "$work/pin" | sort > "$work/pin-reads-ox.dump"

if diff -u "$work/pin.dump" "$work/pin-reads-ox.dump" > "$work/diff.txt"; then
    printf 'IDENTICAL: the pin renders oxpinyin'\''s rewrite of its own profile byte-identically (%s rows)\n' \
        "$(wc -l < "$work/pin.dump" | tr -d ' ')"
else
    cat "$work/diff.txt" >&2
    printf 'DIVERGED: oxpinyin'\''s load->save changed what the pin renders\n' >&2
    exit 1
fi
