#!/usr/bin/env bash
# run-w8-cycle.sh — profile the W8 candidate cycle against installed oxpinyin-capi.
#
# Continues `tools/bisection/run-perf-baseline.sh`: same 20-input parse +
# guess + count cycle (`bisect --perf`), now sampled with debug info.
#
# Prefers `samply record` (2026). Falls back to `cargo flamegraph` / the
# `flamegraph` wrapper, then to `perf record`. Artifacts land under
# `target/profile/`.
#
# Requirements:
#   - exported tables (`PINYIN_EXPORT_DIR` or /tmp/oxpinyin-export)
#   - pinned model20 cache (`tools/model/fetch-model.sh`)
#   - cargo-c (`cargo cinstall --profile profiling`)
#
# Environment:
#   PINYIN_EXPORT_DIR   redb export (default /tmp/oxpinyin-export)
#   PINYIN_MODEL_DIR    extracted model20 (default target/model20/extracted)
#   PERF_CYCLES         bisect cycles (default 32; drown pinyin_init)
#   PERF_CPU            taskset CPU (optional)
#   PROFILE_REUSE_STAGE=1  reuse a previous cargo-c staging tree

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"
BISECT_SRC="$REPO_ROOT/tools/bisection/bisect.c"

PIN_REF='libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw'
MODEL20_SHA256='59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155'

PROFILE_DIR="${OXPINYIN_PROFILE_DIR:-$REPO_ROOT/target/profile}"
STAGE="$PROFILE_DIR/stage"
CYCLES="${PERF_CYCLES:-32}"
EXPORT_DIR="${PINYIN_EXPORT_DIR:-/tmp/oxpinyin-export}"
MODEL_DIR="${PINYIN_MODEL_DIR:-$REPO_ROOT/target/model20/extracted}"

mkdir -p "$PROFILE_DIR"

echo "oxpinyin Stage-2 W8-cycle profile"
echo "  pin: $PIN_REF"
echo "  model20 SHA-256: $MODEL20_SHA256"
echo "  artifacts: $PROFILE_DIR"
echo "  cycles: $CYCLES"

# ── Inputs ──────────────────────────────────────────────────────────────

for name in pinyin_index.redb phrase_index.redb bigram.redb; do
    if [ ! -f "$EXPORT_DIR/$name" ]; then
        echo "fatal: exported table missing at $EXPORT_DIR/$name" >&2
        echo "  run: cargo run --locked --release -p oxpinyin-migrate --features oracle-ffi -- export --out-dir $EXPORT_DIR" >&2
        exit 1
    fi
done
if [ ! -f "$MODEL_DIR/interpolation2.text" ]; then
    echo "fatal: pinned model missing at $MODEL_DIR/interpolation2.text" >&2
    echo "  run: tools/model/fetch-model.sh" >&2
    exit 1
fi

# ── cargo-c install with the profiling profile ──────────────────────────

if [ -f "$STAGE/usr/lib64/pkgconfig/oxpinyin.pc" ] && \
   [ -f "$STAGE/usr/lib64/libpinyin_capi.so" ] && \
   [ "${PROFILE_REUSE_STAGE:-0}" = "1" ]; then
    echo "--- reusing stage: $STAGE ---"
else
    echo "--- cargo cinstall --profile profiling -> $STAGE ---"
    rm -rf "$STAGE"
    cargo cinstall --locked --profile profiling -p oxpinyin-capi \
        --destdir="$STAGE" --prefix=/usr \
        --manifest-path "$REPO_ROOT/Cargo.toml"
fi

export PKG_CONFIG_PATH="$STAGE/usr/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export PKG_CONFIG_SYSROOT_DIR="$STAGE"
CAPI_LIBDIR=$(pkg-config --variable=libdir oxpinyin)
CAPI_SO="$CAPI_LIBDIR/libpinyin_capi.so"
CAPI_DATA="$STAGE/usr/share/oxpinyin"
[ -f "$CAPI_SO" ] || { echo "fatal: $CAPI_SO not found" >&2; exit 1; }

# cargo-c does not install data files; this is the documented packager step.
mkdir -p "$CAPI_DATA"
cp -L "$EXPORT_DIR/pinyin_index.redb" \
      "$EXPORT_DIR/phrase_index.redb" \
      "$EXPORT_DIR/bigram.redb" "$CAPI_DATA/"
cp -L "$MODEL_DIR/interpolation2.text" "$CAPI_DATA/interpolation2.text"
echo "installed capi .so:  $CAPI_SO"
echo "installed capi data: $CAPI_DATA"

# ── Harness ─────────────────────────────────────────────────────────────

echo "--- building bisect --perf harness ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -g \
    -o "$PROFILE_DIR/bisect" "$BISECT_SRC" -ldl

export LD_LIBRARY_PATH="$CAPI_LIBDIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
export PERF_BACKEND=oxpinyin
export PERF_MODE=speed
export PERF_CYCLES="$CYCLES"

run_pinned() {
    if [ -n "${PERF_CPU:-}" ] && command -v taskset >/dev/null 2>&1; then
        taskset -c "$PERF_CPU" "$@"
    else
        "$@"
    fi
}

CYCLE_CMD=("$PROFILE_DIR/bisect" --perf "$CAPI_SO" "$CAPI_DATA")

# ── Record ──────────────────────────────────────────────────────────────

TIMING_JSON="$PROFILE_DIR/w8-cycle-timing.json"
PROFILE_JSON="$PROFILE_DIR/w8-cycle.profile.json.gz"
FLAMEGRAPH_SVG="$PROFILE_DIR/w8-cycle.svg"
PERF_DATA="$PROFILE_DIR/perf.data"
HEADER="$PROFILE_DIR/header.txt"

{
    echo "pin: $PIN_REF"
    echo "model20_sha256: $MODEL20_SHA256"
    echo "so: $CAPI_SO"
    echo "data: $CAPI_DATA"
    echo "cycles: $CYCLES"
    echo "profile: profiling (release + line-tables-only + thin LTO)"
} >"$HEADER"

recorded=0

if command -v samply >/dev/null 2>&1; then
    echo "--- samply record (preferred) ---"
    if run_pinned samply record --save-only -o "$PROFILE_JSON" -- \
        "${CYCLE_CMD[@]}" | tee "$TIMING_JSON"
    then
        echo "samply profile: $PROFILE_JSON"
        recorded=1
    else
        echo "warning: samply failed (often /proc/sys/kernel/perf_event_paranoid > 1); falling back" >&2
    fi
fi

if [ "$recorded" -eq 0 ] && command -v flamegraph >/dev/null 2>&1; then
    echo "--- flamegraph wrapper (cargo-flamegraph) ---"
    if run_pinned flamegraph -o "$FLAMEGRAPH_SVG" -- \
        "${CYCLE_CMD[@]}" | tee "$TIMING_JSON"
    then
        echo "flamegraph: $FLAMEGRAPH_SVG"
        recorded=1
    else
        echo "warning: flamegraph failed; falling back" >&2
    fi
fi

if [ "$recorded" -eq 0 ] && command -v perf >/dev/null 2>&1; then
    echo "--- perf record ---"
    if run_pinned perf record --call-graph dwarf -F 999 -o "$PERF_DATA" -- \
        "${CYCLE_CMD[@]}" | tee "$TIMING_JSON"
    then
        perf report --stdio -i "$PERF_DATA" --no-children --sort overhead \
            | head -n 80 >"$PROFILE_DIR/perf-report.txt" || true
        if command -v inferno-flamegraph >/dev/null 2>&1; then
            perf script -i "$PERF_DATA" | inferno-flamegraph >"$FLAMEGRAPH_SVG"
            echo "flamegraph: $FLAMEGRAPH_SVG"
        fi
        echo "perf data: $PERF_DATA"
        recorded=1
    else
        echo "warning: perf record failed; falling back" >&2
    fi
fi

if [ "$recorded" -eq 0 ] && command -v valgrind >/dev/null 2>&1; then
    echo "--- valgrind callgrind (no perf_events needed) ---"
    CALLGRIND_OUT="$PROFILE_DIR/callgrind.out"
    run_pinned valgrind --tool=callgrind --error-exitcode=0 \
        --trace-children=no \
        --callgrind-out-file="$CALLGRIND_OUT" \
        "${CYCLE_CMD[@]}" | tee "$TIMING_JSON"
    if command -v callgrind_annotate >/dev/null 2>&1 && [ -s "$CALLGRIND_OUT" ]; then
        callgrind_annotate --auto=no --inclusive=no --threshold=1 \
            "$CALLGRIND_OUT" >"$PROFILE_DIR/callgrind-annotate.txt" || true
        echo "callgrind annotate: $PROFILE_DIR/callgrind-annotate.txt"
    fi
    echo "callgrind: $CALLGRIND_OUT"
    recorded=1
fi

if [ "$recorded" -eq 0 ]; then
    echo "warning: samply, flamegraph, perf, and valgrind all missing; writing timing only" >&2
    run_pinned "${CYCLE_CMD[@]}" | tee "$TIMING_JSON"
fi

echo "timing: $TIMING_JSON"
echo "header: $HEADER"
echo "done."
