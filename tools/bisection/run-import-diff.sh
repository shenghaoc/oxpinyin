#!/usr/bin/env bash
# run-import-diff.sh — W7-T1 user-import differential.
#
# Drives the identical scripted C-ABI import sequence
# (tools/bisection/import-diff.c) into libpinyin_capi.so and the pin-built
# libpinyin.so, exports both engines' user data through the W6-T7 phrase
# export iterator, and diffs the (phrase, pinyin, count) triple sets with
# exact-integer equality. This is the second half of the same value-level
# surface the W6-T7 train differential covered.
#
# Env-gated on the pin-built oracle, mirroring W6-T7: PINYIN_ORACLE_PREFIX
# (default $HOME/.local/opt/pinyin-oracle) must hold the pin-verified prefix
# from tools/oracle/build-oracle.sh. Absent -> skip with a diagnostic, exit 0.
#
# Backend cell: PINYIN_ORACLE_DBM=bdb|kc|tkrzw (default bdb; the same
# names as build-oracle.sh --dbm; tools/bisection/oracle-cell.sh). The
# capi is built with the cell's cargo feature and reads IMPORT_SYSTEM
# (default: the committed mini tables for that backend,
# fixtures/w3/<db|kct|tkt>), which must have been written by the same
# backend — another backend's directory FAILS with BACKEND MISMATCH
# (exit 1). The oracle reads its own data dir.
#
# Exit codes: 0 = identical; 77 = skipped; 1 = build/run failure (including a
# backend mismatch); 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
# shellcheck source=oracle-cell.sh
source ./oracle-cell.sh
# shellcheck source=system-dir.sh
source ./system-dir.sh

# ── Build the driver ────────────────────────────────────────────────────

echo "--- building import-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o import-diff import-diff.c -ldl
echo "build: ok"

# ── Build oxpinyin-capi ────────────────────────────────────────────────────

echo "--- building oxpinyin-capi ---"
oracle_cell_artifact OXPINYIN_CAPI_SO libpinyin_capi.so oxpinyin-capi
CAPI_SO=$OXPINYIN_CAPI_SO
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO ($CAPI_FEATURE)"

# ── Locate the pin-built oracle (env-gated) ──────────────────────────────

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 77
fi
if ! grep -q '^pin_ref=libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    echo "  expected libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99"
    exit 77
fi
# The pin_ref above is prefix-matched (the full value is composite), so pin
# the runtime inputs it folds in as explicit fields too: a prefix rebuilt
# against a different model archive or dbm backend must not import-diff as
# if it were this pin. Expected values come from build-oracle.sh, the same
# single source the workflow's drift check reads.
MODEL_SHA256_EXPECTED=$(sed -n 's/^MODEL_SHA256=//p' "$REPO_ROOT/tools/oracle/build-oracle.sh")
# The dbm field follows the backend cell (PINYIN_ORACLE_DBM, default bdb;
# tools/bisection/oracle-cell.sh, sourced above). This used to be scraped
# from a `--with-dbm=<literal>` in build-oracle.sh, which spells it
# `--with-dbm="$dbm_name"` — so the scrape matched the empty string and
# the check accepted any dbm.
DBM_EXPECTED=$ORACLE_DBM_NAME
if ! grep -q "^model_sha256=$MODEL_SHA256_EXPECTED" "$PREFIX/oracle-pin.txt" \
    || ! grep -qx "dbm=$DBM_EXPECTED" "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin (model/dbm fields)"
    echo "  expected model_sha256=$MODEL_SHA256_EXPECTED dbm=$DBM_EXPECTED"
    exit 77
fi
if [ ! -f "$ORACLE_DATA/bigram.db" ]; then
    echo "SKIP: oracle data not found at $ORACLE_DATA"
    exit 77
fi
echo "oracle: $ORACLE_SO"
echo "data:   $ORACLE_DATA"

# ── The capi's system data (per-backend) ────────────────────────────────
#
# The flat fixtures/w3 root lost its DBMs when the fixtures split into
# per-backend directories (P6 345af16d and b63b237d, on main 2026-09-03);
# from then on the capi side found no tables and this runner exited 1.
# The data is now the cell backend's own directory, and its writer is
# checked: all three DBMs share libpinyin's file names, so a directory
# written by another backend would open as the wrong container.
CAPI_SYSTEM="${IMPORT_SYSTEM:-$REPO_ROOT/fixtures/w3/$OXPINYIN_CAPI_BACKEND_EXT}"
missing=()
for t in pinyin_index.bin phrase_index.bin bigram.db gb_char.bin table.conf; do
    [ -f "$CAPI_SYSTEM/$t" ] || missing+=("$t")
done
if [ ${#missing[@]} -gt 0 ]; then
    echo "FAIL: IMPORT_SYSTEM=$CAPI_SYSTEM is not a system data directory"
    echo "  missing: ${missing[*]}"
    exit 1
fi
if ! data_ext=$(system_dir_data_ext "$CAPI_SYSTEM"); then
    echo "FAIL: cannot tell which backend wrote IMPORT_SYSTEM=$CAPI_SYSTEM"
    exit 1
fi
if [ "$data_ext" != "$OXPINYIN_CAPI_BACKEND_EXT" ]; then
    echo "FAIL: BACKEND MISMATCH — the $ORACLE_DBM cell's capi is $(system_dir_ext_name "$OXPINYIN_CAPI_BACKEND_EXT"),"
    echo "  but IMPORT_SYSTEM=$CAPI_SYSTEM was written for $(system_dir_ext_name "$data_ext")"
    exit 1
fi
echo "capi data: $CAPI_SYSTEM ($(system_dir_ext_name "$data_ext"))"
echo ""

# ── Drive both engines once ─────────────────────────────────────────────

CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
if ! ./import-diff "$CAPI_SO" "$CAPI_SYSTEM" > "$CAPI_LOG" 2> /dev/null; then
    echo "FAIL: import-diff crashed against oxpinyin-capi"
    cat "$CAPI_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi
if ! ./import-diff "$ORACLE_SO" "$ORACLE_DATA" > "$ORACLE_LOG" 2> /dev/null; then
    echo "FAIL: import-diff crashed against the oracle"
    cat "$ORACLE_LOG"
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 1
fi

echo "--- capi triples ---"
sort "$CAPI_LOG" | grep -E '^(add |empty batch|save|phrase)'
echo "--- oracle triples ---"
sort "$ORACLE_LOG" | grep -E '^(add |empty batch|save|phrase)'

if ! diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") > /dev/null; then
    echo "DIVERGENCE: import logs differ"
    diff -u <(sort "$ORACLE_LOG") <(sort "$CAPI_LOG") || true
    rm -f "$CAPI_LOG" "$ORACLE_LOG"
    exit 2
fi
rm -f "$CAPI_LOG" "$ORACLE_LOG"
echo "import-diff: IDENTICAL"
echo ""

# ── Classic interchange through the real pinned frontend ────────────────
#
# Two directions, using the actual LibPinyinBackEnd::importPinyinDictionary /
# exportPinyinDictionary methods from the pinned ibus-libpinyin build:
#   frontend(fixture) -> A ; dictool(fixture) -> B ; assert A == B
#   frontend(B)        -> C ; assert C == B
# The fixture includes a 2-field line, which both importers floor at the ABI
# default count (5).

echo "--- classic-format frontend interop ---"
FIXTURE="$REPO_ROOT/tools/bisection/classic-import.txt"

# Locate the ibus-libpinyin object tree from tools/oracle/build-oracle.sh
# (its --work-dir). Prefer the pin-build path, then the other oracle
# worktrees. build-oracle.sh names the source tree by commit SHA since
# ce70df0f (2026-09-07, #369): src/ibus-libpinyin-<sha>, formerly
# src/ibus-libpinyin-1.16.5; both are accepted.
ibus_src_of() {
    local d
    for d in "$1"/src/ibus-libpinyin-*/; do
        if ls "$d"src/*.o > /dev/null 2>&1; then
            printf '%s\n' "${d%/}"
            return 0
        fi
    done
    return 1
}
IBUS_BUILD="${PINYIN_IBUS_BUILD_DIR:-}"
if [ -z "$IBUS_BUILD" ]; then
    for candidate in         "$REPO_ROOT/target/oracle-pin-build"         "$REPO_ROOT/target/oracle-tkrzw-build"         "$REPO_ROOT/target/oracle-w2"; do
        if ibus_src_of "$candidate" > /dev/null; then
            IBUS_BUILD="$candidate"
            break
        fi
    done
fi
IBUS_SRC="$(ibus_src_of "$IBUS_BUILD" 2>/dev/null || true)"
FRONT_SRC="$IBUS_SRC/src"
SCHEMA_SRC="$IBUS_SRC/data/com.github.libpinyin.ibus-libpinyin.gschema.xml"
# The prefix the objects were linked against: the work dir's default
# prefix, else the oracle prefix under test (build-oracle.sh --prefix).
IBUS_PREFIX="${PINYIN_IBUS_PREFIX:-$IBUS_BUILD/prefix}"
[ -f "$IBUS_PREFIX/oracle-pin.txt" ] || IBUS_PREFIX="$PREFIX"

if [ -z "$IBUS_BUILD" ] || [ -z "$IBUS_SRC" ] || [ ! -f "$SCHEMA_SRC" ]; then
    echo "SKIP: ibus-libpinyin build tree not found (set PINYIN_IBUS_BUILD_DIR)"
    exit 77
fi
IBUS_PIN_REF='libpinyin-2.11.92-074a2219c90feaf962d0d24f034514033ece5f99'
if [ ! -f "$IBUS_PREFIX/oracle-pin.txt" ] || ! grep -q "^pin_ref=$IBUS_PIN_REF" "$IBUS_PREFIX/oracle-pin.txt"; then
    echo "SKIP: ibus-libpinyin build at $IBUS_BUILD is off-pin"
    exit 77
fi
for command in g++ pkg-config glib-compile-schemas; do
    command -v "$command" > /dev/null 2>&1 || {
        echo "SKIP: $command not found for frontend interop harness"
        exit 77
    }
done

# Compile the frontend harness from the same object files the pin build made.
read -r -a frontend_cflags <<< "$(pkg-config --cflags ibus-1.0 glib-2.0 gio-2.0 sqlite3)"
read -r -a frontend_libs <<< "$(pkg-config --libs ibus-1.0 glib-2.0 gio-2.0 sqlite3)"
mapfile -t frontend_objects < <(find "$FRONT_SRC" -maxdepth 1 -name '*.o' ! -name '*PYMain.o' -print | sort)
g++ -std=gnu++17 -O2 -I"$FRONT_SRC" -I"$IBUS_PREFIX/include/libpinyin-2.11.92" \
    "${frontend_cflags[@]}" frontend-import.cc "${frontend_objects[@]}" \
    "${frontend_libs[@]}" -L"$IBUS_PREFIX/lib" -lpinyin -o frontend-import

# The frontend build's schema is newer than the system GSettings database;
# compile a private copy for the harness.
SCHEMA_DIR="$(mktemp -d)"
cp "$SCHEMA_SRC" "$SCHEMA_DIR/"
glib-compile-schemas "$SCHEMA_DIR"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK" "$SCHEMA_DIR"' EXIT

run_frontend() {
    local input=$1 output=$2 cache=$3
    rm -rf "$cache"
    mkdir -p "$cache"
    if ! LD_LIBRARY_PATH="$IBUS_PREFIX/lib"         XDG_CACHE_HOME="$cache"         GSETTINGS_SCHEMA_DIR="$SCHEMA_DIR"         ./frontend-import "$input" "$output" > /dev/null 2> "$cache/stderr"; then
        echo "FAIL: pinned frontend import/export failed for $input"
        cat "$cache/stderr"
        exit 1
    fi
}

oracle_cell_artifact OXPINYIN_DICTOOL oxpinyin-dictool oxpinyin-dictool
DICTOOL=$OXPINYIN_DICTOOL

run_frontend "$FIXTURE" "$WORK/frontend-a.txt" "$WORK/frontend-cache-a"
"$DICTOOL" import --user-dir "$WORK/dictool-user" "$FIXTURE"
"$DICTOOL" export --user-dir "$WORK/dictool-user" "$WORK/dictool-b.txt"

echo "frontend(fixture):"
sort "$WORK/frontend-a.txt"
echo "dictool(fixture):"
sort "$WORK/dictool-b.txt"
if ! diff -u <(sort "$WORK/frontend-a.txt") <(sort "$WORK/dictool-b.txt") > /dev/null; then
    echo "DIVERGENCE: classic fixture export differs between frontend and dictool"
    diff -u <(sort "$WORK/frontend-a.txt") <(sort "$WORK/dictool-b.txt") || true
    exit 2
fi

run_frontend "$WORK/dictool-b.txt" "$WORK/frontend-c.txt" "$WORK/frontend-cache-c"
echo "frontend(dictool-written file):"
sort "$WORK/frontend-c.txt"
if ! diff -u <(sort "$WORK/dictool-b.txt") <(sort "$WORK/frontend-c.txt") > /dev/null; then
    echo "DIVERGENCE: dictool-written file did not round-trip through the frontend"
    diff -u <(sort "$WORK/dictool-b.txt") <(sort "$WORK/frontend-c.txt") || true
    exit 2
fi

echo "classic frontend interop: IDENTICAL (both directions)"
exit 0
