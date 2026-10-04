#!/usr/bin/env bash
# run-phrase-surface-diff.sh — Tier-B ABI differential: the phrase-result
# surface (`pinyin_phrase_segment` + `pinyin_get_n_phrase` +
# `pinyin_get_phrase_token`) and the prefix-seeded sentence guess
# (`pinyin_guess_sentence_with_prefix`).
#
# None of the five Tier-B symbols has a consumer call site in either
# frontend, so this driver (phrase-surface-diff.c) is their only oracle
# coverage: segment probes over the real tables log retval plus the full
# token@position array shape (failed-match all-null arrays included),
# and prefix-seeded guesses log retval plus the row-0 sentence.
#
# Exit codes: 0 = identical (the oracle actually ran); 1 = build/run
# failure; 2 = divergence; 3 = the validation could not run (probe
# family inactive, or the pin-built oracle is unavailable or off-pin).
#
# Env:
#   PINYIN_ORACLE_PREFIX  pin-built oracle prefix
#                         (default ~/.local/opt/pinyin-oracle)
#   PHRASE_SYSTEM         an oxpinyin-native system dir for the capi
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
    exit 3
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
    exit 3
fi
CAPI_SO="$CARGO_TARGET_DIR/release/libpinyin_capi.so"
# Always rebuild — cargo is a no-op when the artifact is current, and a
# stale libpinyin_capi.so on disk would otherwise mask the change under
# test.
echo "building libpinyin_capi.so (release, $CAPI_FEATURE)..."
(cd "$REPO_ROOT" && CARGO_TARGET_DIR="$CARGO_TARGET_DIR" cargo build --release -p oxpinyin-capi \
    --no-default-features --features "$CAPI_FEATURE") || exit 1

echo "--- cc phrase-surface-diff.c ---"
DRIVER="$CARGO_TARGET_DIR/phrase-surface-diff"
cc -O2 -Wall -o "$DRIVER" phrase-surface-diff.c -ldl || exit 1

SYSTEM="${PHRASE_SYSTEM:-}"
# Relative PHRASE_SYSTEM is anchored to $REPO_ROOT — the shell is
# `cd`'d into tools/bisection by this point, which is not what a
# caller means by a relative path.
case "$SYSTEM" in
    ''|/*) ;;
    *) SYSTEM="$REPO_ROOT/$SYSTEM" ;;
esac
# The capi under test is built for the cell's backend (CAPI_FEATURE), so
# the data must be that backend's: all three DBMs use libpinyin's own
# file names, so the names alone cannot tell a Kyoto Cabinet directory
# from a Berkeley DB one — the backend check below does.
missing=()
for t in pinyin_index.bin phrase_index.bin bigram.db interpolation2.text; do
    [[ -n "$SYSTEM" && -f "$SYSTEM/$t" ]] || missing+=("$t")
done
if ((${#missing[@]})); then
    echo "SKIP: PHRASE_SYSTEM must name a $ORACLE_DBM_NAME system data dir"
    echo "  missing: ${missing[*]}"
    exit 3
fi
if ! data_ext=$(system_dir_data_ext "$SYSTEM"); then
    echo "FAIL: cannot tell which backend wrote PHRASE_SYSTEM=$SYSTEM"
    echo "  (no datagen-manifest.txt backend= and no known table.conf database format:)"
    exit 1
fi
if [[ $data_ext != "$OXPINYIN_CAPI_BACKEND_EXT" ]]; then
    echo "FAIL: BACKEND MISMATCH — the $ORACLE_DBM cell's capi is $(system_dir_ext_name "$OXPINYIN_CAPI_BACKEND_EXT"),"
    echo "  but PHRASE_SYSTEM=$SYSTEM was written for $(system_dir_ext_name "$data_ext")"
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
    echo "FAIL: phrase-surface-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    exit 1
fi
echo "oxpinyin-capi: ok"

echo "--- oracle side ---"
if ! (cd "$ORACLE_CWD" && "$DRIVER" "$ORACLE_SO" "$ORACLE_DATA") > "$ORACLE_LOG"; then
    echo "FAIL: phrase-surface-diff crashed against oracle"
    cat "$ORACLE_LOG"
    exit 1
fi
echo "oracle: ok"

echo "--- prerequisite: every probe family is active on both sides ---"
probe_surfaces() {
    grep -q '^segment|你好中国|1|n=4|' "$1" && \
        grep -q '^segment|你好，世界。|0|n=6|' "$1" && \
        grep -q '^prefix|nihaoshijie|你好|1|' "$1" && \
        grep -q '^reset|rok=1|n=0|' "$1"
}
if ! probe_surfaces "$CAPI_LOG" || ! probe_surfaces "$ORACLE_LOG"; then
    echo "FAIL: a probe family is inactive (segment / prefix-guess / reset)"
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
