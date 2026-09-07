#!/usr/bin/env bash
# run-perf-same-data.sh — the #260 four-cell benchmark on ONE data
# directory per backend.
#
# Cells A/B are the pin-built libpinyin (Tkrzw / Kyoto Cabinet) on their
# own install's data/. Cells C/D are oxpinyin's C ABI built from this
# tree — opened on THE SAME two directories. No oxpinyin-generated data is
# involved: this measures the drop-in configuration, the way a
# distribution would ship it.
#
# Runs inside the perf-matrix container with the tree mounted at /work,
# on the measurement host itself — no --platform override: an emulated
# run times the interpreter, not the implementations.
#
#   docker run --rm -v "$PWD":/work -w /work \
#     -e CARGO_TARGET_DIR=/work/target-linux -v /tmp/perf-out:/out \
#     oxpinyin-matrix:latest tools/bisection/run-perf-same-data.sh
#
# Environment:
#   MATRIX_OUT   output directory (default /out)
#   PERF_CPU     CPU to pin (default 0)
#   PERF_RUNS    speed processes per cell (default 20)
#   PERF_CYCLES  keystroke cycles per process (default 8)
#   PERF_REPEATS corpus passes per timed cycle (default 1); scales the
#       workload inside the timed region with the corpus frozen
#   PERF_RAM_RUNS  RAM processes per cell (default 10)
#   OXPINYIN_KC_SO / OXPINYIN_TKRZW_SO  prebuilt oxpinyin .so paths
#       (default: cargo build --release from this tree, one per feature).
#       Either way the artifact's backend linkage is verified before any
#       cell is measured -- see verify_backend.
set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

OUT="${MATRIX_OUT:-/out}"
CPU="${PERF_CPU:-0}"
RUNS="${PERF_RUNS:-20}"
CYCLES="${PERF_CYCLES:-8}"
REPEATS="${PERF_REPEATS:-1}"
RAM_RUNS="${PERF_RAM_RUNS:-10}"
TARGET="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
mkdir -p "$OUT"

# Assert an artifact links the backend it is labelled with, and only that
# backend. The backend is never inherited from workspace defaults: the
# default flipped from kyotocabinet to tkrzw at 05688575, and the KC cell's
# implicit-default build silently produced a second tkrzw artifact whose
# processes failed pinyin_init on KC-format data (aborting the whole matrix
# under `set -e`).
#
# This runs on the resolved paths, not inside the build: the OXPINYIN_*_SO
# variables below skip the build entirely, and that is precisely the mode
# in which a mislabelled artifact arrives -- it was built somewhere else,
# by something this script cannot see. When the guard lived inside
# build_capi, every prebuilt-.so run was unverified.
#
# The absence check is the other half. oxpinyin-store has a compile_error!
# for two backends at once, so no legitimate artifact links both; an
# artifact that does was not produced by this tree's build and its storage
# format is unknown.
verify_backend() {
    local backend=$1 so=$2 needed want other
    [ -f "$so" ] || { echo "fatal: $so not found" >&2; exit 1; }
    case "$backend" in
        kyotocabinet) want='^libkyotocabinet\.so'; other='^libtkrzw\.so' ;;
        tkrzw)        want='^libtkrzw\.so';        other='^libkyotocabinet\.so' ;;
        *) echo "fatal: verify_backend: unknown backend '$backend'" >&2; exit 1 ;;
    esac
    needed=$(readelf -d "$so" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p')
    if ! printf '%s\n' "$needed" | grep -q "$want"; then
        echo "fatal: $so does not link a $backend library (NEEDED: $(echo $needed))" >&2
        exit 1
    fi
    if printf '%s\n' "$needed" | grep -q "$other"; then
        echo "fatal: $so links a second backend's library (NEEDED: $(echo $needed))" >&2
        exit 1
    fi
}

# Build the capi .so for exactly one named backend.
build_capi() {
    local backend=$1 out=$2
    [ -n "$backend" ] || { echo "fatal: build_capi requires an explicit backend" >&2; exit 1; }
    case "$backend" in
        kyotocabinet|tkrzw) ;;
        *) echo "fatal: build_capi: unknown backend '$backend'" >&2; exit 1 ;;
    esac
    cargo build --locked --release -p oxpinyin-capi --no-default-features --features "$backend" \
        --manifest-path "$REPO_ROOT/Cargo.toml"
    cp "$TARGET/release/libpinyin_capi.so" "$out"
    strip --strip-all "$out"
}

if [ -z "${OXPINYIN_KC_SO:-}" ]; then
    OXPINYIN_KC_SO="$OUT/libpinyin_capi-kc.so"
    echo "--- building oxpinyin capi (kyotocabinet) ---"; build_capi kyotocabinet "$OXPINYIN_KC_SO"
fi
if [ -z "${OXPINYIN_TKRZW_SO:-}" ]; then
    OXPINYIN_TKRZW_SO="$OUT/libpinyin_capi-tkrzw.so"
    echo "--- building oxpinyin capi (tkrzw) ---"; build_capi tkrzw "$OXPINYIN_TKRZW_SO"
fi

# Unconditional: built here or supplied prebuilt, both are checked.
echo "--- verifying capi backend linkage ---"
verify_backend kyotocabinet "$OXPINYIN_KC_SO"
verify_backend tkrzw "$OXPINYIN_TKRZW_SO"

echo "--- building bisect harness ---"
gcc -std=gnu11 -Wall -Wextra -O2 -o "$SCRIPT_DIR/bisect" "$SCRIPT_DIR/bisect.c" -ldl

find_so() {
    local root=$1 so
    so=$(find "$root" -name 'libpinyin.so' \( -type f -o -type l \) -print -quit 2>/dev/null)
    [ -n "$so" ] || { echo "fatal: libpinyin.so not found under $root" >&2; exit 1; }
    echo "$so"
}
A_SO=$(find_so /opt/libpinyin-tkrzw); A_DATA=/opt/libpinyin-tkrzw/lib/libpinyin/data
B_SO=$(find_so /opt/libpinyin-kc);    B_DATA=/opt/libpinyin-kc/lib/libpinyin/data

LABELS=(libpinyin-tkrzw libpinyin-kc oxpinyin-tkrzw oxpinyin-kc)
SOS=("$A_SO" "$B_SO" "$OXPINYIN_TKRZW_SO" "$OXPINYIN_KC_SO")
DATAS=("$A_DATA" "$B_DATA" "$A_DATA" "$B_DATA")
LIBS=("$(dirname "$A_SO")" "$(dirname "$B_SO")" "$(dirname "$A_SO")" "$(dirname "$B_SO")")

echo "=== Same-data matrix configuration ==="
for i in 0 1 2 3; do
    echo "  ${LABELS[$i]}: so=${SOS[$i]} data=${DATAS[$i]}"
    [ -f "${SOS[$i]}" ] || { echo "fatal: ${SOS[$i]} not found" >&2; exit 1; }
done

run_one() {
    local label=$1 so=$2 data=$3 libdir=$4 mode=$5 outfile=$6
    if command -v taskset >/dev/null 2>&1; then
        taskset -c "$CPU" env LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE="$mode" PERF_CYCLES="$CYCLES" \
            PERF_REPEATS="$REPEATS" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" >>"$outfile" 2>>"$OUT/$label-$mode.err"
    else
        env LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE="$mode" PERF_CYCLES="$CYCLES" \
            PERF_REPEATS="$REPEATS" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" >>"$outfile" 2>>"$OUT/$label-$mode.err"
    fi
}

SPEED_JSONL="$OUT/speed.jsonl"; : > "$SPEED_JSONL"
echo "--- speed: $RUNS alternating runs × $CYCLES cycles × $REPEATS passes, CPU $CPU ---"
for _ in $(seq 1 "$RUNS"); do
    for i in 0 1 2 3; do run_one "${LABELS[$i]}" "${SOS[$i]}" "${DATAS[$i]}" "${LIBS[$i]}" speed "$SPEED_JSONL"; done
done

RAM_INIT_JSONL="$OUT/ram-init.jsonl"; RAM_CYCLE_JSONL="$OUT/ram-cycle.jsonl"
: > "$RAM_INIT_JSONL"; : > "$RAM_CYCLE_JSONL"
echo "--- RAM: $RAM_RUNS runs per mode per cell, CPU $CPU ---"
for _ in $(seq 1 "$RAM_RUNS"); do
    for i in 0 1 2 3; do
        run_one "${LABELS[$i]}" "${SOS[$i]}" "${DATAS[$i]}" "${LIBS[$i]}" ram-init "$RAM_INIT_JSONL"
        run_one "${LABELS[$i]}" "${SOS[$i]}" "${DATAS[$i]}" "${LIBS[$i]}" ram-cycle "$RAM_CYCLE_JSONL"
    done
done

echo "--- summary (medians) ---"
python3 - "$SPEED_JSONL" "$RAM_INIT_JSONL" "$RAM_CYCLE_JSONL" <<'PY'
import json, statistics, sys
def load(p):
    rows = {}
    for line in open(p):
        line = line.strip()
        if line:
            r = json.loads(line)
            if "rss_kib" not in r:
                # bisect nests the counters: ram-init rows carry them in
                # after_init; ram-cycle rows peak in after_last (HWM is
                # monotonic, so the last snapshot holds the process peak).
                snap = r.get("after_init" if r.get("mode") == "ram-init" else "after_last", {})
                r.update({k: snap[k] for k in ("rss_kib", "hwm_kib") if k in snap})
            rows.setdefault(r["backend"], []).append(r)
    return rows
speed, ram_init, ram_cycle = (load(p) for p in sys.argv[1:4])
def med(xs): return statistics.median(xs) if xs else float("nan")
def field(rows, k): return med([r[k] for r in rows if k in r])
print(f"{'cell':<18}{'init ms':>10}{'alloc ms':>10}{'cold ms':>10}{'steady ms':>11}{'rss-init KiB':>14}{'hwm-init KiB':>14}{'rss-cycle KiB':>15}{'hwm-cycle KiB':>15}")
for label in speed:
    s = speed[label]
    init = med([r["init_ns"] for r in s]) / 1e6
    alloc = med([r["alloc_ns"] for r in s]) / 1e6
    cold = med([r["cycles_ns"][0] for r in s]) / 1e6
    steady = med([c for r in s for c in r["cycles_ns"][1:]]) / 1e6
    ri = ram_init.get(label, []); rc = ram_cycle.get(label, [])
    print(f"{label:<18}{init:>10.3f}{alloc:>10.3f}{cold:>10.3f}{steady:>11.3f}{field(ri,'rss_kib'):>14.0f}{field(ri,'hwm_kib'):>14.0f}{field(rc,'rss_kib'):>15.0f}{field(rc,'hwm_kib'):>15.0f}")
PY
