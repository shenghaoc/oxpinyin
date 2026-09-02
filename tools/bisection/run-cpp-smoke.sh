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

CAPI_DIR="$REPO_ROOT/target/debug"
CAPI_SO="$CAPI_DIR/libpinyin_capi.so"
echo "--- building oxpinyin-capi for the C++ smoke gate ---"
cargo build -p oxpinyin-capi --locked --manifest-path "$REPO_ROOT/Cargo.toml" 2>&1
if [ ! -f "$CAPI_SO" ]; then
    echo "fatal: $CAPI_SO not found"
    exit 1
fi

FIX_ROOT="$REPO_ROOT/fixtures/w3"
# The committed W3 fixture holds one per-backend data directory each
# (fixtures/w3/<kct|redb|tkt|lmdb>), a real drop-in dir with libpinyin's
# own file names. OXPINYIN_CAPI_BACKEND_EXT pins the backend for capi
# builds that select one explicitly (e.g. --features tkrzw); unset, this
# gate builds the default (Kyoto Cabinet) and picks the first committed
# backend directory, preferring the default's.
if [ -n "${OXPINYIN_CAPI_BACKEND_EXT:-}" ]; then
    case "$OXPINYIN_CAPI_BACKEND_EXT" in
        kct|redb|tkt|lmdb) SYS_EXT=$OXPINYIN_CAPI_BACKEND_EXT ;;
        *)
            echo "fatal: OXPINYIN_CAPI_BACKEND_EXT='$OXPINYIN_CAPI_BACKEND_EXT' is not one of: kct redb tkt lmdb"
            exit 1
            ;;
    esac
    if [ ! -d "$FIX_ROOT/$SYS_EXT" ]; then
        echo "fatal: no $FIX_ROOT/$SYS_EXT directory for the requested backend"
        exit 1
    fi
else
    SYS_EXT=""
    for ext in kct redb tkt lmdb; do
        if [ -d "$FIX_ROOT/$ext" ]; then
            SYS_EXT=$ext
            break
        fi
    done
    if [ -z "$SYS_EXT" ]; then
        echo "fatal: no per-backend fixture directory under $FIX_ROOT and no OXPINYIN_CAPI_BACKEND_EXT override"
        exit 1
    fi
fi
CAPI_DATA="$FIX_ROOT/$SYS_EXT"

BUILD_DIR="$(mktemp -d)"
trap 'rm -rf "$BUILD_DIR"' EXIT
USER_DIR="$BUILD_DIR/user"
mkdir "$USER_DIR"

# Public pinyin_init requires a parsable interpolation2.text. The committed
# W3 tables have none; copy them and write a stub so the smoke exercises
# the public ABI rather than the fixture constructor.
SYS_DIR="$BUILD_DIR/sys"
mkdir "$SYS_DIR"
cp "$CAPI_DATA"/* "$SYS_DIR/"
printf '%s\n' '\data model interpolation' '\1-gram' '\item 1 ok count 1' \
    > "$SYS_DIR/interpolation2.text"

# The built object carries SONAME libpinyin.so.15 (the drop-in identity), so
# anything that LINKS against it records that name in DT_NEEDED rather than
# libpinyin_capi.so. Without a matching file on the search path the smoke
# binary would silently bind to a system libpinyin if one is installed --
# which is how this gate first caught the change. The symlink makes rpath
# resolve the SONAME to the build under test, so the gate exercises the
# drop-in identity instead of working around it.
ln -sf libpinyin_capi.so "$CAPI_DIR/libpinyin.so.15"

echo "--- compiling C++ smoke TU against pinyin.h ---"
# pinyin.h -> novel_types.h/pinyin_custom2.h -> <glib.h>, and libpinyin.pc
# declares `Requires: glib-2.0`. Supply the glib include dirs (or an explicit
# GLIB_CFLAGS override for constrained builders) so the header tuple resolves.
GLIB_CFLAGS="${GLIB_CFLAGS:-$(pkg-config --cflags glib-2.0 2>/dev/null)}"
g++ -std=c++17 -Wall -Wextra -Werror -O2 \
    $GLIB_CFLAGS \
    -I"$REPO_ROOT/crates/oxpinyin-capi" \
    cpp-smoke.cc \
    -L"$CAPI_DIR" -Wl,-rpath,"$CAPI_DIR" \
    -lpinyin_capi \
    -o "$BUILD_DIR/cpp-smoke"

needed=$(readelf -d "$BUILD_DIR/cpp-smoke" | sed -n 's/.*Shared library: \[\(libpinyin[^]]*\)\].*/\1/p')
if [ "$needed" != "libpinyin.so.15" ]; then
    echo "fatal: smoke binary needs '$needed', expected libpinyin.so.15"
    exit 1
fi
resolved=$(LD_LIBRARY_PATH= ldd "$BUILD_DIR/cpp-smoke" | sed -n 's/.*libpinyin\.so\.15 => \([^ ]*\).*/\1/p')
case "$resolved" in
    "$CAPI_DIR"/*) ;;
    *) echo "fatal: libpinyin.so.15 resolved to '$resolved', not the build under test"; exit 1 ;;
esac

echo "--- running C++ smoke TU ---"
"$BUILD_DIR/cpp-smoke" "$SYS_DIR" "$USER_DIR"
echo "cpp-smoke: ok"
