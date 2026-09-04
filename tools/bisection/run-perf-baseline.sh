#!/usr/bin/env bash
# run-perf-baseline.sh — W8 Stage-2 scoreboard: oracle vs installed oxpinyin.
#
# Measures three axes with one dlopen harness (`bisect --perf`):
#   1. execution speed, C-ABI to C-ABI, alternating oracle/oxpinyin runs;
#   2. installed size (oracle prefix vs cargo-c staged tree);
#   3. RAM via /proc/self/status in the same harness.
#
# Requirements:
#   - the pin-built oracle from tools/oracle/build-oracle.sh
#     (PINYIN_ORACLE_PREFIX, default $HOME/.local/opt/pinyin-oracle)
#   - cargo-c on PATH (stages oxpinyin-capi through cargo cinstall)
#   - the pinned model20 archive/cache, fetched by tools/model/fetch-model.sh
#
# Environment:
#   PINYIN_ORACLE_PREFIX  oracle prefix
#   OXPINYIN_PERF_WORK    work/stage/capture dir (default target/perf-baseline)
#   PERF_CPU              CPU number to pin every run (default 0)
#   PERF_RUNS             speed processes per backend (default 20)
#   PERF_CYCLES           keystroke cycles per process (default 8)
#   PERF_RAM_RUNS         ram-init/ram-cycle processes per backend (default 10)

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"

ORACLE_PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
WORK="${OXPINYIN_PERF_WORK:-$REPO_ROOT/target/perf-baseline}"
STAGE="$WORK/stage"
CPU="${PERF_CPU:-0}"
RUNS="${PERF_RUNS:-20}"
CYCLES="${PERF_CYCLES:-8}"
RAM_RUNS="${PERF_RAM_RUNS:-10}"

ORACLE_SO="$ORACLE_PREFIX/lib/libpinyin.so"
ORACLE_DATA="$ORACLE_PREFIX/lib/libpinyin/data"
ORACLE_PIN='libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c'

mkdir -p "$WORK"
export PINYIN_ORACLE_PREFIX="$ORACLE_PREFIX"

# ── Oracle pin gate ──────────────────────────────────────────────────────

if [ ! -f "$ORACLE_PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "fatal: pin-built oracle not found at $ORACLE_PREFIX" >&2
    echo "  build it with tools/oracle/build-oracle.sh" >&2
    exit 1
fi
if ! grep -q "^pin_ref=.*$ORACLE_PIN" "$ORACLE_PREFIX/oracle-pin.txt"; then
    echo "fatal: oracle prefix at $ORACLE_PREFIX is off-pin" >&2
    exit 1
fi

# ── Harness build (installed flags come from libpinyin.pc below) ──────────

echo "--- building bisect harness ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$SCRIPT_DIR/bisect" \
    "$SCRIPT_DIR/bisect.c" -ldl
echo "build: ok"

# ── Stage the installed oxpinyin-capi tree with cargo-c ─────────────────

STAGED_PC=$(find "$STAGE/usr" -name libpinyin.pc -print -quit 2>/dev/null || true)
if [ -n "$STAGED_PC" ] && [ "${PERF_REUSE_STAGE:-0}" = "1" ]; then
    echo "--- reusing stage: $STAGE ---"
else
    echo "--- cargo cinstall oxpinyin-capi -> $STAGE ---"
    rm -rf "$STAGE"
    cargo cinstall --locked --release -p oxpinyin-capi \
        --destdir="$STAGE" --prefix=/usr \
        --manifest-path "$REPO_ROOT/Cargo.toml"
    STAGED_PC=$(find "$STAGE/usr" -name libpinyin.pc -print -quit)
fi

export PKG_CONFIG_PATH="$(dirname "$STAGED_PC")${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export PKG_CONFIG_SYSROOT_DIR="$STAGE"
CAPI_CFLAGS=$(pkg-config --cflags libpinyin)
CAPI_LIBS=$(pkg-config --libs libpinyin)
CAPI_LIBDIR=$(pkg-config --variable=libdir libpinyin)
CAPI_SO="$CAPI_LIBDIR/libpinyin.so"
CAPI_DATA="$STAGE/usr/share/oxpinyin"
echo "pkg-config oxpinyin cflags: $CAPI_CFLAGS"
echo "pkg-config oxpinyin libs:   $CAPI_LIBS"
echo "installed capi .so:         $CAPI_SO"
[ -f "$CAPI_SO" ] || { echo "fatal: $CAPI_SO not found" >&2; exit 1; }

# ── Installed data tree (cargo-c ships no data; packager must add it) ────

if [ ! -f "$CAPI_DATA/pinyin_index.bin" ] || [ ! -f "$CAPI_DATA/phrase_index.bin" ] \
   || [ ! -f "$CAPI_DATA/bigram.db" ] || [ ! -f "$CAPI_DATA/table.conf" ]; then
    echo "--- populating oxpinyin data from export + model cache ---"
    EXPORT_DIR="${PINYIN_EXPORT_DIR:-/tmp/oxpinyin-export}"
    missing=""
    for table in pinyin_index.bin phrase_index.bin bigram.db table.conf; do
        [ -f "$EXPORT_DIR/$table" ] || missing="$missing $table"
    done
    [ -z "$missing" ] || {
        echo "fatal: exported file(s)$missing not found at $EXPORT_DIR;" >&2
        echo "  generate with: cargo run --release -p oxpinyin-datagen -- compile --out-dir DIR" >&2
        exit 1
    }
    mkdir -p "$CAPI_DATA"
    cp -L "$EXPORT_DIR"/*.bin "$EXPORT_DIR"/*.db "$EXPORT_DIR"/table.conf \
          "$CAPI_DATA/"
    for f in "$EXPORT_DIR"/*.table; do
        [ -f "$f" ] && cp -L "$f" "$CAPI_DATA/"
    done
    # Verify every exported file arrived.
    for f in "$EXPORT_DIR"/*.bin "$EXPORT_DIR"/*.db "$EXPORT_DIR"/table.conf; do
        [ -f "$f" ] || continue
        base=$(basename "$f")
        [ -f "$CAPI_DATA/$base" ] || {
            echo "fatal: $base failed to copy from $EXPORT_DIR to $CAPI_DATA" >&2
            exit 1
        }
    done
fi
for _required in pinyin_index.bin phrase_index.bin bigram.db table.conf \
                 addon_pinyin_index.bin addon_phrase_index.bin punct.bin \
                 gb_char.bin; do
    [ -f "$CAPI_DATA/$_required" ] || {
        echo "fatal: missing $CAPI_DATA/$_required" >&2
        exit 1
    }
done
echo "installed capi data:        $CAPI_DATA"

# ── Axis 1: speed, alternating to control drift ──────────────────────────

SPEED_JSONL="$WORK/speed.jsonl"
: > "$SPEED_JSONL"
run_speed_side() {
    local label=$1 so=$2 data=$3 libdir=$4
    if command -v taskset >/dev/null 2>&1; then
        taskset -c "$CPU" env \
            LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE=speed PERF_CYCLES="$CYCLES" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" \
            >>"$SPEED_JSONL" 2>>"$WORK/$label-speed.err"
    else
        env LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE=speed PERF_CYCLES="$CYCLES" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" \
            >>"$SPEED_JSONL" 2>>"$WORK/$label-speed.err"
    fi
}

echo "--- speed: $RUNS alternating runs x $CYCLES cycles, CPU $CPU ---"
for _ in $(seq 1 "$RUNS"); do
    run_speed_side oracle "$ORACLE_SO" "$ORACLE_DATA" "$ORACLE_PREFIX/lib"
    run_speed_side oxpinyin "$CAPI_SO" "$CAPI_DATA" "$CAPI_LIBDIR"
done
python3 "$SCRIPT_DIR/perf-baseline.py" summarize-speed "$SPEED_JSONL" \
    > "$WORK/speed-report.md"
cat "$WORK/speed-report.md"
echo "captures: $SPEED_JSONL"

# ── Axis 3: RAM, same harness and same /proc/self/status reads ───────────

RAM_INIT_JSONL="$WORK/ram-init.jsonl"
RAM_CYCLE_JSONL="$WORK/ram-cycle.jsonl"
: > "$RAM_INIT_JSONL"
: > "$RAM_CYCLE_JSONL"
run_ram_side() {
    local label=$1 mode=$2 so=$3 data=$4 libdir=$5
    if command -v taskset >/dev/null 2>&1; then
        taskset -c "$CPU" env \
            LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE="$mode" PERF_CYCLES="$CYCLES" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" \
            >>"$WORK/$mode.jsonl" 2>>"$WORK/$label-$mode.err"
    else
        env LD_LIBRARY_PATH="$libdir" \
            PERF_BACKEND="$label" PERF_MODE="$mode" PERF_CYCLES="$CYCLES" \
            "$SCRIPT_DIR/bisect" --perf "$so" "$data" \
            >>"$WORK/$mode.jsonl" 2>>"$WORK/$label-$mode.err"
    fi
}

echo "--- RAM: $RAM_RUNS runs per mode per backend, CPU $CPU ---"
for _ in $(seq 1 "$RAM_RUNS"); do
    run_ram_side oracle ram-init "$ORACLE_SO" "$ORACLE_DATA" "$ORACLE_PREFIX/lib"
    run_ram_side oxpinyin ram-init "$CAPI_SO" "$CAPI_DATA" "$CAPI_LIBDIR"
    run_ram_side oracle ram-cycle "$ORACLE_SO" "$ORACLE_DATA" "$ORACLE_PREFIX/lib"
    run_ram_side oxpinyin ram-cycle "$CAPI_SO" "$CAPI_DATA" "$CAPI_LIBDIR"
done
python3 "$SCRIPT_DIR/perf-baseline.py" summarize-ram "$RAM_INIT_JSONL" "$RAM_CYCLE_JSONL" \
    > "$WORK/ram-report.md"
cat "$WORK/ram-report.md"
echo "captures: $RAM_INIT_JSONL $RAM_CYCLE_JSONL"

# ── Axis 2: installed size ───────────────────────────────────────────────

python3 "$SCRIPT_DIR/perf-baseline.py" size "$ORACLE_PREFIX" "$STAGE/usr" \
    > "$WORK/size-report.md"
cat "$WORK/size-report.md"
echo "reports: $WORK/{speed,ram,size}-report.md"
