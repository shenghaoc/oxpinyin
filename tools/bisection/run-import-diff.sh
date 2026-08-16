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
# Exit codes: 0 = identical or skipped; 1 = build/run failure; 2 = divergence.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

# ── Build the driver ────────────────────────────────────────────────────

echo "--- building import-diff driver ---"
gcc -std=gnu11 -Wall -Wextra -Werror -O2 -o import-diff import-diff.c -ldl
echo "build: ok"

# ── Build oxpinyin-capi ────────────────────────────────────────────────────

echo "--- building oxpinyin-capi ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_SO="$REPO_ROOT/target/debug/libpinyin_capi.so"
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi
echo "capi: $CAPI_SO"

# ── Locate the pin-built oracle (env-gated) ──────────────────────────────

PREFIX="${PINYIN_ORACLE_PREFIX:-$HOME/.local/opt/pinyin-oracle}"
ORACLE_SO="$PREFIX/lib/libpinyin.so"
ORACLE_DATA="$PREFIX/lib/libpinyin/data"

if [ ! -f "$PREFIX/oracle-pin.txt" ] || [ ! -f "$ORACLE_SO" ]; then
    echo "SKIP: pin-built oracle not found at $PREFIX"
    echo "  build it with tools/oracle/build-oracle.sh and set PINYIN_ORACLE_PREFIX"
    exit 0
fi
if ! grep -q '^pin_ref=libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c' \
    "$PREFIX/oracle-pin.txt"; then
    echo "SKIP: oracle prefix at $PREFIX is off-pin"
    echo "  expected libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c"
    exit 0
fi
if [ ! -f "$ORACLE_DATA/bigram.db" ]; then
    echo "SKIP: oracle data not found at $ORACLE_DATA"
    exit 0
fi
echo "oracle: $ORACLE_SO"
echo "data:   $ORACLE_DATA"
echo ""

# ── Drive both engines once ─────────────────────────────────────────────

CAPI_LOG="$(mktemp)"
ORACLE_LOG="$(mktemp)"
if ! ./import-diff "$CAPI_SO" "$REPO_ROOT/fixtures/w3" > "$CAPI_LOG" 2> /dev/null; then
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

# Locate the ibus-libpinyin object tree from tools/oracle/build-oracle.sh.
# Prefer the pin-build path, then the other oracle worktrees.
IBUS_BUILD="${PINYIN_IBUS_BUILD_DIR:-}"
if [ -z "$IBUS_BUILD" ]; then
    for candidate in         "$REPO_ROOT/target/oracle-pin-build"         "$REPO_ROOT/target/oracle-tkrzw-build"         "$REPO_ROOT/target/oracle-w2"; do
        if ls "$candidate/src/ibus-libpinyin-1.16.5/src/"*.o > /dev/null 2>&1; then
            IBUS_BUILD="$candidate"
            break
        fi
    done
fi
FRONT_SRC="$IBUS_BUILD/src/ibus-libpinyin-1.16.5/src"
SCHEMA_SRC="$IBUS_BUILD/src/ibus-libpinyin-1.16.5/data/com.github.libpinyin.ibus-libpinyin.gschema.xml"
IBUS_PREFIX="$IBUS_BUILD/prefix"

if [ -z "$IBUS_BUILD" ] || ! ls "$FRONT_SRC/"*.o > /dev/null 2>&1 || [ ! -f "$SCHEMA_SRC" ]; then
    echo "SKIP: ibus-libpinyin build tree not found (set PINYIN_IBUS_BUILD_DIR)"
    exit 0
fi
IBUS_PIN_REF='libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c'
if [ ! -f "$IBUS_PREFIX/oracle-pin.txt" ] || ! grep -q "^pin_ref=$IBUS_PIN_REF" "$IBUS_PREFIX/oracle-pin.txt"; then
    echo "SKIP: ibus-libpinyin build at $IBUS_BUILD is off-pin"
    exit 0
fi
for command in g++ pkg-config glib-compile-schemas; do
    command -v "$command" > /dev/null 2>&1 || {
        echo "SKIP: $command not found for frontend interop harness"
        exit 0
    }
done

# Compile the frontend harness from the same object files the pin build made.
g++ -std=gnu++17 -O2     -I"$FRONT_SRC"     -I"$IBUS_PREFIX/include/libpinyin-2.11.91"     $(pkg-config --cflags ibus-1.0 glib-2.0 gio-2.0 sqlite3)     frontend-import.cc     $(find "$FRONT_SRC" -maxdepth 1 -name '*.o' ! -name '*PYMain.o' -print | sort)     $(pkg-config --libs ibus-1.0 glib-2.0 gio-2.0 sqlite3)     -L"$IBUS_PREFIX/lib" -lpinyin     -o frontend-import

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

DICTOOL="$REPO_ROOT/target/debug/oxpinyin-dictool"
cargo build -p oxpinyin-dictool --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1

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
