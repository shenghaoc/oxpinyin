#!/usr/bin/env bash
# check.test.sh — exercises every exit path of tools/perf-gate/check.py.
#
# The gate's value rests on four exit codes staying distinct, so each one gets
# a case here, and so does the trap that motivated the ordering rule: two
# rounds agreeing on a `-1` allocation reading must fault, never pass, and
# never be read as an improvement that ratchets the baseline down.
#
# Pure JSON in, exit code out — no toolchain, no container, no fixtures. Run:
#   tools/perf-gate/check.test.sh

set -euo pipefail
cd "$(dirname "$0")"

CHECK=./check.py
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

pass=0
fail=0

# A baseline and a capture that agree on everything. Cases below patch one
# field of one of them with jq-free python and assert the resulting exit code.
cat > "$WORK/baseline.json" <<'JSON'
{
  "captured_utc": "2026-09-11T19:08:37Z",
  "fingerprint": {
    "image_digest": "sha256:aaaa",
    "runner_image": "ubuntu24/20260901.1",
    "kernel": "Linux 6.12.0 x86_64",
    "rustc": "rustc 1.97.1",
    "valgrind": "3.26.0",
    "fixture_sha256": "bbbb",
    "harness_commit": "cccc"
  },
  "artifacts": {
    "shipped_libpinyin": {
      "recipe": "tools/packaging/install.sh libpinyin --prefix=/usr --destdir=<stage> -- --no-default-features --features tkrzw,shipped",
      "sha256": "dddd", "needed": ["libtkrzw.so.1"], "bytes": 1446528
    }
  },
  "metrics": {
    "g1": {"section_sum": 1000000, "stripped_size": 1446528, "payload_bytes": 1500000},
    "g2": {"ir_oxpinyin_object": 2000000, "ir_program_total": 5000000},
    "g3": {"alloc_count_per_cycle": 120, "alloc_bytes_per_cycle": 40960, "peak_live_bytes": 100000},
    "g4": {"rss_init_kib": 14000, "rss_cycle_kib": 23000, "hwm_cycle_kib": 23100}
  }
}
JSON

cat > "$WORK/capture.json" <<'JSON'
{
  "captured_utc": "2026-09-11T19:20:00Z",
  "fingerprint": {
    "image_digest": "sha256:aaaa",
    "runner_image": "ubuntu24/20260901.1",
    "kernel": "Linux 6.12.0 x86_64",
    "rustc": "rustc 1.97.1",
    "valgrind": "3.26.0",
    "fixture_sha256": "bbbb",
    "harness_commit": "cccc"
  },
  "artifacts": {
    "shipped_libpinyin": {
      "recipe": "tools/packaging/install.sh libpinyin --prefix=/usr --destdir=<stage> -- --no-default-features --features tkrzw,shipped",
      "sha256": "dddd", "needed": ["libtkrzw.so.1"], "bytes": 1446528
    }
  },
  "rounds": {
    "g1": [
      {"section_sum": 1000000, "stripped_size": 1446528, "payload_bytes": 1500000},
      {"section_sum": 1000000, "stripped_size": 1446528, "payload_bytes": 1500000}
    ],
    "g2": [{"ir_oxpinyin_object": 2000000}, {"ir_oxpinyin_object": 2000100}],
    "g3": [
      {"alloc_count_per_cycle": 120, "alloc_bytes_per_cycle": 40960, "peak_live_bytes": 100000},
      {"alloc_count_per_cycle": 120, "alloc_bytes_per_cycle": 40960, "peak_live_bytes": 100000}
    ],
    "g4": [
      {"rss_init_kib": 14000, "rss_cycle_kib": 23000},
      {"rss_init_kib": 14020, "rss_cycle_kib": 23010}
    ]
  }
}
JSON

# patch <src> <dst> <python-expression-over-`d`>
patch() {
    python3 - "$1" "$2" "$3" <<'PY'
import json, sys
src, dst, expr = sys.argv[1], sys.argv[2], sys.argv[3]
d = json.load(open(src))
exec(expr)
json.dump(d, open(dst, "w"))
PY
}

# expect <code> <label> [extra check.py args...]
expect() {
    local want=$1 label=$2; shift 2
    local got=0
    "$CHECK" --baseline "$WORK/baseline.json" --capture "$WORK/probe.json" "$@" \
        >"$WORK/out" 2>&1 || got=$?
    if [ "$got" = "$want" ]; then
        printf 'ok   %-46s exit %s\n' "$label" "$got"
        pass=$((pass + 1))
    else
        printf 'FAIL %-46s want %s got %s\n' "$label" "$want" "$got"
        sed 's/^/       /' "$WORK/out"
        fail=$((fail + 1))
    fi
}

cp "$WORK/capture.json" "$WORK/probe.json"
expect 0 "clean capture"

# --- exit 3: BASELINE STALE ------------------------------------------------
patch "$WORK/capture.json" "$WORK/probe.json" 'd["fingerprint"]["kernel"] = "Linux 6.13.0 x86_64"'
expect 3 "kernel moved (not pinned by an image)"

patch "$WORK/capture.json" "$WORK/probe.json" 'd["fingerprint"]["harness_commit"] = "ffff"'
expect 3 "harness commit moved"

patch "$WORK/capture.json" "$WORK/probe.json" 'd["artifacts"]["shipped_libpinyin"]["recipe"] = "…--features tkrzw,shipped"'
expect 3 "recipe elided into a fragment"

patch "$WORK/capture.json" "$WORK/probe.json" 'd["artifacts"]["shipped_libpinyin"]["recipe"] = ""'
expect 3 "recipe empty"

patch "$WORK/capture.json" "$WORK/probe.json" 'd["artifacts"]["shipped_libpinyin"]["recipe"] = "install.sh libpinyin --destdir=<TODO> -- --features tkrzw"'
expect 3 "recipe carries an unexpanded placeholder"

printf 'not json {' > "$WORK/probe.json"
expect 3 "capture is not strict JSON"

# --- exit 2: INSTRUMENT FAULT ----------------------------------------------
# The case the ordering rule exists for: both rounds read -1, so they agree
# exactly, and -1 - (-1) == 0 would read as zero allocations per cycle.
patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g3"]:
    r["alloc_count_per_cycle"] = -1
    r["alloc_bytes_per_cycle"] = -1
    r["peak_live_bytes"] = -1'
expect 2 "alloc readers absent (-1), rounds agree"

patch "$WORK/capture.json" "$WORK/probe.json" 'd["rounds"]["g1"][1]["section_sum"] = 1000004'
expect 2 "G1 rounds disagree (deterministic metric)"

patch "$WORK/capture.json" "$WORK/probe.json" 'd["rounds"]["g2"][1]["ir_oxpinyin_object"] = 2400000'
expect 2 "G2 rounds disagree beyond the 0.05% floor"

patch "$WORK/capture.json" "$WORK/probe.json" 'd["rounds"]["g3"] = [d["rounds"]["g3"][0]]'
expect 2 "only one round captured"

patch "$WORK/capture.json" "$WORK/probe.json" 'del d["rounds"]["g2"]'
expect 2 "a metric is missing from the capture"

# --- exit 1: REGRESSION ----------------------------------------------------
patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g1"]: r["section_sum"] = 1010000'
expect 1 "G1 section sum over +0.5%"

patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g1"]: r["stripped_size"] = 1512064'
expect 1 "G1 stripped size up at all"

patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g2"]: r["ir_oxpinyin_object"] = 2100000'
expect 1 "G2 instructions +5%"

patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g3"]: r["alloc_count_per_cycle"] = 121'
expect 1 "G3 one more allocation per cycle"

# --- the thresholds that must NOT fire -------------------------------------
patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g1"]: r["section_sum"] = 1000100'
expect 0 "G1 growth inside the 4096 B floor"

patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g1"]: r["stripped_size"] = 1400000'
expect 0 "G1 size decrease"

patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g2"]: r["ir_oxpinyin_object"] = 2020000'
expect 0 "G2 +1% warns, does not fail"

patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g3"]: r["alloc_count_per_cycle"] = 119'
expect 0 "G3 one fewer allocation"

# --- G4: reported on a PR, gated on the nightly ----------------------------
patch "$WORK/capture.json" "$WORK/probe.json" '
for r in d["rounds"]["g4"]: r["rss_cycle_kib"] = 24500'
expect 0 "G4 +6.5% is reported on a PR"
expect 1 "G4 +6.5% is gated on the nightly" --nightly

printf '\n%d passed, %d failed\n' "$pass" "$fail"
[ "$fail" -eq 0 ]
