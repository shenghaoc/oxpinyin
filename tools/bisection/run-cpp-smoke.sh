#!/usr/bin/env bash
# run-cpp-smoke.sh — compile/run the C++ header-compatibility gate.
#
# The fork consumes pinyin.h from C++ translation units, so the header must
# carry `extern "C"` guards and every typedef/constant the fork references.
# This builds oxpinyin-capi, compiles cpp-smoke.cc with g++ against the
# checked-in generated header, links libpinyin_capi.so, and runs the binary
# against the committed W3 tables.
#
# Exits 0 on success, 1 on build/run failure.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"

echo "--- building oxpinyin-capi for the C++ smoke gate ---"
cargo build -p oxpinyin-capi --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
CAPI_DIR="$REPO_ROOT/target/debug"
CAPI_SO="$CAPI_DIR/libpinyin_capi.so"
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

CAPI_DATA="$REPO_ROOT/fixtures/w3"
if [ ! -f "$CAPI_DATA/pinyin_index.redb" ]; then
    echo "fatal: redb tables not found at $CAPI_DATA"
    exit 1
fi

BUILD_DIR="$(mktemp -d)"
trap 'rm -rf "$BUILD_DIR"' EXIT
USER_DIR="$BUILD_DIR/user"
mkdir "$USER_DIR"

echo "--- compiling C++ smoke TU against pinyin.h ---"
g++ -std=c++17 -Wall -Wextra -Werror -O2 \
    -I"$REPO_ROOT/crates/oxpinyin-capi" \
    cpp-smoke.cc \
    -L"$CAPI_DIR" -Wl,-rpath,"$CAPI_DIR" \
    -lpinyin_capi \
    -o "$BUILD_DIR/cpp-smoke"

echo "--- running C++ smoke TU ---"
"$BUILD_DIR/cpp-smoke" "$CAPI_DATA" "$USER_DIR"
echo "cpp-smoke: ok"
