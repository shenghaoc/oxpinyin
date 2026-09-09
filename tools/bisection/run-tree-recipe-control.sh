#!/usr/bin/env bash
# run-tree-recipe-control.sh — within-session separation of the engine-tree
# effect from the build-recipe effect on the steady keystroke-cycle
# wall-clock ratio.
#
# The confound this exists to resolve: the merged record's amd64 pass and the
# branch's pass differ in TWO ways — the engine tree (f79f665d -> current,
# session rewrite) and the build recipe (record: cargo build + strip at run
# time; branch: cargo cinstall). Either alone could have moved the ratio, and
# nothing on the branch separated them.
#
# Four cells, ONE container, ONE data directory, ONE harness, ONE session,
# round-robin within each round:
#
#   L  libpinyin, in-image (pin 074a2219, 2.11.92)          the denominator
#   X  oxpinyin at f79f665d, record recipe (cargo build + strip)
#   Y  oxpinyin at the current tree, record recipe
#   Z  oxpinyin at the current tree, cargo cinstall
#
#   tree effect   = Y/L - X/L   (recipe held constant: both cargo build)
#   recipe effect = Z/L - Y/L   (tree held constant: both current)
#
# X/L is the reproduction gate against the record's own numbers. Z/L is a
# soundness check on the prior session's 0.9527 — reported, never a decision
# input, because it is the one cross-session comparison left.
#
# Runs inside localhost/oxpinyin-matrix:knob-amd64 — the record's own image,
# whose /repo is byte-identical to f79f665d. The harness is that image's
# bisect.c; its run_perf_unit is byte-identical to the current tree's, so the
# timed unit is the record's.
#
# Usage:
#   podman run --rm --security-opt label=disable \
#     -v /tmp/ct-old:/src-old:Z -v /tmp/ct-curr:/src-curr:Z \
#     -v <tree>/tools:/ctltools:Z -v /tmp/control-out:/out:Z \
#     -e CTRL_COMMIT=$(git rev-parse HEAD) \
#     --entrypoint bash localhost/oxpinyin-matrix:knob-amd64 \
#     /ctltools/bisection/run-tree-recipe-control.sh
#
# Captures are written to /out and are not committed.

set -euo pipefail

SRC_OLD=${SRC_OLD:-/src-old}
SRC_CURR=${SRC_CURR:-/src-curr}
OUT=${CTRL_OUT:-/out}
RUNS=${CTRL_RUNS:-20}
CYCLES=${CTRL_CYCLES:-8}
SIZES=${CTRL_SIZES:-1 8 16}
CPU=${CTRL_CPU:-0}
CTLTOOLS=${CTLTOOLS:-/ctltools}
mkdir -p "$OUT"

L_LABEL=libpinyin-tkrzw
X_LABEL=oxpinyin-old-cargo
Y_LABEL=oxpinyin-cur-cargo
Z_LABEL=oxpinyin-cur-cinstall
S_LABEL=oxpinyin-cur-shipped

# The record's own documented Tkrzw artifact (docs/findings/
# perf-steady-cycle-cross-host-2026-09-07.md, amd64 capi-artifacts row).
RECORD_X_SHA=bf8d3b5707b5bc1bafc09670185b96770b73f596dd2eb57b8a9cba46bad4aec8

LP_LIB=/opt/libpinyin-tkrzw/lib
DATA=$LP_LIB/libpinyin/data
L_SO=$(find /opt/libpinyin-tkrzw -name 'libpinyin.so' \( -type f -o -type l \) -print -quit)

# ── environment ─────────────────────────────────────────────────────
{
    echo "=== environment ==="
    echo "captured_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "controller_commit: ${CTRL_COMMIT:-unset}"
    echo "container_arch: $(uname -m)"
    echo "kernel: $(uname -srvm)"
    echo "nproc: $(nproc)"
    echo "cpu: $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')"
    echo "memtotal: $(grep -m1 MemTotal /proc/meminfo)"
    echo "gcc: $(gcc --version | head -1)"
    echo "rustc: $(rustc --version)"
    echo "cargo: $(cargo --version)"
    echo "libtkrzw: $(pkg-config --modversion tkrzw 2>/dev/null || echo unknown)"
    echo "libpinyin: $(grep -h '^Version:' /opt/libpinyin-tkrzw/lib/pkgconfig/libpinyin.pc 2>/dev/null || echo unknown)"
    echo "data_dir: $DATA"
    echo "loadavg: $(cat /proc/loadavg)"
} | tee "$OUT/environment.txt"

# ── builds: X and Y with the record recipe; Z with cinstall ─────────
build_cargo() { # <src> <out>
    local src=$1 out=$2 target
    target=$(mktemp -d)
    cargo build --locked --release -p oxpinyin-capi \
        --no-default-features --features tkrzw \
        --manifest-path "$src/Cargo.toml" \
        --target-dir "$target" >&2
    cp "$target/release/libpinyin_capi.so" "$out"
    strip --strip-all "$out"
}

build_cinstall() { # <src> <out> <features>
    local src=$1 out=$2 feats=$3 target stage
    target=$(mktemp -d); stage=$(mktemp -d)
    cargo cinstall --locked --release -p oxpinyin-capi \
        --no-default-features --features "$feats" \
        --manifest-path "$src/Cargo.toml" \
        --target-dir "$target" --destdir "$stage" --prefix /usr >&2
    cp "$(find "$stage" -name 'libpinyin.so.15.0.0' -print -quit)" "$out"
}

X_SO=/tmp/X-old-cargo.so
Y_SO=/tmp/Y-cur-cargo.so
Z_SO=/tmp/Z-cur-cinstall.so
S_SO=/tmp/S-cur-shipped.so
build_cargo "$SRC_OLD" "$X_SO"
build_cargo "$SRC_CURR" "$Y_SO"
build_cinstall "$SRC_CURR" "$Z_SO" tkrzw
build_cinstall "$SRC_CURR" "$S_SO" tkrzw,shipped

{
    echo "=== build identity ==="
    for pair in "L:$L_SO" "X:$X_SO" "Y:$Y_SO" "Z:$Z_SO" "S:$S_SO"; do
        tag=${pair%%:*}; so=${pair#*:}
        real=$(readlink -f "$so")
        hooks=$(nm -D "$real" 2>/dev/null | grep -c 'oxpinyin_init_for_fixtures\|oxpinyin_test_set_user_bigram' || true)
        echo "$tag: $(sha256sum "$real" | cut -d' ' -f1) $(stat -c%s "$real") B  fixture-hooks: $hooks  NEEDED: $(readelf -d "$real" | sed -n 's/.*Shared library: \[\(.*\)\]/\1/p' | tr '\n' ' ')"
    done
    echo "record X: $RECORD_X_SHA 1688840 B (docs/findings/perf-steady-cycle-cross-host-2026-09-07.md)"
    echo "fixture-hook expectation: Z 2, S 0 (the shipped feature compiles them out)"
} | tee "$OUT/build-identity.txt"

# ── hard gate: X must be the record's artifact, byte for byte ───────
x_sha=$(sha256sum "$X_SO" | cut -d' ' -f1)
if [ "$x_sha" != "$RECORD_X_SHA" ]; then
    echo "FATAL: cell X is not the record's artifact." >&2
    echo "  built:  $x_sha" >&2
    echo "  record: $RECORD_X_SHA" >&2
    echo "The control cannot proceed: X must reproduce the record's build before" >&2
    echo "any ratio is read. Stop and report." >&2
    exit 1
fi
echo "GATE: X reproduces the record's artifact byte-for-byte ($x_sha)"

# ── harness: the image's own bisect.c (== f79f665d) ─────────────────
BISECT=/tmp/bisect
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$BISECT" /repo/tools/bisection/bisect.c -ldl
echo "harness: $(sha256sum /repo/tools/bisection/bisect.c | cut -d' ' -f1) (record image /repo)"

# ── the run: one session, round-robin L X Y Z S within each round ─────
SPEED=$OUT/speed.jsonl
: > "$SPEED"
: > "$OUT/load.txt"
for n in $SIZES; do
    echo "--- n=$n: $RUNS runs x $CYCLES cycles x $n repeats, round-robin L X Y Z S ---"
    echo "before n=$n: $(cat /proc/loadavg)" >> "$OUT/load.txt"
    for _ in $(seq 1 "$RUNS"); do
        for cell in "$L_LABEL:$L_SO" "$X_LABEL:$X_SO" "$Y_LABEL:$Y_SO" "$Z_LABEL:$Z_SO" "$S_LABEL:$S_SO"; do
            label=${cell%%:*}; so=${cell#*:}
            taskset -c "$CPU" env LD_LIBRARY_PATH="$LP_LIB" \
                PERF_BACKEND="$label" PERF_MODE=speed \
                PERF_CYCLES="$CYCLES" PERF_REPEATS="$n" \
                "$BISECT" --perf "$so" "$DATA" >> "$SPEED" 2>>"$OUT/run.err"
        done
    done
    echo "after  n=$n: $(cat /proc/loadavg)" >> "$OUT/load.txt"
done
echo "speed rows: $(wc -l < "$SPEED")"

# ── ratios, through the committed perf-ratio.py ─────────────────────
for n in $SIZES; do
    {
        echo "## size n=$n — ratios vs L (perf-ratio.py, seed 20260907)"
        for pair in "$X_LABEL:$L_LABEL" "$Y_LABEL:$L_LABEL" \
                    "$Z_LABEL:$L_LABEL" "$S_LABEL:$L_LABEL" \
                    "$S_LABEL:$Z_LABEL"; do
            num=${pair%%:*}; den=${pair#*:}
            python3 "$CTLTOOLS/bisection/perf-ratio.py" "$SPEED" \
                --numerator "$num" --denominator "$den" \
                --repeats "$n" --format md
        done
    } > "$OUT/ratios-n$n.md"
done

# ── decomposition: paired round bootstrap ───────────────────────────
for n in $SIZES; do
    python3 "$CTLTOOLS/bisection/perf-decomp.py" "$SPEED" --repeats "$n" \
        --l "$L_LABEL" --x "$X_LABEL" --y "$Y_LABEL" --z "$Z_LABEL" \
        --format md > "$OUT/decomp-n$n.md"
done

echo
echo "=== captures written to $OUT ==="
ls -1 "$OUT"
