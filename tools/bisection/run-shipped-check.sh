#!/usr/bin/env bash
# run-shipped-check.sh — is the actual shipping artifact (cargo cinstall
# --features shipped) the same as cell Z (cargo cinstall, no shipped flag)?
#
# The reframe says cinstall is the shipping path and Z is the product. The
# release workflow runs tools/packaging/install.sh, which runs `cargo
# cinstall`, and the crate's own docs name the shipped drop-in as
# `cargo capi install --features shipped`. The `shipped` feature compiles out
# two fixture hooks, so the literal product artifact is cinstall + shipped,
# not cinstall alone. This measures the difference within one session:
#
#   L  libpinyin in-image
#   Z  current tree, cargo cinstall (no shipped flag)
#   S  current tree, cargo cinstall --features shipped   <- the product
#
# Z/L and S/L in one session make the shipped-vs-plain difference a
# within-session comparison; the delta against the prior 0.9527 stays a
# soundness check, never a decision input.

set -euo pipefail
SRC_CURR=${SRC_CURR:-/src-curr}
OUT=${CTRL_OUT:-/out}
RUNS=${CTRL_RUNS:-20}
CYCLES=${CTRL_CYCLES:-8}
SIZES=${CTRL_SIZES:-8 16}
CPU=${CTRL_CPU:-0}
CTLTOOLS=${CTLTOOLS:-/ctltools}
mkdir -p "$OUT"

L_LABEL=libpinyin-tkrzw
Z_LABEL=oxpinyin-cur-cinstall
S_LABEL=oxpinyin-shipped

LP_LIB=/opt/libpinyin-tkrzw/lib
DATA=/opt/libpinyin-tkrzw/lib/libpinyin/data
L_SO=$(find /opt/libpinyin-tkrzw -name 'libpinyin.so' \( -type f -o -type l \) -print -quit)

build_cinstall() { # <out> <extra-features>
    local out=$1 feats=$2 target stage
    target=$(mktemp -d); stage=$(mktemp -d)
    cargo cinstall --locked --release -p oxpinyin-capi \
        --no-default-features --features "$feats" \
        --manifest-path "$SRC_CURR/Cargo.toml" \
        --target-dir "$target" --destdir "$stage" --prefix /usr >&2
    cp "$(find "$stage" -name 'libpinyin.so.15.0.0' -print -quit)" "$out"
}

Z_SO=/tmp/Z-plain.so
S_SO=/tmp/S-shipped.so
build_cinstall "$Z_SO" tkrzw
build_cinstall "$S_SO" tkrzw,shipped

{
    echo "=== identity (shipped check) ==="
    echo "captured_utc: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    for pair in "L:$L_SO" "Z:$Z_SO" "S:$S_SO"; do
        tag=${pair%%:*}; so=${pair#*:}; real=$(readlink -f "$so")
        echo "$tag: $(sha256sum "$real" | cut -d' ' -f1) $(stat -c%s "$real") B"
    done
    echo "Z fixture hooks: $(nm -D "$(readlink -f "$Z_SO")" | grep -c 'oxpinyin_init_for_fixtures\|oxpinyin_test_set_user_bigram')"
    echo "S fixture hooks: $(nm -D "$(readlink -f "$S_SO")" | grep -c 'oxpinyin_init_for_fixtures\|oxpinyin_test_set_user_bigram')"
} | tee "$OUT/shipped-identity.txt"

BISECT=/tmp/bisect
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$BISECT" /repo/tools/bisection/bisect.c -ldl

SPEED=$OUT/speed-shipped.jsonl
: > "$SPEED"
for n in $SIZES; do
    echo "--- n=$n: $RUNS runs, round-robin L Z S ---"
    for _ in $(seq 1 "$RUNS"); do
        for cell in "$L_LABEL:$L_SO" "$Z_LABEL:$Z_SO" "$S_LABEL:$S_SO"; do
            label=${cell%%:*}; so=${cell#*:}
            taskset -c "$CPU" env LD_LIBRARY_PATH="$LP_LIB" \
                PERF_BACKEND="$label" PERF_MODE=speed \
                PERF_CYCLES="$CYCLES" PERF_REPEATS="$n" \
                "$BISECT" --perf "$so" "$DATA" >> "$SPEED" 2>>"$OUT/shipped.err"
        done
    done
done

for n in $SIZES; do
    {
        echo "## n=$n"
        for num in "$Z_LABEL" "$S_LABEL"; do
            python3 "$CTLTOOLS/bisection/perf-ratio.py" "$SPEED" \
                --numerator "$num" --denominator "$L_LABEL" \
                --repeats "$n" --format md
        done
    } > "$OUT/shipped-ratios-n$n.md"
done
echo "done: $(wc -l < "$SPEED") rows"
