#!/usr/bin/env bash
# Instruction-level differential for the steady keystroke cycle.
#
# Answers one question: does oxpinyin execute MORE INSTRUCTIONS than
# libpinyin on the keystroke path, or the SAME INSTRUCTIONS MORE SLOWLY?
#
# Runs inside the image built by Dockerfile.callgrind:
#   docker build -f tools/bisection/Dockerfile.callgrind -t oxpinyin-callgrind .
#   docker run --rm --security-opt label=disable \
#               --security-opt seccomp=unconfined \
#               -v /tmp/cg-out:/out:Z oxpinyin-callgrind
#
# The two --security-opt flags exist only so stage 5's perf_event_open can
# reach the PMU: a default rootless container is denied by its SELinux
# context and seccomp profile, not by the host (perf_event_paranoid=2
# permits user-space-only counters). They restore, inside this one
# container, exactly the credential posture the invoking user already
# has on the host, and grant nothing beyond it. The :Z relabels the
# output volume for the same SELinux.
#
# On a host with no PMU at all — Apple-silicon containers, some microVMs —
# stage 5 reports NO HARDWARE COUNTERS and the capture continues; the
# distortion then stays unquantified and is recorded as such. Drop the two
# --security-opt flags there, or keep them, harmlessly.
#
# One harness binary is run under callgrind against each `.so` in turn.
# Because bisect.c drives both engines through the same C ABI, the totals
# are directly comparable; because the collection anchors live in the
# harness rather than in either library, the toggled region is identical.
#
# Stages, in order:
#   0. environment table
#   1. collection anchors are two distinct symbols
#   2. debug-info neutrality: is the profiled artifact the same code
#   3. valgrind's malloc interception: is it in effect
#   4. callgrind Ir/D1/LL/branch differential, both cells, twice each
#   5. hardware instruction counters (perf_event_open, instructions:u)
#      bracketing the same steady anchor region, native runs: the
#      hardware count includes the real glibc allocator while callgrind's
#      Ir includes valgrind's replacement allocator, so the difference
#      between the two engine ratios measures the interception distortion
#      that stage 3 declares
#   6. allocations and bytes per cycle from the gated counting allocator
#   7. wall-clock timing, so the Ir ratio and the timing ratio come from
#      one host in one session
#
# Nothing here interprets the numbers. The decision rule lives in the
# accompanying findings document.

set -euo pipefail

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
OUT=${CG_OUT:-/out}
CPU=${PERF_CPU:-0}

# Stage 4/5 capture size. Callgrind is deterministic, so this is chosen
# for a workable runtime under a ~50x interpreter, not for noise.
CG_CYCLES=${CG_CYCLES:-4}
CG_REPEATS=${CG_REPEATS:-1}
CG_ROUNDS=${CG_ROUNDS:-2}

# Stage 7 uses the cross-host record's own protocol, deliberately NOT the
# callgrind pass's cycle count: the timing ratio has to be comparable to
# the protocol every previous steady-cycle record was taken under.
T_CYCLES=${T_CYCLES:-8}
T_RUNS=${T_RUNS:-20}
T_REPEATS=${T_REPEATS:-1}

# Stage 5 runs native processes with the hardware instruction counter
# bracketing the steady cycles. One number per process; the median across
# runs is the figure, matching how the timing pass aggregates.
HW_RUNS=${HW_RUNS:-10}

# Stage 2's comparison lives in a shared library so the standalone probe
# (tools/env-probe/probe-debuginfo-neutrality.sh) cannot drift from it.
SCRIPT_LIB=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
# shellcheck source=debuginfo-neutrality-lib.sh
source "$SCRIPT_LIB/debuginfo-neutrality-lib.sh"

mkdir -p "$OUT"

LP_SO=$(find /opt/libpinyin-tkrzw -name 'libpinyin.so' \( -type f -o -type l \) -print -quit)
LP_LIB=$(dirname "$LP_SO")
DATA=/opt/libpinyin-tkrzw/lib/libpinyin/data

find_ox() {
    # The capi crate installs as a drop-in libpinyin.so.15 replacement
    # (crate [lib] name `pinyin`), not libpinyin_capi.so.
    find "$1" -name 'libpinyin.so' \( -type f -o -type l \) -print -quit
}
OX_SO=$(find_ox /opt/oxpinyin-tkrzw/stage)
OX_NODEBUG_SO=$(find_ox /opt/oxpinyin-nodebug/stage)
OX_ALLOC_SO=$(find_ox /opt/oxpinyin-alloccount/stage)

for f in "$LP_SO" "$OX_SO" "$OX_NODEBUG_SO" "$OX_ALLOC_SO"; do
    [ -n "$f" ] && [ -e "$f" ] || { echo "fatal: missing artifact: '$f'" >&2; exit 1; }
done
[ -d "$DATA" ] || { echo "fatal: data dir $DATA missing" >&2; exit 1; }

# Both cells open the SAME libpinyin-installed data directory: this is the
# drop-in configuration, with no oxpinyin-generated data anywhere in it.
run_cell() { # label so extra-valgrind-args...
    local label=$1 so=$2; shift 2
    taskset -c "$CPU" env LD_LIBRARY_PATH="$LP_LIB" \
        PERF_BACKEND="$label" PERF_MODE=speed \
        PERF_CYCLES="$CG_CYCLES" PERF_REPEATS="$CG_REPEATS" \
        valgrind --tool=callgrind \
                 --collect-atstart=no \
                 --toggle-collect=bisect_perf_steady_unit \
                 --cache-sim=yes \
                 --branch-sim=yes \
                 --separate-threads=no \
                 "$@" \
        "$SCRIPT_DIR/bisect" --perf "$so" "$DATA"
}

# ── 0. environment ──────────────────────────────────────────────────
{
    echo "=== environment ==="
    echo "captured_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "container_arch: $(uname -m)"
    echo "kernel: $(uname -srvm)"
    echo "nproc: $(nproc)"
    echo "cpu: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')"
    echo "memtotal: $(grep -m1 MemTotal /proc/meminfo)"
    echo "gcc: $(gcc --version | head -1)"
    echo "rustc: $(rustc --version)"
    echo "cargo: $(cargo --version)"
    echo "valgrind: $(valgrind --version)"
    echo "perf_event_paranoid: $(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo unknown)"
    echo "libpinyin_pkgversion: $(grep -h '^Version:' /opt/libpinyin-tkrzw/lib/pkgconfig/libpinyin.pc 2>/dev/null || echo unknown)"
    echo "libtkrzw: $(pkg-config --modversion tkrzw 2>/dev/null || echo unknown)"
    for f in "$LP_SO" "$OX_SO" "$OX_NODEBUG_SO" "$OX_ALLOC_SO"; do
        echo "artifact: $(sha256sum "$f") $(stat -c%s "$f") bytes"
    done
    echo "data_dir: $DATA"
} | tee "$OUT/environment.txt"

# ── 1. collection anchors ───────────────────────────────────────────
{
    echo "=== collection anchors ==="
    nm "$SCRIPT_DIR/bisect" | grep -E 'bisect_perf_(cold|steady)_unit'
    cold=$(nm "$SCRIPT_DIR/bisect" | awk '$3=="bisect_perf_cold_unit"{print $1}')
    steady=$(nm "$SCRIPT_DIR/bisect" | awk '$3=="bisect_perf_steady_unit"{print $1}')
    if [ "$cold" = "$steady" ]; then
        echo "FAIL: anchors folded to one address; the toggle would collect the cold cycle"
        exit 1
    fi
    echo "PASS: distinct addresses, so --toggle-collect=bisect_perf_steady_unit"
    echo "      excludes process init and the cold cycle"
} | tee "$OUT/anchors.txt"

# ── 2. debug-info neutrality ────────────────────────────────────────
# The profiled artifact carries debug info and the timed one does not, so
# the two must be the same code. The comparison itself — full normalized
# instruction text for the multiset, hash-stripped symbol identity plus
# instruction-body pairing for the relocation count — lives in
# debuginfo-neutrality-lib.sh, shared with the standalone probe. Three
# outcomes, and only the last is fatal:
#
#   a. identical .text bytes                  — fully neutral
#   b. same instruction multiset at different addresses
#      — the profile's Ir is unaffected (the same instructions execute);
#        the SIMULATED cache and branch figures are affected, because
#        layout is exactly what they model. Report both facts.
#   c. a different instruction multiset       — different code. Stop.
#
# Case (b) is the expected one and is not a defect in -Cdebuginfo. The
# profile's debug level feeds cargo's crate metadata hash, which feeds
# every mangled symbol name, which feeds link order.
{
    echo "=== debug-info neutrality ==="
    compare_debuginfo_neutrality "$OX_SO" "$OX_NODEBUG_SO"
} | tee "$OUT/debuginfo-neutrality.txt"

# ── 3. valgrind's malloc interception ───────────────────────────────
# vg_replace_malloc.c is linked into every valgrind tool, so client
# malloc/free are redirected and the Ir attributed to allocation is
# valgrind's replacement allocator, not glibc's. Establish whether the
# redirection is actually in effect here before any Ir number is read.
{
    echo "=== valgrind malloc interception ==="
    taskset -c "$CPU" env LD_LIBRARY_PATH="$LP_LIB" \
        PERF_BACKEND=probe PERF_MODE=speed PERF_CYCLES=2 PERF_REPEATS=1 \
        valgrind --tool=callgrind --collect-atstart=no \
                 --toggle-collect=bisect_perf_steady_unit \
                 --trace-redir=yes \
                 --callgrind-out-file="$OUT/callgrind.redir-probe.out" \
        "$SCRIPT_DIR/bisect" --perf "$OX_SO" "$DATA" \
        > "$OUT/redir-probe.stdout" 2> "$OUT/redir-probe.stderr" || true
    echo "--- redirections naming malloc/free (from --trace-redir) ---"
    grep -E 'REDIR|vgpreload|malloc|free' "$OUT/redir-probe.stderr" | head -40 || true
    echo "--- objects credited in the profile ---"
    grep -E '^(ob|fl|fn)=' "$OUT/callgrind.redir-probe.out" \
        | grep -iE 'vgpreload|malloc' | sort -u | head -20 || true
    echo
    echo "Reading: a malloc entry credited to a vgpreload object is proof the"
    echo "replacement is active, so Ir attributed to allocation understates the"
    echo "real glibc cost. Call counts are unaffected."
} | tee "$OUT/malloc-interception.txt"

# ── 4. the differential ─────────────────────────────────────────────
# Both cells, two rounds each, allocator intercepted as in every valgrind
# run. Callgrind is deterministic; the second round exists to demonstrate
# that (a >0.1% divergence between rounds is reported, not averaged).
for round in $(seq 1 "$CG_ROUNDS"); do
    for cell in "libpinyin-tkrzw:$LP_SO" "oxpinyin-tkrzw:$OX_SO"; do
        label=${cell%%:*}; so=${cell#*:}
        echo "--- callgrind: $label round $round ---"
        run_cell "$label" "$so" \
            --callgrind-out-file="$OUT/callgrind.$label.r$round.out" \
            > "$OUT/cg.$label.r$round.stdout" 2> "$OUT/cg.$label.r$round.stderr"
    done
done

for f in "$OUT"/callgrind*.out; do
    [ -e "$f" ] || continue
    case "$f" in *redir-probe*) continue;; esac
    callgrind_annotate --threshold=99.5 "$f" > "$f.annotated.txt" 2>&1 || true
    callgrind_annotate --threshold=99.5 --inclusive=yes "$f" > "$f.inclusive.txt" 2>&1 || true
done

# ── 5. hardware instruction counters: the interception distortion ────
# Callgrind's Ir is collected with vg_replace_malloc active (stage 3), so
# whatever share of the cycle is allocation cost is counted against
# valgrind's simplified allocator, not glibc's — an understatement whose
# size differs per engine and therefore biases the Ir ratio. This stage
# measures that bias instead of declaring it: bisect.c brackets the same
# steady cycles (same anchors, same boundary as the callgrind toggle and
# the alloc counters) with PERF_EVENT_IOC_ENABLE/DISABLE around a
# PERF_COUNT_HW_INSTRUCTIONS counter, exclude_kernel=1, in native
# processes — real glibc malloc included. Medians per cell; the ratio
# between the two engines' medians is the hardware counterpart of I. The
# difference between the two ratios is the measured interception
# distortion; without this number a "stalls" verdict may not be reached.
#
# Requires a container whose SELinux label and seccomp profile do not
# deny perf_event_open (see the run command in the header).
HW="$OUT/hw-instructions.jsonl"; : > "$HW"
for _ in $(seq 1 "$HW_RUNS"); do
    for cell in "libpinyin-tkrzw:$LP_SO" "oxpinyin-tkrzw:$OX_SO"; do
        label=${cell%%:*}; so=${cell#*:}
        taskset -c "$CPU" env LD_LIBRARY_PATH="$LP_LIB" \
            PERF_BACKEND="$label" PERF_MODE=speed \
            PERF_CYCLES="$T_CYCLES" PERF_REPEATS="$T_REPEATS" \
            PERF_HW=instructions \
            "$SCRIPT_DIR/bisect" --perf "$so" "$DATA" >> "$HW" 2>>"$OUT/hw.err"
    done
done
python3 - "$HW" <<'PY' | tee "$OUT/hw-instructions.md"
import json, statistics, sys

runs = {}
hw_errno = None
with open(sys.argv[1], encoding="utf-8") as handle:
    for line in handle:
        line = line.strip()
        if not line:
            continue
        row = json.loads(line)
        if "steady_hw_instructions" not in row:
            hw_errno = row.get("hw_errno", "absent")
            continue
        runs.setdefault(row.get("backend", "?"), []).append(
            (row["steady_hw_instructions"], row.get("steady_hw_cycles", 0)))
if hw_errno is not None:
    print(f"NO HARDWARE COUNTERS: a run reported none (hw_errno={hw_errno}).")
    print("On a host with no PMU at all (Apple-silicon containers, some")
    print("microVMs) this is expected and permanent, and the Ir differential")
    print("above still stands on its own. On a Linux host that does have a")
    print("PMU it means the container denied perf_event_open: rerun with")
    print("  --security-opt label=disable --security-opt seccomp=unconfined")
    print("Distortion remains UNQUANTIFIED either way, so no stall verdict")
    print("may rest on callgrind Ir alone. Continuing with the remaining")
    print("stages rather than aborting the capture.")
    sys.exit(0)
med = {}
for backend, values in sorted(runs.items()):
    counts = [v[0] for v in values]
    cycles = {v[1] for v in values}
    med[backend] = statistics.median(counts)
    per_cycle = med[backend] / max(cycles or {1})
    print(f"{backend}: n={len(counts)} steady_cycles={sorted(cycles)} "
          f"median={med[backend]:.0f} per-cycle={per_cycle:,.0f}")
labels = sorted(med)
if len(labels) == 2:
    ratio = med[labels[1]] / med[labels[0]]
    print(f"hardware instruction ratio ({labels[1]} / {labels[0]}): {ratio:.4f}")
    print("Compare against the callgrind Ir ratio over the same region;")
    print("the difference between the two ratios is the measured")
    print("malloc-interception distortion.")
PY

# ── 6. allocations per cycle ────────────────────────────────────────
# Native, not under valgrind: the counter reports what the client asked
# for, which is exactly what the intercepted profile cannot.
{
    echo "=== allocations per steady cycle (oxpinyin, alloc-count build) ==="
    taskset -c "$CPU" env LD_LIBRARY_PATH="$LP_LIB" \
        PERF_BACKEND=oxpinyin-alloccount PERF_MODE=speed \
        PERF_CYCLES="$T_CYCLES" PERF_REPEATS="$T_REPEATS" \
        "$SCRIPT_DIR/bisect" --perf "$OX_ALLOC_SO" "$DATA"
    echo
    echo "steady_alloc_count / steady_alloc_cycles = allocating calls per cycle."
    echo "libpinyin has no counterpart symbol; its comparable figure is the"
    echo "malloc/operator-new CALL COUNT in its stage 4 profile, which is"
    echo "produced symmetrically for both cells by one instrument."
} | tee "$OUT/alloc-count.txt"

# ── 7. wall-clock timing, same host, same session ───────────────────
# The record's protocol, so this ratio is comparable to every previous
# steady-cycle capture: PERF_CYCLES=8, 20 runs per cell, cells
# interleaved round-robin, taskset pinned.
SPEED="$OUT/speed.jsonl"; : > "$SPEED"
echo "--- timing: $T_RUNS alternating runs x $T_CYCLES cycles x $T_REPEATS passes ---"
for _ in $(seq 1 "$T_RUNS"); do
    for cell in "libpinyin-tkrzw:$LP_SO" "oxpinyin-tkrzw:$OX_SO"; do
        label=${cell%%:*}; so=${cell#*:}
        taskset -c "$CPU" env LD_LIBRARY_PATH="$LP_LIB" \
            PERF_BACKEND="$label" PERF_MODE=speed \
            PERF_CYCLES="$T_CYCLES" PERF_REPEATS="$T_REPEATS" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$DATA" >> "$SPEED" 2>>"$OUT/speed.err"
    done
done
python3 "$SCRIPT_DIR/perf-ci.py" "$SPEED" --metric steady --metric cold --format md \
    | tee "$OUT/timing.md"

echo
echo "=== captures written to $OUT ==="
ls -1 "$OUT"
