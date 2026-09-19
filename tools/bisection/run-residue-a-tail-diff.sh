#!/usr/bin/env bash
# run-residue-a-tail-diff.sh — mechanism probe for residue A (the missing
# user-phrase tail) and the common-root test against residues B and C in
# docs/findings/probe-coverage-abi.md. Diagnosis only: diffs the two ABI
# logs and captures the side-specific dumps; never asserts a class.
#
# Env-gated on the pin-built oracle (PINYIN_ORACLE_PREFIX). The capi
# system directory resolves through system-dir.sh; RESIDUE_A_SAME_DIR=1
# drives both .so files against the oracle's own data/ (the settling
# configuration). Optional:
#
#   RESIDUE_A_CAPI_SO      a capi .so to probe instead of the fresh debug
#                          build (the counterfactual build of the
#                          common-root experiment)
#   RESIDUE_A_DUMP_PREFIX  an oracle prefix built with
#                          --apply-patches tools/bisection/patches/nbest-tails-dump;
#                          the driver runs against it once more with
#                          OXPINYIN_DUMP_TAILS=1 and the stderr dump of the
#                          trellis tails lands in $OUT/pin-tails.log
#   RESIDUE_A_PROBE=1      also run the runtime-side probe
#                          (crates/oxpinyin-runtime/examples/nbest_tail_probe)
#                          on the same system dir into $OUT/ox-probe.log
#   RESIDUE_A_OUT          output directory (default: a mktemp dir,
#                          removed when the logs are identical)
#
# Exit: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
# shellcheck source=tools/bisection/system-dir.sh
. ./system-dir.sh
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building residue-a-tail driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o residue-a-tail-diff \
    residue-a-tail-diff.c -ldl
echo "build: ok"

if [[ -n "${RESIDUE_A_CAPI_SO:-}" ]]; then
    CAPI_SO="$RESIDUE_A_CAPI_SO"
    echo "capi (supplied): $CAPI_SO"
else
    echo "--- building oxpinyin-capi ---"
    cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
    CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
    echo "capi: $CAPI_SO"
fi
if [[ ! -f "$CAPI_SO" ]]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi
if [[ ! -f "$ORACLE_DATA/bigram.db" ]]; then
    echo "SKIP: oracle data not found at $ORACLE_DATA"
    exit 0
fi

if [[ "${RESIDUE_A_SAME_DIR:-}" == "1" ]]; then
    SYSTEM="$ORACLE_DATA"
    echo "same-dir mode: both sides on $SYSTEM"
else
    SYSTEM="$(resolve_system_dir RESIDUE_A_SYSTEM residue-a-tail)"
fi

OUT_DIR="${RESIDUE_A_OUT:-}"
if [[ -z "$OUT_DIR" ]]; then
    OUT_DIR="$(mktemp -d)"
    CLEAN_OUT=1
else
    mkdir -p "$OUT_DIR"
    CLEAN_OUT=0
fi
CAPI_LOG="$OUT_DIR/residue-a-capi.log"
ORACLE_LOG="$OUT_DIR/residue-a-oracle.log"

echo "--- capi side ---"
if ! ./residue-a-tail-diff "$CAPI_SO" "$SYSTEM" > "$CAPI_LOG" 2>"$OUT_DIR/capi.err"; then
    echo "FAIL: residue-a-tail-diff crashed against oxpinyin-capi"
    cat "$OUT_DIR/capi.err" || true
    [[ "$CLEAN_OUT" == 1 ]] && rm -rf "$OUT_DIR"
    exit 1
fi
echo "oxpinyin-capi: ok ($(wc -l < "$CAPI_LOG") log lines)"

echo "--- oracle side ---"
if ! ./residue-a-tail-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2>"$OUT_DIR/oracle.err"; then
    echo "FAIL: residue-a-tail-diff crashed against the oracle"
    cat "$OUT_DIR/oracle.err" || true
    [[ "$CLEAN_OUT" == 1 ]] && rm -rf "$OUT_DIR"
    exit 1
fi
echo "oracle: ok ($(wc -l < "$ORACLE_LOG") log lines)"

if [[ -n "${RESIDUE_A_DUMP_PREFIX:-}" ]]; then
    DUMP_SO="$RESIDUE_A_DUMP_PREFIX/lib/libpinyin.so"
    DUMP_DATA="$RESIDUE_A_DUMP_PREFIX/lib/libpinyin/data"
    echo "--- oracle side, instrumented (tails to stderr) ---"
    if [[ ! -f "$DUMP_SO" ]]; then
        echo "FAIL: instrumented oracle not found at $DUMP_SO"
        exit 1
    fi
    if ! OXPINYIN_DUMP_TAILS=1 ./residue-a-tail-diff "$DUMP_SO" "$DUMP_DATA" \
        > "$OUT_DIR/residue-a-oracle-dump.log" 2>"$OUT_DIR/pin-tails.log"; then
        echo "FAIL: residue-a-tail-diff crashed against the instrumented oracle"
        exit 1
    fi
    if ! diff -q "$ORACLE_LOG" "$OUT_DIR/residue-a-oracle-dump.log" >/dev/null; then
        echo "FAIL: the instrumented oracle's stdout differs from the pin's"
        exit 1
    fi
    echo "instrumented oracle: stdout identical to the pin; tails in $OUT_DIR/pin-tails.log"
fi

if [[ "${RESIDUE_A_PROBE:-}" == "1" ]]; then
    echo "--- runtime-side probe ---"
    PROBE_USER="$(mktemp -d)"
    if ! cargo run -q -p oxpinyin-runtime --example nbest_tail_probe \
        --manifest-path "$REPO_ROOT/Cargo.toml" -- "$SYSTEM" "$PROBE_USER" \
        > "$OUT_DIR/ox-probe.log" 2>"$OUT_DIR/ox-probe.err"; then
        echo "FAIL: nbest_tail_probe failed"
        cat "$OUT_DIR/ox-probe.err" || true
        rm -rf "$PROBE_USER"
        exit 1
    fi
    rm -rf "$PROBE_USER"
    echo "probe: $OUT_DIR/ox-probe.log"
fi

echo "--- diff ---"
if diff -u "$ORACLE_LOG" "$CAPI_LOG"; then
    echo "IDENTICAL"
    [[ "$CLEAN_OUT" == 1 ]] && rm -rf "$OUT_DIR"
    exit 0
fi
echo "DIVERGENT (expected while residue A remains open)"
echo "logs: $ORACLE_LOG  $CAPI_LOG"
exit 2
