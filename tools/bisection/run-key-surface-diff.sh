#!/usr/bin/env bash
# run-key-surface-diff.sh — Tier-A ABI parity differential: single-key
# parsing (`pinyin_parse_full_pinyin` / `_double_` / `_chewing`) and the
# `ChewingKey` display getters, plus the `pinyin_get_context` and
# addon-unload contracts.
#
# Drives the scripted C-ABI sequence (key-surface-diff.c) into
# libpinyin_capi.so and the pin-built libpinyin.so and diffs the FULL
# logs. The two-byte key logged per probe is the byte-identity check of
# the packed `ChewingKey` bitfield (D1's cross-engine layout
# verification; the driver TU additionally static-asserts the mirror
# sizes).
#
# Option profiles: the parity word `0x18a`, `+USE_TONE` (0x1aa),
# `+USE_TONE|FORCE_TONE` (0x1ea), `FORCE_TONE` alone (0x1ca) — the D3
# FORCE_TONE-law differential for the double/zhuyin single-key seams.
# Scheme sweeps: double pinyin 1..6, chewing 1..6+8+9 (the pins abort on
# double 30 and zhuyin 7 — the recorded #109 contract slots).
#
# Exit codes: 0 = identical or skipped; 1 = build/run failure;
# 2 = divergence; 3 = a probe surface is inactive (the run would have
# compared nothing). Skip → 0 matches the sibling `run-*-diff.sh` family.
#
# Env:
#   PINYIN_ORACLE_PREFIX  pin-built oracle prefix
#                         (default ~/.local/opt/pinyin-oracle)
#   KEY_SYSTEM            an oxpinyin-native system dir for the capi
#                         side (the oracle side always reads from
#                         PINYIN_ORACLE_PREFIX/lib/libpinyin/data). The
#                         three `.kct` tables (pinyin_index, phrase_index,
#                         bigram) plus interpolation2.text are required.
#                         `.kct` because this script's own `cargo build
#                         -p oxpinyin-capi` uses default features → KC,
#                         so the built `.so`'s compiled-in backend can
#                         only read `.kct`. Point at a redb/lmdb/tkrzw
#                         dir and the driver will fail to open it.

set -u
cd "$(dirname "$0")" || exit 1

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [[ ! -f "$PREFIX/oracle-pin.txt" || ! -f "$ORACLE_SO" ]]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 0
fi
# Full ORACLE_PIN_REF from tools/oracle/build-oracle.sh (LIBPINYIN_TAG,
# LIBPINYIN_SHA, MODEL_SHA256, dbm). Exact whole-line match so an oracle
# built against a different model checksum or a different DBM backend
# does not silently validate this differential.
EXPECTED_PIN_REF='pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw'
if ! grep -Fxq "$EXPECTED_PIN_REF" "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    exit 0
fi

REPO_ROOT="$(git rev-parse --show-toplevel)"
# Honour CARGO_TARGET_DIR so a caller who redirects cargo output (a
# distro-package builder, a shared-target CI, a per-worktree target)
# ends up loading the .so cargo actually wrote instead of a stale one
# at $REPO_ROOT/target. Anchor a relative value to $REPO_ROOT rather
# than `$(pwd)` — the shell is `cd`'d into tools/bisection by this
# point, which is not what a caller means by a relative path.
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
case "$CARGO_TARGET_DIR" in
    /*) ;;
    *) CARGO_TARGET_DIR="$REPO_ROOT/$CARGO_TARGET_DIR" ;;
esac
CAPI_SO="$CARGO_TARGET_DIR/release/libpinyin_capi.so"
# Always rebuild — cargo is a no-op when the artifact is current, and a
# stale libpinyin_capi.so on disk would otherwise mask the change under
# test.
echo "building libpinyin_capi.so (release)..."
(cd "$REPO_ROOT" && CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --release -p oxpinyin-capi) || exit 1

echo "--- cc key-surface-diff.c ---"
DRIVER="$CARGO_TARGET_DIR/key-surface-diff"
cc -O2 -o "$DRIVER" key-surface-diff.c -ldl || exit 1

SYSTEM="${KEY_SYSTEM:-}"
# This script's `cargo build -p oxpinyin-capi` above uses default
# features → Kyoto Cabinet, so the `.so` under test only opens `.kct`
# tables. Accepting other peer extensions here would pass the gate on
# a dir the driver cannot actually load, and the failure would land
# mid-run rather than as this clean skip.
has_all_kct_tables() {
    local t
    for t in pinyin_index phrase_index bigram; do
        [[ -f "$SYSTEM/$t.kct" ]] || return 1
    done
}
if [[ -z "$SYSTEM" ]] || ! [[ -f "$SYSTEM/interpolation2.text" ]] \
    || ! has_all_kct_tables; then
    echo "SKIP: KEY_SYSTEM must name an oxpinyin-native KC system dir"
    echo "  (pinyin_index.kct, phrase_index.kct and bigram.kct plus"
    echo "  interpolation2.text)"
    exit 0
fi

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
trap 'rm -f "$CAPI_LOG" "$ORACLE_LOG"' EXIT
if ! "$DRIVER" "$CAPI_SO" "$SYSTEM" > "$CAPI_LOG"; then
    echo "FAIL: key-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
if ! "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG"; then
    echo "FAIL: key-surface-diff crashed against oracle"
    cat "$ORACLE_LOG"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: every probe family is active on both sides ---"
# The full probe matrix the driver runs must be present on both logs
# before diff is trusted. Anchors are picked so that a per-dimension
# collapse (a profile refused for the whole run, a scheme silently
# skipped, a getter never dispatched) is caught here rather than
# swallowed as an "identical" empty comparison.
probe_surfaces() {
    local f=$1
    # Four option profiles must exercise the full-pinyin seam.
    grep -q '^full|0x18a|' "$f" && \
        grep -q '^full|0x1aa|' "$f" && \
        grep -q '^full|0x1ea|' "$f" && \
        grep -q '^full|0x1ca|' "$f" && \
        # Six double-pinyin schemes must each land at least one probe
        # under the baseline profile (the four-profile sweep on top is
        # what the diff itself pins byte-for-byte).
        grep -q '^double|1|0x18a|' "$f" && \
        grep -q '^double|2|0x18a|' "$f" && \
        grep -q '^double|3|0x18a|' "$f" && \
        grep -q '^double|4|0x18a|' "$f" && \
        grep -q '^double|5|0x18a|' "$f" && \
        grep -q '^double|6|0x18a|' "$f" && \
        # Eight live chewing keyboards must each land at least one probe.
        grep -q '^chewing|1|0x18a|' "$f" && \
        grep -q '^chewing|2|0x18a|' "$f" && \
        grep -q '^chewing|3|0x18a|' "$f" && \
        grep -q '^chewing|4|0x18a|' "$f" && \
        grep -q '^chewing|5|0x18a|' "$f" && \
        grep -q '^chewing|6|0x18a|' "$f" && \
        grep -q '^chewing|8|0x18a|' "$f" && \
        grep -q '^chewing|9|0x18a|' "$f" && \
        # Every display-getter kind must have been dispatched.
        grep -q '^render|full|.*|zhuyin|' "$f" && \
        grep -q '^render|full|.*|pinyin|' "$f" && \
        grep -q '^render|full|.*|luoma|' "$f" && \
        grep -q '^render|full|.*|secondary|' "$f" && \
        grep -q '^render|full|.*|shengmu|' "$f" && \
        grep -q '^render|full|.*|yunmu|' "$f" && \
        grep -q '^render|full|.*|shengmu_skip|' "$f" && \
        grep -q '^render|full|.*|incomplete|' "$f" && \
        # Zero-key sentinels: the crash-if-guard-fails invariants.
        grep -q '^zero|zhuyin|' "$f" && \
        grep -q '^zero|strings|' "$f" && \
        grep -q '^zero|incomplete|' "$f" && \
        # Context + addon-unload contracts.
        grep -q '^context|match|' "$f" && \
        grep -q '^unload_addon|0|' "$f" && \
        grep -q '^unload_addon|5|' "$f" && \
        grep -q '^unload_addon|15|' "$f"
}
if ! probe_surfaces "$CAPI_LOG" || ! probe_surfaces "$ORACLE_LOG"; then
    echo "FAIL: probe matrix is incomplete (missing profile / scheme / render kind / zero / context / addon row)"
    echo "  An inactive slice means that surface compared nothing."
    exit 3
fi

diff -u "$ORACLE_LOG" "$CAPI_LOG"
diff_status=$?
case "$diff_status" in
    0)
        echo "IDENTICAL: $(wc -l < "$CAPI_LOG") probe lines agree with the pin"
        ;;
    1)
        echo
        echo "DIVERGENCE: the logs differ (above)"
        exit 2
        ;;
    *)
        echo
        echo "FAIL: diff exited with status $diff_status (comparison error, not a divergence)"
        exit 1
        ;;
esac
