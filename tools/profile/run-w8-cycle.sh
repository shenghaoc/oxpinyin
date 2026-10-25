#!/usr/bin/env bash
# run-w8-cycle.sh — profile the W8 candidate cycle against installed oxpinyin-capi.
#
# Continues `tools/bisection/run-perf-baseline.sh`: same 20-input parse +
# guess + count cycle (`bisect --perf`), now sampled with debug info.
#
# Linux prefers `samply record` (2026) and falls back to `cargo flamegraph` /
# the `flamegraph` wrapper, then `perf record`, then Valgrind Callgrind. On
# macOS samply is required: taskset/PERF_CPU, perf and its
# perf_event_paranoid hint are Linux-only, and the Linux fallbacks are not
# attempted. Artifacts land under `target/profile/`.
#
# When samply supports `--unstable-presymbolicate`, the record also writes
# `<profile>.syms.json` and extract-hot-stacks.py reports function names.
# A samply without that flag still records; the report shows unresolved
# frames instead.
#
# After a samply record the run prints the profile's sample total and warns
# when it is below the level extract-hot-stacks.py defines (MIN_SAMPLES): with
# fewer samples the order of the hot stacks is mostly sampling noise. The
# other profilers write no samply profile, print no total and keep their
# shorter default run.
#
# Requirements:
#   - exported tables (`PINYIN_EXPORT_DIR` or /tmp/oxpinyin-export)
#   - pinned model20 cache (`tools/model/fetch-model.sh`)
#   - cargo-c (`cargo cinstall --profile profiling`)
#   - macOS: samply and `OXPINYIN_TKRZW_PREFIX` or `OXPINYIN_TKRZW_LIBRARY`
#     so tools/tkrzw/runtime.sh can verify the staged dylib's libtkrzw
#
# Environment:
#   PINYIN_EXPORT_DIR   tkrzw export (default /tmp/oxpinyin-export);
#                       produce it with `--features tkrzw` (the oracle
#                       this cycle compares against is tkrzw)
#   PINYIN_MODEL_DIR    extracted model20 (default target/model20/extracted)
#   PERF_CYCLES         bisect cycles for every profiler. Unset, samply runs
#                       2048, which gave about three times MIN_SAMPLES samples
#                       on the measuring host (a faster host takes fewer), and
#                       the other profilers run 32, as before
#   PERF_CPU            taskset CPU (Linux only; refused on macOS)
#   PROFILE_REUSE_STAGE=1  reuse a previous cargo-c staging tree
#   OXPINYIN_PROFILE_OS test override for the host name; native runs use uname
#   OXPINYIN_TKRZW_PREFIX  Homebrew prefix for libtkrzw on macOS

# shellcheck source=../lib/bash-floor.sh
. "${0%/*}/../lib/bash-floor.sh"

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"
HOST_OS=${OXPINYIN_PROFILE_OS:-$(uname -s)}
CHECK_ONLY=0
[ "${1:-}" = --check ] && CHECK_ONLY=1
source "$REPO_ROOT/tools/tkrzw/runtime.sh"
BISECT_SRC="$REPO_ROOT/tools/bisection/bisect.c"

# The flag is unstable and absent from older samply releases; probe the
# installed help text instead of assuming it.
samply_has_presymbolicate() {
    local help
    command -v samply >/dev/null 2>&1 || return 1
    help=$(samply record --help 2>&1 || true)
    case "$help" in
        *--unstable-presymbolicate*) return 0 ;;
    esac
    return 1
}

# The W8 cycle profiles against the tkrzw cell (its exports and records
# are tkrzw). build-oracle.sh defaults to Berkeley DB since 2026-09-27, so
# build this prefix with `build-oracle.sh --dbm tkrzw`.
PIN_REF='libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw'
MODEL20_SHA256='59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155'
EXPECTED_MODEL_FILES=(
    art.table culture.table economy.table gb_char.table gbk_char.table
    geology.table history.table interpolation2.text life.table merged.table
    nature.table opengram.table people.table punct.table science.table
    society.table sport.table technology.table
)

PROFILE_DIR="${OXPINYIN_PROFILE_DIR:-$REPO_ROOT/target/profile}"
STAGE="$PROFILE_DIR/stage"
# A sampling profile ranks stacks by sample count, and samply takes about one
# sample per CPU millisecond, so its default run is long enough to reach the
# level extract-hot-stacks.py defines (MIN_SAMPLES). The other profilers keep
# the shorter default: Callgrind counts instructions exactly, and perf record
# keeps a stack dump for every sample.
CYCLES="${PERF_CYCLES:-32}"
SAMPLY_CYCLES="${PERF_CYCLES:-2048}"
if [ "$HOST_OS" = Darwin ] || [ "$SAMPLY_CYCLES" = "$CYCLES" ]; then
    CYCLES_NOTE="$SAMPLY_CYCLES"
else
    CYCLES_NOTE="$SAMPLY_CYCLES (samply), $CYCLES (other profilers)"
fi
EXPORT_DIR="${PINYIN_EXPORT_DIR:-/tmp/oxpinyin-export}"
if [ -n "${PINYIN_MODEL_DIR:-}" ]; then
    MODEL_DIR="$PINYIN_MODEL_DIR"
elif [ -n "${PINYIN_MODEL_CACHE:-}" ]; then
    MODEL_DIR="$PINYIN_MODEL_CACHE/extracted"
else
    MODEL_DIR="$REPO_ROOT/target/model20/extracted"
fi

if [ "$HOST_OS" = Darwin ] && [ -n "${PERF_CPU:-}" ]; then
    echo "fatal: PERF_CPU is unsupported on macOS: Mach affinity is a cache hint, not CPU pinning; unset PERF_CPU." >&2
    exit 2
fi
if [ "$HOST_OS" = Darwin ] && ! command -v samply >/dev/null 2>&1; then
    echo "fatal: samply is required on macOS; install it (for example: cargo install samply) or put it on PATH." >&2
    exit 1
fi
SAMPLY_PRESYMBOLICATE=0
if samply_has_presymbolicate; then
    SAMPLY_PRESYMBOLICATE=1
fi
if command -v samply >/dev/null 2>&1 && [ "$SAMPLY_PRESYMBOLICATE" -eq 0 ]; then
    echo "warning: samply has no --unstable-presymbolicate; the hot-stack report will show unresolved frames." >&2
fi
if [ "$CHECK_ONLY" = 1 ]; then
    echo "samply cycles: $SAMPLY_CYCLES"
    if command -v samply >/dev/null 2>&1; then
        if [ "$SAMPLY_PRESYMBOLICATE" -eq 1 ]; then
            echo "samply presymbolicate: available"
        else
            echo "samply presymbolicate: unavailable"
        fi
    fi
    if [ "$HOST_OS" = Darwin ]; then
        echo "profile platform: Darwin; profiler=samply"
        echo "note: taskset and PERF_CPU are unsupported on macOS: Mach affinity is a cache hint, not CPU pinning" >&2
        echo "note: perf record and the perf_event_paranoid hint are Linux-only; skipped on macOS" >&2
    else
        echo "profile platform: $HOST_OS; profiler fallback chain: samply, flamegraph, perf, valgrind"
        echo "fallback cycles: $CYCLES"
    fi
    exit 0
fi
if [ "$HOST_OS" = Darwin ] && ! cargo cinstall --version >/dev/null 2>&1; then
    echo "fatal: cargo-c is required on macOS; install cargo-cinstall or put it on PATH." >&2
    exit 1
fi
if [ "$HOST_OS" = Darwin ] && [ -z "${OXPINYIN_TKRZW_PREFIX:-}" ] && \
   [ -z "${OXPINYIN_TKRZW_LIBRARY:-}" ]; then
    echo "fatal: set OXPINYIN_TKRZW_PREFIX or OXPINYIN_TKRZW_LIBRARY so runtime.sh can verify libtkrzw." >&2
    exit 1
fi

file_sha256() {
    local path=$1
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum -- "$path" | awk '{print $1}'
    else
        shasum -a 256 -- "$path" | awk '{print $1}'
    fi
}

# The extracted tree is the pin only when fetch-model.sh's `verified`
# marker (or the sibling archive digest) matches MODEL20_SHA256.
verify_pinned_model() {
    local dir=$1 name cache_root marker digest archive
    [ -d "$dir" ] || {
        echo "fatal: model dir is not a directory: $dir" >&2
        echo "  run: tools/model/fetch-model.sh" >&2
        exit 1
    }
    for name in "${EXPECTED_MODEL_FILES[@]}"; do
        if [ ! -f "$dir/$name" ]; then
            echo "fatal: model dir $dir is missing $name" >&2
            echo "  run: tools/model/fetch-model.sh" >&2
            exit 1
        fi
    done
    cache_root=$(uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -c 'import os, sys; print(os.path.dirname(os.path.realpath(sys.argv[1])))' "$dir")
    marker="$cache_root/verified"
    if [ -f "$marker" ] && grep -qx "sha256=$MODEL20_SHA256" "$marker"; then
        MODEL_DIR=$(uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$dir")
        return 0
    fi
    archive="$cache_root/downloads/model20.text.tar.gz"
    if [ -f "$archive" ]; then
        digest=$(file_sha256 "$archive")
        if [ "$digest" = "$MODEL20_SHA256" ]; then
            MODEL_DIR=$(uv run --no-project --managed-python --python cpython@3.15+gil python -X lazy_imports=normal -c 'import os, sys; print(os.path.realpath(sys.argv[1]))' "$dir")
            return 0
        fi
        echo "fatal: archive $archive is not the pinned model20" >&2
        echo "  expected: $MODEL20_SHA256" >&2
        echo "  actual:   $digest" >&2
        exit 1
    fi
    echo "fatal: $dir is not a verified model20 cache (no sha256=$MODEL20_SHA256 marker)" >&2
    echo "  run: tools/model/fetch-model.sh" >&2
    exit 1
}

mkdir -p "$PROFILE_DIR"

# ── Inputs ──────────────────────────────────────────────────────────────

# All three backend flavours name their tables identically, so only the
# manifest's backend token distinguishes them; the cycle's oracle is tkrzw.
if ! grep -q "^backend=tkt$" "$EXPORT_DIR/datagen-manifest.txt"; then
	echo "fatal: $EXPORT_DIR is not a tkrzw export; rebuild it with --features tkrzw" >&2
	exit 2
fi
for name in pinyin_index.bin phrase_index.bin bigram.db; do
    if [ ! -f "$EXPORT_DIR/$name" ]; then
        echo "fatal: exported table missing at $EXPORT_DIR/$name" >&2
        exit 1
    fi
done
verify_pinned_model "$MODEL_DIR"

echo "oxpinyin Stage-2 W8-cycle profile"
echo "  pin: $PIN_REF"
echo "  model20 SHA-256: $MODEL20_SHA256"
echo "  model dir: $MODEL_DIR"
echo "  artifacts: $PROFILE_DIR"
echo "  cycles: $CYCLES_NOTE"

# ── cargo-c install with the profiling profile ──────────────────────────

if [ "$HOST_OS" = Darwin ]; then
    if [ -f "$STAGE/usr/lib/pkgconfig/libpinyin.pc" ] && \
       ls "$STAGE"/usr/lib/libpinyin*.dylib >/dev/null 2>&1 && \
       [ "${PROFILE_REUSE_STAGE:-0}" = "1" ]; then
        echo "--- reusing stage: $STAGE ---"
    else
        echo "--- cargo cinstall --profile profiling -> $STAGE ---"
        rm -rf "$STAGE"
        # Homebrew's native libraries are outside the shared cache; opt the
        # profiling dylib out so the linker can bind them.
        export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C link-arg=-Wl,-not_for_dyld_shared_cache"
        cargo cinstall --locked --profile profiling --no-default-features --features tkrzw \
            -p oxpinyin-capi --destdir="$STAGE" --prefix=/usr \
            --manifest-path "$REPO_ROOT/Cargo.toml"
    fi
    export PKG_CONFIG_PATH="$STAGE/usr/lib/pkgconfig:$STAGE/usr/lib64/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
    export PKG_CONFIG_SYSROOT_DIR="$STAGE"
    CAPI_LIBDIR=$(pkg-config --variable=libdir libpinyin)
    [ -n "$CAPI_LIBDIR" ] || CAPI_LIBDIR=$(pkg-config --variable=libdir oxpinyin)
    CAPI_SO=
    for candidate in libpinyin.dylib libpinyin.15.dylib libpinyin.15.0.0.dylib libpinyin_capi.dylib; do
        if [ -f "$CAPI_LIBDIR/$candidate" ]; then
            CAPI_SO="$CAPI_LIBDIR/$candidate"
            break
        fi
    done
else
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
fi
CAPI_DATA="$STAGE/usr/share/oxpinyin"
[ -f "$CAPI_SO" ] || { echo "fatal: $CAPI_SO not found" >&2; exit 1; }

# cargo-c does not install data files; this is the documented packager step.
mkdir -p "$CAPI_DATA"
if [ "$HOST_OS" = Darwin ]; then
    # The native engine resolves the remaining character, merged and table
    # files from the same directory; copy the export's data set, not only the
    # three files the validation above requires.
    cp -L "$EXPORT_DIR"/*.bin "$CAPI_DATA/"
    cp -L "$EXPORT_DIR/bigram.db" "$CAPI_DATA/"
    [ ! -f "$EXPORT_DIR/table.conf" ] || cp -L "$EXPORT_DIR/table.conf" "$CAPI_DATA/"
else
    cp -L "$EXPORT_DIR/pinyin_index.bin" \
          "$EXPORT_DIR/phrase_index.bin" \
          "$EXPORT_DIR/bigram.db" "$CAPI_DATA/"
fi
cp -L "$MODEL_DIR/interpolation2.text" "$CAPI_DATA/interpolation2.text"
if [ "$HOST_OS" = Darwin ]; then
    echo "installed capi dylib: $CAPI_SO"
else
    echo "installed capi .so:  $CAPI_SO"
fi
echo "installed capi data: $CAPI_DATA"

# ── Harness ─────────────────────────────────────────────────────────────

echo "--- building bisect --perf harness ---"
if [ "$HOST_OS" = Darwin ]; then
    cc -std=gnu11 -Wall -Wextra -Werror -O2 -g \
        -o "$PROFILE_DIR/bisect" "$BISECT_SRC"
else
    gcc -std=gnu11 -Wall -Wextra -Werror -O2 -g \
        -o "$PROFILE_DIR/bisect" "$BISECT_SRC" -ldl
fi

if [ "$HOST_OS" != Darwin ]; then
    export LD_LIBRARY_PATH="$CAPI_LIBDIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
export PERF_BACKEND=oxpinyin
export PERF_MODE=speed
export PERF_CYCLES="$CYCLES"

run_pinned() {
    if [ "$HOST_OS" = Darwin ]; then
        "$@"
    elif [ -n "${PERF_CPU:-}" ] && command -v taskset >/dev/null 2>&1; then
        taskset -c "$PERF_CPU" "$@"
    else
        "$@"
    fi
}

# Check the dlopened subject before the profiler starts, then profile the worker.
tkrzw_run auto subject "$CAPI_SO"
CYCLE_CMD=("$PROFILE_DIR/bisect" --perf "$CAPI_SO" "$CAPI_DATA")

# ── Record ──────────────────────────────────────────────────────────────

TIMING_JSON="$PROFILE_DIR/w8-cycle-timing.json"
PROFILE_JSON="$PROFILE_DIR/w8-cycle.profile.json.gz"
PROFILE_SYMS="${PROFILE_JSON%.*}.syms.json"
FLAMEGRAPH_SVG="$PROFILE_DIR/w8-cycle.svg"
PERF_DATA="$PROFILE_DIR/perf.data"
HEADER="$PROFILE_DIR/header.txt"

{
    echo "pin: $PIN_REF"
    echo "model20_sha256: $MODEL20_SHA256"
    echo "so: $CAPI_SO"
    echo "data: $CAPI_DATA"
    echo "cycles: $CYCLES_NOTE"
    echo "profile: profiling (release + line-tables-only + thin LTO)"
} >"$HEADER"

# Print the samply profile's sample total; extract-hot-stacks.py warns on
# stderr when it is below its level. A profile it cannot read costs a warning,
# not the run: samply has already written it and the timing is saved.
report_samples() {
    uv run --no-project --managed-python --python cpython@3.15+gil python \
        -X lazy_imports=normal "$SCRIPT_DIR/extract-hot-stacks.py" \
        --total "$PROFILE_JSON" ||
        echo "warning: could not read the sample total from $PROFILE_JSON" >&2
}

recorded=0
# Never pair a fresh profile with a sidecar from an earlier run.
rm -f "$PROFILE_JSON" "$PROFILE_SYMS"

if command -v samply >/dev/null 2>&1; then
    echo "--- samply record (preferred) ---"
    SAMPLY_ARGS=(record --save-only)
    if [ "$SAMPLY_PRESYMBOLICATE" -eq 1 ]; then
        SAMPLY_ARGS+=(--unstable-presymbolicate)
    fi
    if PERF_CYCLES="$SAMPLY_CYCLES" run_pinned samply "${SAMPLY_ARGS[@]}" -o "$PROFILE_JSON" -- \
        "${CYCLE_CMD[@]}" | tee "$TIMING_JSON"
    then
        echo "samply profile: $PROFILE_JSON"
        if [ -f "$PROFILE_SYMS" ]; then
            echo "samply symbols: $PROFILE_SYMS"
        else
            echo "warning: samply wrote no symbol sidecar; the hot-stack report will show unresolved frames." >&2
        fi
        report_samples
        recorded=1
    else
        if [ "$HOST_OS" = Darwin ]; then
            echo "error: samply failed; macOS has no perf or Callgrind fallback." >&2
            exit 1
        else
            echo "warning: samply failed (often /proc/sys/kernel/perf_event_paranoid > 1); falling back" >&2
        fi
    fi
fi

if [ "$HOST_OS" = Darwin ]; then
    echo "note: perf record and the perf_event_paranoid hint are Linux-only; skipped on macOS" >&2
    echo "note: flamegraph and Valgrind/Callgrind fallbacks are not used on macOS; samply is required" >&2
fi

if [ "$HOST_OS" != Darwin ] && [ "$recorded" -eq 0 ] && command -v flamegraph >/dev/null 2>&1; then
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

if [ "$HOST_OS" != Darwin ] && [ "$recorded" -eq 0 ] && command -v perf >/dev/null 2>&1; then
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

if [ "$HOST_OS" != Darwin ] && [ "$recorded" -eq 0 ] && command -v valgrind >/dev/null 2>&1; then
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
