#!/usr/bin/env bash
# run-perf-matrix.sh — 4-cell backend × implementation benchmark matrix.
#
# Measures four configurations through bisect --perf:
#   A. libpinyin  + Tkrzw
#   B. libpinyin  + Kyoto Cabinet
#   C. oxpinyin   + Tkrzw
#   D. oxpinyin   + Kyoto Cabinet
#
# Each cell is measured for speed, RAM, and installed size.
# Speed/RAM runs alternate all four cells in round-robin to control drift.
#
# Environment:
#   MATRIX_OUT   output directory (default /out)
#   PERF_CPU     CPU to pin (default 0)
#   PERF_RUNS    speed processes per cell (default 20)
#   PERF_CYCLES  keystroke cycles per process (default 8)
#   PERF_RAM_RUNS  RAM processes per cell (default 10)

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"

OUT="${MATRIX_OUT:-/out}"
CPU="${PERF_CPU:-0}"
RUNS="${PERF_RUNS:-20}"
CYCLES="${PERF_CYCLES:-8}"
RAM_RUNS="${PERF_RAM_RUNS:-10}"

mkdir -p "$OUT"

# ── Discover paths ───────────────────────────────────────────────────

find_so() {
    local root=$1
    local so
    so=$(find "$root" -name 'libpinyin.so' \( -type f -o -type l \) -print -quit 2>/dev/null)
    [ -n "$so" ] || { echo "fatal: libpinyin.so not found under $root" >&2; exit 1; }
    echo "$so"
}

A_LABEL="libpinyin-tkrzw"
A_SO=$(find_so /opt/libpinyin-tkrzw)
A_DATA="/opt/libpinyin-tkrzw/lib/libpinyin/data"
A_LIB=$(dirname "$A_SO")

B_LABEL="libpinyin-kc"
B_SO=$(find_so /opt/libpinyin-kc)
B_DATA="/opt/libpinyin-kc/lib/libpinyin/data"
B_LIB=$(dirname "$B_SO")

C_LABEL="oxpinyin-tkrzw"
C_SO=$(find_so /opt/oxpinyin-tkrzw/stage)
C_DATA="/opt/oxpinyin-tkrzw/data"
C_LIB=$(dirname "$C_SO")

D_LABEL="oxpinyin-kc"
D_SO=$(find_so /opt/oxpinyin-kc/stage)
D_DATA="/opt/oxpinyin-kc/data"
D_LIB=$(dirname "$D_SO")

LABELS=("$A_LABEL" "$B_LABEL" "$C_LABEL" "$D_LABEL")
SOS=("$A_SO" "$B_SO" "$C_SO" "$D_SO")
DATAS=("$A_DATA" "$B_DATA" "$C_DATA" "$D_DATA")
LIBS=("$A_LIB" "$B_LIB" "$C_LIB" "$D_LIB")

echo "=== Matrix Configuration ==="
for i in 0 1 2 3; do
    echo "  ${LABELS[$i]}:"
    echo "    so:   ${SOS[$i]}"
    echo "    data: ${DATAS[$i]}"
    echo "    lib:  ${LIBS[$i]}"
    [ -f "${SOS[$i]}" ] || { echo "fatal: ${SOS[$i]} not found" >&2; exit 1; }
    [ -d "${DATAS[$i]}" ] || { echo "fatal: ${DATAS[$i]} not found" >&2; exit 1; }
done

# ── Speed ────────────────────────────────────────────────────────────

SPEED_JSONL="$OUT/speed.jsonl"
: > "$SPEED_JSONL"

run_one() {
    local label=$1 so=$2 data=$3 libdir=$4 mode=$5 outfile=$6
    if command -v taskset >/dev/null 2>&1; then
        taskset -c "$CPU" env \
            LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE="$mode" PERF_CYCLES="$CYCLES" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" \
            >>"$outfile" 2>>"$OUT/$label-$mode.err"
    else
        env LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE="$mode" PERF_CYCLES="$CYCLES" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" \
            >>"$outfile" 2>>"$OUT/$label-$mode.err"
    fi
}

echo "--- speed: $RUNS alternating runs × $CYCLES cycles, CPU $CPU ---"
for _ in $(seq 1 "$RUNS"); do
    for i in 0 1 2 3; do
        run_one "${LABELS[$i]}" "${SOS[$i]}" "${DATAS[$i]}" "${LIBS[$i]}" speed "$SPEED_JSONL"
    done
done

# ── RAM ──────────────────────────────────────────────────────────────

RAM_INIT_JSONL="$OUT/ram-init.jsonl"
RAM_CYCLE_JSONL="$OUT/ram-cycle.jsonl"
: > "$RAM_INIT_JSONL"
: > "$RAM_CYCLE_JSONL"

echo "--- RAM: $RAM_RUNS runs per mode per cell, CPU $CPU ---"
for _ in $(seq 1 "$RAM_RUNS"); do
    for i in 0 1 2 3; do
        run_one "${LABELS[$i]}" "${SOS[$i]}" "${DATAS[$i]}" "${LIBS[$i]}" ram-init "$RAM_INIT_JSONL"
        run_one "${LABELS[$i]}" "${SOS[$i]}" "${DATAS[$i]}" "${LIBS[$i]}" ram-cycle "$RAM_CYCLE_JSONL"
    done
done

# ── Size ─────────────────────────────────────────────────────────────

echo "--- installed size ---"

# Size prefixes: libpinyin uses the install prefix; oxpinyin uses the
# staged tree. Both are stripped uniformly by the Dockerfile.
SIZE_PREFIXES=(
    "/opt/libpinyin-tkrzw"
    "/opt/libpinyin-kc"
    "/opt/oxpinyin-tkrzw/stage/usr"
    "/opt/oxpinyin-kc/stage/usr"
)
SIZE_DATA_DIRS=(
    "/opt/libpinyin-tkrzw/lib/libpinyin/data"
    "/opt/libpinyin-kc/lib/libpinyin/data"
    "/opt/oxpinyin-tkrzw/data"
    "/opt/oxpinyin-kc/data"
)

python3 "$SCRIPT_DIR/perf-matrix.py" summarize \
    --speed "$SPEED_JSONL" \
    --ram-init "$RAM_INIT_JSONL" \
    --ram-cycle "$RAM_CYCLE_JSONL" \
    --labels "${LABELS[@]}" \
    --size-prefixes "${SIZE_PREFIXES[@]}" \
    --data-dirs "${SIZE_DATA_DIRS[@]}" \
    > "$OUT/matrix-report.md"

cat "$OUT/matrix-report.md"
echo ""
echo "captures: $OUT/{speed,ram-init,ram-cycle}.jsonl"
echo "report:   $OUT/matrix-report.md"
