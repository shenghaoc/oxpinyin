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
#                         core tables under libpinyin's own names
#                         (pinyin_index.bin, phrase_index.bin, bigram.db,
#                         the layout every datagen output and libpinyin
#                         install uses) plus interpolation2.text are
#                         required, and the directory must have been written
#                         by the cell's backend (its datagen-manifest.txt
#                         `backend=` or table.conf `database format:`); a
#                         directory from another backend FAILS (exit 1).
#   PINYIN_ORACLE_DBM     the backend cell: bdb (default — the reference
#                         build is a bare ./configure), kc or tkrzw. The
#                         oracle prefix must be built with the same
#                         `build-oracle.sh --dbm`, and the capi is built
#                         with the matching cargo feature
#                         (tools/bisection/oracle-cell.sh).

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
REPO_ROOT="$(git rev-parse --show-toplevel)"
# The backend cell (PINYIN_ORACLE_DBM, default bdb) fixes the expected
# pin ref (+dbm-<cell>), the capi's cargo feature and the data's
# backend. Exact whole-line match on the ref so an oracle built against
# a different model checksum or another cell's DBM does not silently
# validate this differential.
# shellcheck source=oracle-cell.sh
source ./oracle-cell.sh
# shellcheck source=system-dir.sh
source ./system-dir.sh
if ! grep -Fxq "$EXPECTED_PIN_REF" "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin for the $ORACLE_DBM cell"
    echo "  expected $EXPECTED_PIN_REF"
    echo "  (build it with build-oracle.sh --dbm $ORACLE_DBM, or set PINYIN_ORACLE_DBM)"
    exit 0
fi
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
echo "building libpinyin_capi.so (release, $CAPI_FEATURE)..."
(cd "$REPO_ROOT" && CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --release -p oxpinyin-capi \
    --no-default-features --features "$CAPI_FEATURE") || exit 1

echo "--- cc key-surface-diff.c ---"
DRIVER="$CARGO_TARGET_DIR/key-surface-diff"
cc -O2 -o "$DRIVER" key-surface-diff.c -ldl || exit 1

SYSTEM="${KEY_SYSTEM:-}"
# The capi under test is built for the cell's backend (CAPI_FEATURE), so
# the data must be that backend's: all three DBMs use libpinyin's own
# file names, so the names alone cannot tell a Kyoto Cabinet directory
# from a Berkeley DB one — the backend check below does.
missing=()
for t in pinyin_index.bin phrase_index.bin bigram.db interpolation2.text; do
    [[ -n "$SYSTEM" && -f "$SYSTEM/$t" ]] || missing+=("$t")
done
if ((${#missing[@]})); then
    echo "SKIP: KEY_SYSTEM must name a $ORACLE_DBM_NAME system data dir"
    echo "  missing: ${missing[*]}"
    exit 0
fi
if ! data_ext=$(system_dir_data_ext "$SYSTEM"); then
    echo "FAIL: cannot tell which backend wrote KEY_SYSTEM=$SYSTEM"
    echo "  (no datagen-manifest.txt backend= and no known table.conf database format:)"
    exit 1
fi
if [[ $data_ext != "$OXPINYIN_CAPI_BACKEND_EXT" ]]; then
    echo "FAIL: BACKEND MISMATCH — the $ORACLE_DBM cell's capi is $(system_dir_ext_name "$OXPINYIN_CAPI_BACKEND_EXT"),"
    echo "  but KEY_SYSTEM=$SYSTEM was written for $(system_dir_ext_name "$data_ext")"
    exit 1
fi

echo "--- capi side ---"
CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
# The driver opens the context with an empty user dir (""), which both
# libraries resolve against the working directory: the pin writes
# user.conf there. Each side therefore runs in a fresh private directory
# (both start equal, and nothing lands in tools/bisection), removed on
# exit. The data paths are made absolute first, since the driver runs
# from elsewhere.
CAPI_CWD="$(mktemp -d)"
ORACLE_CWD="$(mktemp -d)"
trap 'rm -rf "$CAPI_LOG" "$ORACLE_LOG" "$CAPI_CWD" "$ORACLE_CWD"' EXIT
SYSTEM="$(cd "$SYSTEM" && pwd)"
ORACLE_DATA="$(cd "$ORACLE_DATA" && pwd)"
ORACLE_SO="$(cd "$(dirname "$ORACLE_SO")" && pwd)/$(basename "$ORACLE_SO")"
if ! (cd "$CAPI_CWD" && "$DRIVER" "$CAPI_SO" "$SYSTEM") > "$CAPI_LOG"; then
    echo "FAIL: key-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
if ! (cd "$ORACLE_CWD" && "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA") > "$ORACLE_LOG"; then
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
