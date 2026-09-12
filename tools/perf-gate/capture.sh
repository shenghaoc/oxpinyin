#!/usr/bin/env bash
# capture.sh — produce one perf-gate capture: build, measure, emit JSON.
#
# The other half of tools/perf-gate/check.py. This runs steps 1-10 of the lane
# in docs/perf/ci-perf-size-gate-proposal-2026-09-09.md and writes a document
# check.py can read; check.py runs steps 11-13 and decides.
#
# **Nothing in .github/ runs this.** The lane is a CI-policy change and that
# ask is open. This exists so Phase 0 can be run by hand on a provisioned
# Linux host, which is the only place it can run at all: it needs a Linux
# toolchain, tkrzw, cargo-c, valgrind, and the committed w3 fixtures.
#
# Usage:
#   tools/perf-gate/capture.sh --out capture.json [--backend tkrzw] [--cpu 0]
#   tools/perf-gate/capture.sh --stub --out capture.json    # schema only
#
# --stub emits a schema-complete document WITHOUT measuring anything, so the
# handshake between this script and check.py can be tested with no toolchain.
# It stamps "stub": true, and check.py rejects that as an INSTRUMENT FAULT —
# a stub can never be mistaken for a measurement, which is the only way a
# convenience like this is safe to ship.
#
# What is NOT verified here: on a host without the toolchain, only the --stub
# path is exercised. The measurement path below is written against the same
# harness the existing runners drive (tools/bisection/bisect.c, PERF_MODE,
# taskset) but has not been run end to end at the time of writing; Phase 0 is
# what runs it. Said plainly rather than left to be discovered.

set -euo pipefail
cd "$(dirname "$0")/../.."
REPO_ROOT=$(pwd)

OUT=""
BACKEND=tkrzw
CPU=${PERF_CPU:-0}
STUB=0
CG_CYCLES=${CG_CYCLES:-4}
G3_CYCLES=${G3_CYCLES:-8}
G4_PROCESSES=${G4_PROCESSES:-10}

while [ $# -gt 0 ]; do
	case "$1" in
	--out) shift; OUT=${1:-} ;;
	--backend) shift; BACKEND=${1:-} ;;
	--cpu) shift; CPU=${1:-} ;;
	--stub) STUB=1 ;;
	-h | --help) sed -n '2,30p' "$0"; exit 0 ;;
	*) echo "fatal: unknown argument $1" >&2; exit 2 ;;
	esac
	shift
done
[ -n "$OUT" ] || { echo "fatal: --out is required" >&2; exit 2; }

# The fixture directory whose name matches the compiled backend. Mismatching
# these is how a capture ends up measuring a different workload than it says.
case "$BACKEND" in
tkrzw) FIXTURE_EXT=tkt ;;
kyotocabinet) FIXTURE_EXT=kct ;;
lmdb) FIXTURE_EXT=lmdb ;;
redb) FIXTURE_EXT=redb ;;
*) echo "fatal: unsupported backend '$BACKEND'" >&2; exit 2 ;;
esac
DATA="$REPO_ROOT/fixtures/w3/$FIXTURE_EXT"

# The one placeholder a recipe may carry: the staging directory is a mktemp
# path with no bearing on the artifact, and recording it verbatim would change
# the recipe string on every run and make the fingerprint unequal to itself.
STAGE_TOKEN='<stage>'

emit_json() { # <python expression building `doc`>
	python3 - "$OUT" "$1" <<'PY'
import json, sys
out, expr = sys.argv[1], sys.argv[2]
doc = {}
exec(expr)
with open(out, "w") as fh:
    json.dump(doc, fh, indent=2, sort_keys=True)
    fh.write("\n")
PY
}

if [ "$STUB" = 1 ]; then
	# Representative shape, no measurement. Every field the checker reads is
	# present so the schemas can be compared; "stub" is what stops it ever
	# being read as a result.
	emit_json '
doc = {
    "stub": True,
    "captured_utc": "1970-01-01T00:00:00Z",
    "fingerprint": {
        "image_digest": "sha256:stub", "apt_snapshot": "stub",
        "runner_image": "stub", "kernel": "stub", "rustc": "stub",
        "glibc": "stub", "libtkrzw": "stub", "valgrind": "stub",
        "readelf": "stub", "fixture_sha256": "stub", "harness_commit": "stub",
    },
    "artifacts": {
        "shipped_libpinyin": {
            "recipe": "tools/packaging/install.sh libpinyin --prefix=/usr --destdir=<stage> -- --no-default-features --features tkrzw,shipped",
            "installed_name": "libpinyin.so.15.0.0",
            "sha256": "stub", "needed": [], "bytes": 0,
        },
    },
    "rounds": {
        "g1": [{"section_sum": 0, "stripped_size": 0, "payload_bytes": 0}] * 2,
        "g2": [{"ir_oxpinyin_object": 0, "ir_program_total": 0}] * 2,
        "g3": [{"alloc_count_per_cycle": 0, "alloc_bytes_per_cycle": 0,
                "peak_live_bytes": 0}] * 2,
        "g4": [{"rss_init_kib": 0, "rss_cycle_kib": 0, "hwm_cycle_kib": 0}] * 2,
    },
}'
	echo "wrote $OUT (stub: schema only, no measurement)"
	exit 0
fi

# ── 1. environment probe: recorded, never depended on ────────────────────
PMU=unavailable
if gcc -O0 -o /tmp/probe-pmu tools/env-probe/probe-perf-event-open.c 2>/dev/null &&
	/tmp/probe-pmu >/dev/null 2>&1; then
	PMU=available
fi
echo "perf_event_open: $PMU (recorded; callgrind is the instrument either way)"

need() { command -v "$1" >/dev/null || { echo "fatal: $1 not found" >&2; exit 2; }; }
need cargo
need valgrind
need readelf
need strip
[ -d "$DATA" ] || { echo "fatal: fixture dir $DATA missing" >&2; exit 2; }

# ── 2-5. build the artifacts ─────────────────────────────────────────────
build_cinstall() { # <library> <features> <out-so>
	local library=$1 features=$2 out=$3 stage target
	stage=$(mktemp -d); target=$(mktemp -d)
	tools/packaging/install.sh "$library" --prefix=/usr --destdir="$stage" \
		-- --no-default-features --features "$features" \
		--target-dir "$target" >&2
	cp "$(find "$stage" -name "${library#lib}*.so.15.0.0" -o -name "$library.so.15.0.0" |
		head -1)" "$out"
	strip --strip-all "$out"
	# The recipe recorded is the command actually run, with only the scratch
	# staging path normalized. check.py rejects anything less complete.
	printf 'tools/packaging/install.sh %s --prefix=/usr --destdir=%s -- --no-default-features --features %s\n' \
		"$library" "$STAGE_TOKEN" "$features"
}

SHIPPED_SO=$(mktemp -u /tmp/perf-gate-shipped-XXXX.so)
ZHUYIN_SO=$(mktemp -u /tmp/perf-gate-zhuyin-XXXX.so)
ALLOC_SO=$(mktemp -u /tmp/perf-gate-alloc-XXXX.so)

# libpinyin takes `shipped`; libzhuyin has no such feature (its [features]
# block is capi, default and the four backends), so passing one is a build
# error rather than a no-op.
SHIPPED_RECIPE=$(build_cinstall libpinyin "$BACKEND,shipped" "$SHIPPED_SO")
ZHUYIN_RECIPE=$(build_cinstall libzhuyin "$BACKEND" "$ZHUYIN_SO")

ALLOC_RECIPE="cargo build --locked --release -p oxpinyin-capi --no-default-features --features $BACKEND,alloc-count"
$ALLOC_RECIPE >&2
cp target/release/libpinyin_capi.so "$ALLOC_SO"

# ── 6. the guard: alloc-count must never reach a shipped artifact ────────
if nm -D "$SHIPPED_SO" | grep -q oxpinyin_alloc_; then
	echo "INSTRUMENT FAULT: the shipped artifact exports oxpinyin_alloc_* symbols" >&2
	exit 2
fi

BISECT=$(mktemp -u /tmp/perf-gate-bisect-XXXX)
gcc -std=gnu11 -O2 -o "$BISECT" tools/bisection/bisect.c -ldl

# ── 7-10. measure ────────────────────────────────────────────────────────
section_sum() { # <so>
	readelf -S "$1" | awk '
		/\.text|\.rodata|\.data\.rel\.ro|\.rela\.dyn|\.eh_frame|\.gcc_except_table/ {
			for (i = 1; i <= NF; i++) if ($i ~ /^[0-9a-f]{6,}$/) size = $i
			total += strtonum("0x" size)
		}
		END { print total + 0 }'
}

ir_for() { # <so> -- oxpinyin object Ir only, not PROGRAM TOTALS
	local so=$1 outfile
	outfile=$(mktemp -u /tmp/perf-gate-cg-XXXX)
	taskset -c "$CPU" env PERF_BACKEND=gate PERF_MODE=speed PERF_CYCLES="$CG_CYCLES" \
		valgrind --tool=callgrind --collect-atstart=no \
		--toggle-collect=bisect_perf_steady_unit \
		--callgrind-out-file="$outfile" \
		"$BISECT" --perf "$so" "$DATA" >/dev/null 2>&1
	# Sum the cost lines attributed to the oxpinyin object, so glibc and
	# libtkrzw cannot move the gated number or mask a regression in ours.
	python3 - "$outfile" "$so" <<'PY'
import re, sys
out, so = sys.argv[1], sys.argv[2]
name = so.rsplit("/", 1)[-1]
total, inside = 0, False
for line in open(out, errors="replace"):
    if line.startswith("ob="):
        inside = name in line or "libpinyin" in line
    elif inside and re.match(r"^[0-9+\-*]", line):
        parts = line.split()
        if len(parts) >= 2 and parts[1].isdigit():
            total += int(parts[1])
print(total)
PY
}

alloc_round() { # emits the three G3 fields as one JSON object
	# bisect.c prints steady_alloc_{cycles,count,bytes} in speed mode, but
	# only `if (alloc_steady_captured && cycles > 1)` — that is, only when the
	# oxpinyin_alloc_* readers resolved. On an artifact built without
	# --features alloc-count the keys are ABSENT rather than zero, which is
	# the honest signal and must not be silently defaulted. Absence is mapped
	# to -1, the convention bisect.c uses elsewhere, and check.py faults on
	# either form before it compares anything.
	taskset -c "$CPU" env PERF_BACKEND=gate PERF_MODE=speed PERF_CYCLES="$G3_CYCLES" \
		"$BISECT" --perf "$ALLOC_SO" "$DATA" |
		python3 -c '
import json, sys
row = json.loads(sys.stdin.readline())
cycles = row.get("steady_alloc_cycles", 0)
if not cycles or "steady_alloc_count" not in row:
    print(json.dumps({"alloc_count_per_cycle": -1, "alloc_bytes_per_cycle": -1,
                      "peak_live_bytes": -1}))
else:
    print(json.dumps({
        "alloc_count_per_cycle": row["steady_alloc_count"] // cycles,
        "alloc_bytes_per_cycle": row["steady_alloc_bytes"] // cycles,
        "peak_live_bytes": row.get("alloc_init", {}).get("peak_live_bytes", -1),
    }))'
}

rss_pass() { # -- one G4 pass: medians over G4_PROCESSES processes
	# rss_kib/hwm_kib are nested under after_first (the post-init snapshot)
	# and after_last (post-cycle), not at the top level.
	local i
	for ((i = 0; i < G4_PROCESSES; i++)); do
		taskset -c "$CPU" env PERF_BACKEND=gate PERF_MODE=speed \
			PERF_CYCLES="$G3_CYCLES" "$BISECT" --perf "$SHIPPED_SO" "$DATA"
	done | python3 -c '
import json, statistics, sys
rows = [json.loads(line) for line in sys.stdin if line.strip()]
if not rows:
    sys.exit("fatal: no rows from the RSS pass")
def med(tag, key):
    return int(statistics.median(r.get(tag, {}).get(key, 0) for r in rows))
print(json.dumps({
    "rss_init_kib":  med("after_first", "rss_kib"),
    "rss_cycle_kib": med("after_last", "rss_kib"),
    "hwm_cycle_kib": med("after_last", "hwm_kib"),
}))'
}

echo "measuring: G1 x2 (rebuild), G2 x2, G3 x2, G4 2 passes of $G4_PROCESSES"

G1_R1=$(printf '{"section_sum": %s, "stripped_size": %s, "payload_bytes": %s}' \
	"$(section_sum "$SHIPPED_SO")" "$(stat -c%s "$SHIPPED_SO")" \
	"$(( $(stat -c%s "$SHIPPED_SO") + $(stat -c%s "$ZHUYIN_SO") ))")
# G1 round 2 rebuilds, because build reproducibility is what could silently
# move a size number; G2/G3 re-run the process over one artifact instead.
SHIPPED_SO2=$(mktemp -u /tmp/perf-gate-shipped2-XXXX.so)
ZHUYIN_SO2=$(mktemp -u /tmp/perf-gate-zhuyin2-XXXX.so)
build_cinstall libpinyin "$BACKEND,shipped" "$SHIPPED_SO2" >/dev/null
build_cinstall libzhuyin "$BACKEND" "$ZHUYIN_SO2" >/dev/null
G1_R2=$(printf '{"section_sum": %s, "stripped_size": %s, "payload_bytes": %s}' \
	"$(section_sum "$SHIPPED_SO2")" "$(stat -c%s "$SHIPPED_SO2")" \
	"$(( $(stat -c%s "$SHIPPED_SO2") + $(stat -c%s "$ZHUYIN_SO2") ))")

G2_R1=$(printf '{"ir_oxpinyin_object": %s}' "$(ir_for "$SHIPPED_SO")")
G2_R2=$(printf '{"ir_oxpinyin_object": %s}' "$(ir_for "$SHIPPED_SO")")
G3_R1=$(alloc_round)
G3_R2=$(alloc_round)
# Two INDEPENDENT passes. Reusing one pass's medians for both rounds would make
# G4's agreement check compare a number with itself and pass unconditionally,
# which is worse than not checking it.
G4_P1=$(rss_pass)
G4_P2=$(rss_pass)

FINGERPRINT=$(python3 -c '
import hashlib, json, os, pathlib, subprocess, sys
def run(*cmd):
    try:
        return subprocess.run(cmd, capture_output=True, text=True).stdout.split("\n")[0].strip()
    except OSError:
        return "unknown"
data = pathlib.Path(sys.argv[1])
h = hashlib.sha256()
for path in sorted(p for p in data.rglob("*") if p.is_file()):
    h.update(path.name.encode()); h.update(path.read_bytes())
print(json.dumps({
    "image_digest":   os.environ.get("PERF_GATE_IMAGE_DIGEST", "unrecorded"),
    "apt_snapshot":   os.environ.get("PERF_GATE_APT_SNAPSHOT", "unrecorded"),
    "runner_image":   f'"'"'{os.environ.get("ImageOS","local")}/{os.environ.get("ImageVersion","local")}'"'"',
    "kernel":         run("uname", "-srvm"),
    "rustc":          run("rustc", "-V"),
    "glibc":          run("ldd", "--version"),
    "libtkrzw":       os.environ.get("PERF_GATE_LIBTKRZW", run("pkg-config", "--modversion", "tkrzw")),
    "valgrind":       run("valgrind", "--version"),
    "readelf":        run("readelf", "--version"),
    "fixture_sha256": h.hexdigest(),
    "harness_commit": run("git", "log", "-1", "--format=%H", "--",
                          "tools/bisection/bisect.c", "tools/perf-gate"),
}))' "$DATA")

emit_json "
import json
doc = {
    'captured_utc': '$(date -u +%Y-%m-%dT%H:%M:%SZ)',
    'fingerprint': json.loads('''$FINGERPRINT'''),
    'artifacts': {
        'shipped_libpinyin': {
            'recipe': '''$SHIPPED_RECIPE''',
            'installed_name': 'libpinyin.so.15.0.0',
            'sha256': '$(sha256sum "$SHIPPED_SO" | cut -d' ' -f1)',
            'needed': '''$(readelf -d "$SHIPPED_SO" |
                sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' |
                python3 -c 'import json,sys; print(json.dumps([l.strip() for l in sys.stdin]))')''',
            'bytes': $(stat -c%s "$SHIPPED_SO"),
        },
        'shipped_libzhuyin': {
            'recipe': '''$ZHUYIN_RECIPE''',
            'installed_name': 'libzhuyin.so.15.0.0',
            'sha256': '$(sha256sum "$ZHUYIN_SO" | cut -d' ' -f1)',
            'needed': [], 'bytes': $(stat -c%s "$ZHUYIN_SO"),
        },
        'alloccount_libpinyin': {
            'recipe': '''$ALLOC_RECIPE''',
            'installed_name': 'libpinyin_capi.so -- a fixture; nothing installs it',
            'sha256': '$(sha256sum "$ALLOC_SO" | cut -d' ' -f1)',
            'needed': [], 'bytes': $(stat -c%s "$ALLOC_SO"),
        },
    },
    'capture_params': {
        'g2': {'cg_cycles': $CG_CYCLES, 'rounds': 2},
        'g3': {'cycles': $G3_CYCLES, 'rounds': 2},
        'g4': {'processes_per_pass': $G4_PROCESSES, 'passes': 2},
        'cpu': $CPU, 'backend': '$BACKEND', 'fixture': 'fixtures/w3/$FIXTURE_EXT',
        'perf_event_open': '$PMU',
    },
    'rounds': {
        'g1': [json.loads('''$G1_R1'''), json.loads('''$G1_R2''')],
        'g2': [json.loads('''$G2_R1'''), json.loads('''$G2_R2''')],
        'g3': [json.loads('''$G3_R1'''), json.loads('''$G3_R2''')],
        'g4': [json.loads('''$G4_P1'''), json.loads('''$G4_P2''')],
    },
}"

rm -f "$SHIPPED_SO" "$SHIPPED_SO2" "$ZHUYIN_SO" "$ZHUYIN_SO2" "$ALLOC_SO" "$BISECT"
echo "wrote $OUT"
