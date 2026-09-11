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
    --test user_dir_round_trip -- --ignored --nocapture a_pin_profile

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

printf '== Phase D: oxpinyin trains its own profile, the pin reads it ==\n'
# The reverse direction: a profile oxpinyin created from scratch, not one
# it loaded from the pin. The Rust side trains through the production
# Runtime into $work/ox and saves; the pin then opens that directory and
# renders it. The assertion is not byte-equality with Phase A — the two
# libraries segment and train the same input differently (the n-best
# trellis, class (a)) — but that the pin can open the profile, walks a
# non-empty export, and the user-phrase rows it renders carry the
# remember-input phrases both sides trained. A file oxpinyin wrote that
# the pin cannot parse is the failure this phase exists to catch.
OX_SYSTEM_DIR="$data_dir" \
OX_OWNED_DIR="$work/ox" \
OX_INPUTS="${inputs[*]}" \
OX_EXPECTED_PHRASES="$work/expected-phrases" \
cargo test --locked --manifest-path "$root/Cargo.toml" -p oxpinyin-runtime \
    "${feature_flags[@]}" \
    --test user_dir_round_trip -- --ignored --nocapture oxpinyin_trains

"$work/user_driver" phrases "$data_dir" "$work/ox" | sort > "$work/pin-reads-ox-owned.dump"
rows=$(wc -l < "$work/pin-reads-ox-owned.dump" | tr -d ' ')
phrases=$(grep -c '^P' "$work/pin-reads-ox-owned.dump" || true)
if [[ $rows -eq 0 || $phrases -eq 0 ]]; then
    printf 'DIVERGED: the pin found nothing (or no phrases) in oxpinyin own profile (rows=%s phrases=%s)\n' \
        "$rows" "$phrases" >&2
    exit 1
fi
# Every trained phrase row must survive: compare the pin rendered phrase
# set against exactly what the test remembered, so a profile that lost
# rows reports DIVERGED rather than READABLE-on-one-survivor.
actual_phrases=$(awk -F'\t' '$1 == "P" {print $2}' "$work/pin-reads-ox-owned.dump" | sort)
expected_phrases=$(sort "$work/expected-phrases")
if [[ "$actual_phrases" != "$expected_phrases" ]]; then
    printf 'DIVERGED: the pin rendered phrase set differs from what oxpinyin remembered\n' >&2
    diff <(printf '%s\n' "$expected_phrases") <(printf '%s\n' "$actual_phrases") >&2 || true
    exit 1
fi
printf 'READABLE: the pin loads and renders oxpinyin own profile (%s rows, %s phrase rows, all expected)\n' \
    "$rows" "$phrases"
