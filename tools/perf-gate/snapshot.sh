#!/usr/bin/env bash
# snapshot.sh — measure the tree once and emit one perf snapshot as JSON.
#
# One half of the nightly series described in
# docs/perf/ci-perf-size-gate-proposal-2026-09-09.md; tools/perf-gate/series.py
# is the other, and compares this sample against the previous night's.
#
# This is NOT a gate. It measures and records. Nothing here decides anything,
# nothing here fails a build over a number, and no threshold lives in this
# file. Wall clock is deliberately absent: the evidence against timing in CI
# is in the document above and is unchanged by moving to a nightly cadence.
#
# The environment is recorded WITH the sample, because the series can localize
# the night a level moved but cannot say what moved it. Without the
# environment beside each number, a toolchain bump and a large refactor are
# indistinguishable.
#
# Usage: tools/perf-gate/snapshot.sh --out snapshot.json [--backend tkrzw]

set -euo pipefail
cd "$(dirname "$0")/../.."

OUT=""
BACKEND=tkrzw
CPU=${PERF_CPU:-0}
CYCLES=${PERF_GATE_CYCLES:-8}
CG_CYCLES=${PERF_GATE_CG_CYCLES:-4}
RSS_PROCESSES=${PERF_GATE_RSS_PROCESSES:-5}

while [ $# -gt 0 ]; do
	case "$1" in
	--out) shift; OUT=${1:-} ;;
	--backend) shift; BACKEND=${1:-} ;;
	-h | --help) sed -n '2,19p' "$0"; exit 0 ;;
	*) echo "fatal: unknown argument $1" >&2; exit 2 ;;
	esac
	shift
done
[ -n "$OUT" ] || { echo "fatal: --out is required" >&2; exit 2; }

case "$BACKEND" in
tkrzw) EXT=tkt ;;
kyotocabinet) EXT=kct ;;
lmdb) EXT=lmdb ;;
redb) EXT=redb ;;
*) echo "fatal: unsupported backend '$BACKEND'" >&2; exit 2 ;;
esac
DATA="$PWD/fixtures/w3/$EXT"
[ -d "$DATA" ] || { echo "fatal: fixture dir $DATA missing" >&2; exit 2; }

have() { command -v "$1" >/dev/null 2>&1; }
for tool in cargo gcc readelf strip nm; do
	have "$tool" || { echo "fatal: $tool not found" >&2; exit 2; }
done

# taskset pins the measured processes when it is available. It is not on every
# image, and its absence must not silently change what is measured without
# saying so, so it is recorded in the sample.
TASKSET=()
PINNED=false
if have taskset; then TASKSET=(taskset -c "$CPU"); PINNED=true; fi

WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

echo "==> building the shipped artifact (cinstall, the product path)"
STAGE="$WORK/stage"
tools/packaging/install.sh libpinyin --prefix=/usr --destdir="$STAGE" \
	-- --no-default-features --features "$BACKEND,shipped" \
	--target-dir "$WORK/target-shipped" >&2
SHIPPED=$(find "$STAGE" -name 'libpinyin.so.15.0.0' -print -quit)
[ -n "$SHIPPED" ] || { echo "fatal: cinstall produced no libpinyin.so.15.0.0" >&2; exit 2; }
strip --strip-all "$SHIPPED"

# The runbook's rule, enforced rather than trusted: the alloc-count readers are
# never in a shipped artifact.
if nm -D "$SHIPPED" | grep -q oxpinyin_alloc_; then
	echo "fatal: the shipped artifact exports oxpinyin_alloc_* symbols" >&2
	exit 2
fi

echo "==> building the alloc-count artifact (a fixture; nothing installs it)"
ALLOC_RECIPE="cargo build --locked --release -p oxpinyin-capi --no-default-features --features $BACKEND,alloc-count"
CARGO_TARGET_DIR="$WORK/target-alloc" $ALLOC_RECIPE >&2
ALLOC_SO="$WORK/target-alloc/release/libpinyin_capi.so"
[ -f "$ALLOC_SO" ] || { echo "fatal: no alloc-count cdylib at $ALLOC_SO" >&2; exit 2; }

BISECT="$WORK/bisect"
gcc -std=gnu11 -O2 -o "$BISECT" tools/bisection/bisect.c -ldl

# ── size ────────────────────────────────────────────────────────────────
# The section sum is the resolving measure. Stripped file size is
# page-quantized on these hosts (perf-so-size-2026-09.md measured seven
# distinct probe cdylibs all reporting an identical 266,736 B), so a real
# few-KiB change can land with a zero file-size delta.
# Parsed in python rather than awk. `strtonum` is a gawk extension and Debian
# ships mawk, so the awk form died on this runner -- and it was wrong besides:
# readelf prints "  [ 1] .text", which awk splits as "[", "1]", ".text", so a
# $2 name test only ever matched two-digit section indices and $6 was the
# offset, not the size. It would have returned a plausible-looking wrong number
# wherever gawk happened to be installed.
section_sum() {
	readelf -S -W "$1" | python3 -c '
import re, sys
WANT = {".text", ".rodata", ".data.rel.ro", ".rela.dyn",
        ".eh_frame", ".eh_frame_hdr", ".gcc_except_table"}
# [Nr] Name Type Address Off Size ...  — all three numerics are hex.
ROW = re.compile(r"\s*\[\s*\d+\]\s+(\S+)\s+\S+\s+[0-9a-fA-F]+\s+[0-9a-fA-F]+\s+([0-9a-fA-F]+)")
total = 0
for line in sys.stdin:
    m = ROW.match(line)
    if m and m.group(1) in WANT:
        total += int(m.group(2), 16)
print(total)
'
}

# ── allocations ─────────────────────────────────────────────────────────
# bisect.c emits steady_alloc_{cycles,count,bytes} in speed mode, and only
# `if (alloc_steady_captured && cycles > 1)` — that is, only when the
# oxpinyin_alloc_* readers resolved. On an artifact built without
# --features alloc-count the keys are ABSENT, not zero. Absence is reported as
# null and series.py refuses to treat it as a measurement.
alloc_json() {
	"${TASKSET[@]}" env PERF_BACKEND=gate PERF_MODE=speed PERF_CYCLES="$CYCLES" \
		"$BISECT" --perf "$ALLOC_SO" "$DATA" 2>/dev/null |
		python3 -c '
import json, sys
row = json.loads(sys.stdin.readline())
n = row.get("steady_alloc_cycles", 0)
if not n or "steady_alloc_count" not in row:
    print(json.dumps({"alloc_count_per_cycle": None, "alloc_bytes_per_cycle": None}))
else:
    print(json.dumps({
        "alloc_count_per_cycle": row["steady_alloc_count"] // n,
        "alloc_bytes_per_cycle": row["steady_alloc_bytes"] // n,
    }))'
}

# ── RSS ─────────────────────────────────────────────────────────────────
# rss_kib/hwm_kib are nested under after_first and after_last, not at the top
# level. after_first is written after cycle 0 — a COMPLETED cycle, not after
# init — so the fields are named for that rather than for an init snapshot the
# speed mode never takes. Recorded as a trend; never a trigger.
rss_json() {
	local i
	for ((i = 0; i < RSS_PROCESSES; i++)); do
		"${TASKSET[@]}" env PERF_BACKEND=gate PERF_MODE=speed PERF_CYCLES="$CYCLES" \
			"$BISECT" --perf "$SHIPPED" "$DATA" 2>/dev/null
	done | python3 -c '
import json, statistics, sys
rows = [json.loads(l) for l in sys.stdin if l.strip()]
def med(tag, key):
    # Absence is not zero. A default of 0 here would record a missing
    # measurement as a real one and invent an RSS cliff in the series.
    vals = [r[tag][key] for r in rows
            if isinstance(r.get(tag), dict) and isinstance(r[tag].get(key), (int, float))]
    return int(statistics.median(vals)) if vals else None
print(json.dumps({"rss_first_cycle_kib": med("after_first", "rss_kib"),
                  "rss_last_cycle_kib": med("after_last", "rss_kib")}))'
}

# ── instructions ────────────────────────────────────────────────────────
# Optional by design: valgrind is not on every image, and a missing instrument
# reports null rather than zero. Attributed to the oxpinyin object only, so
# glibc and libtkrzw cannot move the number or mask a move in ours.
ir_json() {
	if ! have valgrind; then
		echo '{"ir_oxpinyin_object": null}'
		return
	fi
	local cg="$WORK/callgrind.out"
	"${TASKSET[@]}" env PERF_BACKEND=gate PERF_MODE=speed PERF_CYCLES="$CG_CYCLES" \
		valgrind --tool=callgrind --collect-atstart=no \
		--toggle-collect=bisect_perf_steady_unit \
		--callgrind-out-file="$cg" \
		"$BISECT" --perf "$SHIPPED" "$DATA" >/dev/null 2>&1 || {
		echo '{"ir_oxpinyin_object": null}'
		return
	}
	local ir
	ir=$(python3 "$PWD/tools/perf-gate/callgrind-ir.py" "$cg" libpinyin)
	printf '{"ir_oxpinyin_object": %s}\n' "$ir"
}

echo "==> measuring"
SIZE_SECTION=$(section_sum "$SHIPPED")
SIZE_STRIPPED=$(stat -c%s "$SHIPPED")
ALLOC=$(alloc_json)
RSS=$(rss_json)
IR=$(ir_json)

python3 - "$OUT" "$SHIPPED" "$DATA" "$ALLOC_RECIPE" <<PY
import hashlib, json, os, pathlib, subprocess, sys

out, shipped, data, alloc_recipe = sys.argv[1:5]

def run(*cmd):
    try:
        r = subprocess.run(cmd, capture_output=True, text=True)
        return r.stdout.strip().split("\n")[0] or "unknown"
    except OSError:
        return "unknown"

h = hashlib.sha256()
for p in sorted(q for q in pathlib.Path(data).rglob("*") if q.is_file()):
    h.update(p.name.encode()); h.update(p.read_bytes())

doc = {
    "captured_utc": run("date", "-u", "+%Y-%m-%dT%H:%M:%SZ"),
    "commit": run("git", "rev-parse", "HEAD"),
    "event": os.environ.get("GITHUB_EVENT_NAME", "local"),
    "run_url": (f'{os.environ["GITHUB_SERVER_URL"]}/{os.environ["GITHUB_REPOSITORY"]}'
                f'/actions/runs/{os.environ["GITHUB_RUN_ID"]}'
                if os.environ.get("GITHUB_RUN_ID") else None),
    # Recorded with the sample so a move can be read against whether the
    # environment moved too. The series cannot attribute cause without this.
    "environment": {
        "kernel": run("uname", "-srvm"),
        "runner_image": f'{os.environ.get("ImageOS", "local")}/{os.environ.get("ImageVersion", "local")}',
        "rustc": run("rustc", "-V"),
        "cc": run("cc", "--version"),
        "glibc": run("ldd", "--version"),
        "valgrind": run("valgrind", "--version"),
        "binutils": run("readelf", "--version"),
        "backend": "$BACKEND",
        "fixture_sha256": h.hexdigest(),
        # $PINNED is the JSON literal true/false, not Python's True/False.
        "cpu_pinned": json.loads("$PINNED"),
        "harness_commit": run("git", "log", "-1", "--format=%H", "--",
                              "tools/bisection/bisect.c", "tools/perf-gate"),
    },
    "artifacts": {
        "shipped_libpinyin": {
            "recipe": "tools/packaging/install.sh libpinyin --prefix=/usr --destdir=<stage> -- --no-default-features --features $BACKEND,shipped",
            "sha256": run("sha256sum", shipped).split()[0],
            "bytes": os.path.getsize(shipped),
        },
        "alloccount_libpinyin": {"recipe": alloc_recipe},
    },
    "metrics": {
        "section_sum": $SIZE_SECTION,
        "stripped_size": $SIZE_STRIPPED,
        **json.loads('''$ALLOC'''),
        **json.loads('''$RSS'''),
        **json.loads('''$IR'''),
    },
}
for field, value in (("commit", doc["commit"]),
                     ("harness_commit", doc["environment"]["harness_commit"])):
    if value == "unknown":
        sys.exit(f"fatal: {field} could not be resolved — a sample with no "
                 f"provenance must not enter the series (is this a git "
                 f"checkout, and is safe.directory set?)")

with open(out, "w") as fh:
    json.dump(doc, fh, indent=2, sort_keys=True)
    fh.write("\n")
print("wrote", out)
PY
