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
# Runs inside the perf-matrix container with the tree mounted at /work:
#
#   docker run --rm --platform linux/arm64 -v "$PWD":/work -w /work \
#     -e CARGO_TARGET_DIR=/work/target-linux -v /tmp/perf-out:/out \
#     oxpinyin-matrix:latest tools/bisection/run-perf-same-data.sh
#
# Environment:
#   MATRIX_OUT   output directory (default /out)
#   PERF_CPU     CPU to pin (default 0)
#   PERF_RUNS    speed processes per cell (default 20)
#   PERF_CYCLES  keystroke cycles per process (default 8)
#   PERF_RAM_RUNS  RAM processes per cell (default 10)
#   OXPINYIN_KC_SO / OXPINYIN_TKRZW_SO  prebuilt oxpinyin .so paths
#       (default: cargo build --release from this tree, one per feature)
set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

OUT="${MATRIX_OUT:-/out}"
CPU="${PERF_CPU:-0}"
RUNS="${PERF_RUNS:-20}"
CYCLES="${PERF_CYCLES:-8}"
RAM_RUNS="${PERF_RAM_RUNS:-10}"
TARGET="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
mkdir -p "$OUT"

build_capi() {
    local features=$1 out=$2
    if [ -z "$features" ]; then
        cargo build --locked --release -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml"
    else
        cargo build --locked --release -p oxpinyin-capi --no-default-features --features "$features" \
            --manifest-path "$REPO_ROOT/Cargo.toml"
    fi
    cp "$TARGET/release/libpinyin_capi.so" "$out"
    strip --strip-all "$out"
}

if [ -z "${OXPINYIN_KC_SO:-}" ]; then
    OXPINYIN_KC_SO="$OUT/libpinyin_capi-kc.so"
    echo "--- building oxpinyin capi (kyotocabinet) ---"; build_capi "" "$OXPINYIN_KC_SO"
fi
if [ -z "${OXPINYIN_TKRZW_SO:-}" ]; then
    OXPINYIN_TKRZW_SO="$OUT/libpinyin_capi-tkrzw.so"
    echo "--- building oxpinyin capi (tkrzw) ---"; build_capi tkrzw "$OXPINYIN_TKRZW_SO"
fi

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
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" >>"$outfile" 2>>"$OUT/$label-$mode.err"
    else
        env LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE="$mode" PERF_CYCLES="$CYCLES" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" >>"$outfile" 2>>"$OUT/$label-$mode.err"
    fi
}

SPEED_JSONL="$OUT/speed.jsonl"; : > "$SPEED_JSONL"
echo "--- speed: $RUNS alternating runs × $CYCLES cycles, CPU $CPU ---"
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
            r = json.loads(line); rows.setdefault(r["backend"], []).append(r)
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
