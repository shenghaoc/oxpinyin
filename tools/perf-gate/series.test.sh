#!/usr/bin/env bash
# series.test.sh — the behaviours tools/perf-gate/series.py must hold to.
#
# Each case here is a rule from
# docs/perf/ci-perf-size-gate-proposal-2026-09-09.md, and several of them
# encode a mistake the rejected per-PR proposal made: absence read as zero, a
# move attributed to a cause the data cannot carry, an off-cadence sample
# becoming the predecessor.
#
# Pure JSON in, exit code out. Run: tools/perf-gate/series.test.sh

set -euo pipefail
cd "$(dirname "$0")"

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT
pass=0
fail=0

# write_snap <file> <python patch over `d`>
write_snap() {
	python3 - "$1" "$2" <<'PY'
import json, sys
path, expr = sys.argv[1], sys.argv[2]
d = {
    "captured_utc": "2026-09-12T03:00:00Z",
    "commit": "a" * 40,
    "event": "schedule",
    "environment": {
        "kernel": "Linux 6.12.0 x86_64", "runner_image": "ubuntu24/1",
        "rustc": "rustc 1.97.1", "cc": "gcc 15.3.0", "glibc": "2.42",
        "valgrind": "3.26.0", "binutils": "2.47", "backend": "tkrzw",
        "fixture_sha256": "f" * 64, "cpu_pinned": True,
        "harness_commit": "b" * 40,
    },
    "artifacts": {},
    "metrics": {
        "section_sum": 1000000, "stripped_size": 1446528,
        "alloc_count_per_cycle": 120, "alloc_bytes_per_cycle": 40960,
        "ir_oxpinyin_object": 2000000,
        "rss_init_kib": 14000, "rss_cycle_kib": 23000,
    },
}
exec(expr)
json.dump(d, open(path, "w"))
PY
}

# expect <code> <label> <series-dir> [extra args...]
expect() {
	local want=$1 label=$2 dir=$3; shift 3
	local got=0
	./series.py --snapshot "$WORK/snap.json" --series-dir "$dir" "$@" \
		>"$WORK/out" 2>&1 || got=$?
	if [ "$got" = "$want" ]; then
		printf 'ok   %-52s exit %s\n' "$label" "$got"
		pass=$((pass + 1))
	else
		printf 'FAIL %-52s want %s got %s\n' "$label" "$want" "$got"
		sed 's/^/       /' "$WORK/out"
		fail=$((fail + 1))
	fi
}

says() { # <label> <grep pattern>
	if grep -q "$2" "$WORK/out"; then
		printf 'ok   %-52s reports it\n' "$1"
		pass=$((pass + 1))
	else
		printf 'FAIL %-52s missing from the report: %s\n' "$1" "$2"
		fail=$((fail + 1))
	fi
}

# --- an empty series: nothing to compare, and that is not an error ---------
EMPTY="$WORK/empty"; mkdir -p "$EMPTY"
write_snap "$WORK/snap.json" 'pass'
expect 0 "first sample, no predecessor" "$EMPTY"
says "first sample explains itself" "nothing to compare"

# --- the append rule: only the schedule writes -----------------------------
DIR="$WORK/series"; mkdir -p "$DIR"
write_snap "$WORK/snap.json" 'd["captured_utc"] = "2026-09-11T03:00:00Z"'
expect 0 "a run without --append persists nothing" "$DIR"
[ "$(find "$DIR" -name '*.json' | wc -l)" -eq 0 ] &&
	{ printf 'ok   %-52s series still empty\n' "no-append leaves no sample"; pass=$((pass + 1)); } ||
	{ printf 'FAIL %-52s a sample was written\n' "no-append leaves no sample"; fail=$((fail + 1)); }
says "no-append says why" "only on the nightly schedule"

expect 0 "--append persists the sample" "$DIR" --append
[ "$(find "$DIR" -name '*.json' | wc -l)" -eq 1 ] &&
	{ printf 'ok   %-52s one sample stored\n' "append writes exactly one"; pass=$((pass + 1)); } ||
	{ printf 'FAIL %-52s wrong sample count\n' "append writes exactly one"; fail=$((fail + 1)); }

# --- a quiet night: small moves are the normal state of a live series ------
write_snap "$WORK/snap.json" 'd["metrics"]["section_sum"] = 1001000'
expect 0 "a 0.1% move is not flagged" "$DIR"

# --- a jump, environment unchanged: flag it --------------------------------
write_snap "$WORK/snap.json" 'd["metrics"]["section_sum"] = 1050000'
expect 1 "a 5% size jump is flagged" "$DIR"
says "the flag names the metric" "section_sum"

write_snap "$WORK/snap.json" 'd["metrics"]["alloc_count_per_cycle"] = 140'
expect 1 "an allocation jump is flagged" "$DIR"

# --- the same jump with the environment moved: report, do not attribute ----
write_snap "$WORK/snap.json" '
d["metrics"]["section_sum"] = 1050000
d["environment"]["cc"] = "gcc 16.1.0"'
expect 0 "a jump across a toolchain change is not flagged" "$DIR"
says "and says why it is not" "unattributable"
says "and names what moved" "cc"

# --- RSS is a trend, never a trigger ---------------------------------------
write_snap "$WORK/snap.json" 'd["metrics"]["rss_cycle_kib"] = 30000'
expect 0 "a 30% RSS move is reported, not flagged" "$DIR"
says "RSS is marked as trend only" "trend only"

# --- absence is not zero ---------------------------------------------------
write_snap "$WORK/snap.json" '
d["metrics"]["ir_oxpinyin_object"] = None
d["metrics"]["alloc_count_per_cycle"] = None'
expect 0 "a null metric is not a 100% improvement" "$DIR"
says "a null is called out as unmeasured" "not measured"

# --- a zero predecessor is not a free pass ---------------------------------
# alloc_count_per_cycle can legitimately be 0. Going 0 -> positive is the one
# transition most worth seeing, and a percentage of a zero baseline hides it.
ZERO="$WORK/zero"; mkdir -p "$ZERO"
write_snap "$WORK/snap.json" '
d["captured_utc"] = "2026-09-11T03:00:00Z"
d["metrics"]["alloc_count_per_cycle"] = 0'
expect 0 "seed a series with a zero metric" "$ZERO" --append
write_snap "$WORK/snap.json" 'd["metrics"]["alloc_count_per_cycle"] = 5'
expect 1 "zero to positive is flagged" "$ZERO"
says "and is described as from zero" "from zero"

write_snap "$WORK/snap.json" 'd["metrics"]["alloc_count_per_cycle"] = 0'
expect 0 "zero to zero is not flagged" "$ZERO"

# --- a malformed snapshot is a tool error, not a finding -------------------
printf 'not json {' > "$WORK/snap.json"
expect 2 "malformed snapshot" "$DIR"

write_snap "$WORK/snap.json" 'del d["metrics"]'
expect 2 "snapshot with no metrics block" "$DIR"

# --- structurally invalid input, which is valid JSON -----------------------
# Each of these parses, then blows up somewhere deep in the comparison unless
# the shape is checked first.
write_snap "$WORK/snap.json" 'd["metrics"]["section_sum"] = "1000000"'
expect 2 "a string metric is malformed, not compared" "$DIR"

printf '[1, 2, 3]' > "$WORK/snap.json"
expect 2 "a JSON list is malformed" "$DIR"

# --- a corrupt predecessor degrades to 'no predecessor' --------------------
CORRUPT="$WORK/corrupt"; mkdir -p "$CORRUPT"
printf 'garbage' > "$CORRUPT/20260911T030000-aaaaaaaaaaaa.json"
write_snap "$WORK/snap.json" 'pass'
expect 0 "a corrupt predecessor does not fail the lane" "$CORRUPT"

# A predecessor that is valid JSON but the wrong shape takes the same path,
# rather than raising out of prev.get() or the subtraction.
SHAPE="$WORK/shape"; mkdir -p "$SHAPE"
printf '[1, 2, 3]' > "$SHAPE/20260911T030000-aaaaaaaaaaaa.json"
write_snap "$WORK/snap.json" 'pass'
expect 0 "a list predecessor is treated as missing" "$SHAPE"
says "and says so" "nothing to compare"

printf '{"metrics": {"section_sum": "big"}}' > "$SHAPE/20260911T030001-bbbbbbbbbbbb.json"
write_snap "$WORK/snap.json" 'pass'
expect 0 "a string-metric predecessor is treated as missing" "$SHAPE"

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
