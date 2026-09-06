#!/usr/bin/env bash
# release-stage.sh — build the two shipped cdylibs under ONE store backend and
# stage the complete drop-in libpinyin/libzhuyin install tree for packaging.
#
# The build and layout come from the one supported install path,
# tools/packaging/install.sh (cargo cinstall + the complete .pc), run once
# per library with the backend selected through cargo-c's ordinary feature
# flags: `--no-default-features --features <backend>` (plus `shipped` on the
# pinyin crate). cargo-c forwards both — its subcommands register cargo's
# own feature argument set — so the release lanes select kyotocabinet on
# Fedora/Arch exactly as a distro packager would. The tree is staged under
# --dest via install.sh's --destdir, which is the DESTDIR every distro build
# uses. This script adds only what packaging needs on top: stripping (every
# distro's libpinyin ships stripped) and the gates below.
#
# Usage: release-stage.sh <backend> --prefix=DIR --libdir=DIR [--dest=DIR]
#   <backend>  kyotocabinet | tkrzw | lmdb | redb  (exactly one; the store's
#              compile_error! guards refuse any other selection)
#   --dest     staging root the prefix/libdir are written under;
#              defaults to target/release-stage/<backend>
#
# The staged tree is the COMPLETE install (runtime + dev files). The per-distro
# makers split it into runtime/dev packages the way each distro splits the real
# libpinyin (Debian: libpinyin15 + libpinyin15-dev; Fedora: libpinyin +
# libpinyin-devel; Arch: one package).
#
# Exits 0 only after every gate passes: SONAME, the five pkg-config reads real
# consumers perform, and a C compile/link/run smoke against the staged tree.

set -euo pipefail
cd "$(dirname "$0")"
REPO_ROOT="$(cd ../.. && pwd)"
PINYIN_CRATE_DIR="$REPO_ROOT/crates/oxpinyin-capi"
ZHUYIN_CRATE_DIR="$REPO_ROOT/crates/oxpinyin-zhuyin-capi"

# The companion headers both crates ship, verbatim, into the same include
# subdirectory. They must be byte-identical or staging both libraries would
# silently rewrite what the other put there (same check as install.sh).
COMPANION_HEADERS="novel_types.h pinyin_custom2.h"

# Expected `database_format` per backend; must mirror build.rs's
# database_format() and the .pc gate below catches any drift.
db_format_for() {
  case "$1" in
    kyotocabinet) echo "KyotoCabinet" ;;
    tkrzw)        echo "Tkrzw" ;;
    lmdb)         echo "LMDB" ;;
    redb)         echo "redb" ;;
    *) return 1 ;;
  esac
}

BACKEND=""
PREFIX=""
LIBDIR=""
DEST=""

usage() {
  echo "usage: $0 <kyotocabinet|tkrzw|lmdb|redb> --prefix=DIR --libdir=DIR [--dest=DIR]" >&2
  exit 2
}

case "${1:-}" in
  kyotocabinet|tkrzw|lmdb|redb) BACKEND="$1"; shift ;;
  *) usage ;;
esac

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix=*) PREFIX="${1#*=}" ;;
    --prefix)   shift; PREFIX="${1:-}" ;;
    --libdir=*) LIBDIR="${1#*=}" ;;
    --libdir)   shift; LIBDIR="${1:-}" ;;
    --dest=*)   DEST="${1#*=}" ;;
    --dest)     shift; DEST="${1:-}" ;;
    *) usage ;;
  esac
  shift || true
done

[ -n "$PREFIX" ] && [ -n "$LIBDIR" ] || usage
DEST="${DEST:-$REPO_ROOT/target/release-stage/$BACKEND}"
# The staging root must never be one of the real install roots: packaging
# writes into it and then installs via dpkg/dnf/pacman, never in place.
if [ "$DEST" = "/" ] || [ "$DEST" = "$PREFIX" ] || [ "$DEST" = "$LIBDIR" ]; then
  echo "error: --dest '$DEST' collides with a real install root" >&2
  exit 2
fi

DB_FORMAT="$(db_format_for "$BACKEND")"

# cmp (diffutils) is not in every minimal container base; its absence must
# not read as "headers differ".
command -v cmp >/dev/null 2>&1 || {
  echo "error: cmp (diffutils) is required for the companion-header check" >&2
  exit 1
}

# Pre-build guard: identical companion headers in both crates.
for name in $COMPANION_HEADERS; do
  if ! cmp -s -- "$PINYIN_CRATE_DIR/$name" "$ZHUYIN_CRATE_DIR/$name"; then
    echo "error: companion header '$name' differs between the two crates" >&2
    exit 1
  fi
done

command -v cargo-cinstall >/dev/null 2>&1 || {
  echo "error: cargo-c (cargo cinstall) is required; install the distro's cargo-c package" >&2
  exit 1
}

# --- build + stage via install.sh ------------------------------------------
#
# One library per invocation, as install.sh requires. `shipped` compiles out
# the fixture hooks no real consumer calls and exists only on the pinyin
# crate. --locked keeps the release lanes on the committed Cargo.lock.
rm -rf -- "$DEST"
echo "== installing libpinyin into $DEST (backend: $BACKEND, features: shipped) =="
./install.sh libpinyin --prefix="$PREFIX" --libdir="$LIBDIR" --destdir="$DEST" -- \
  --release --locked --no-default-features --features "$BACKEND,shipped"
echo "== installing libzhuyin into $DEST (backend: $BACKEND) =="
./install.sh libzhuyin --prefix="$PREFIX" --libdir="$LIBDIR" --destdir="$DEST" -- \
  --release --locked --no-default-features --features "$BACKEND"

STAGE_LIB="$DEST$LIBDIR"
STAGE_INC="$DEST$PREFIX/include/libpinyin-2.11.91"
STAGE_PC="$STAGE_LIB/pkgconfig"

# The fixed tree of docs/findings/installed-naming.md, file by file: a
# missing piece here means cargo-c's layout moved and the packagers' file
# lists would ship a hole.
for f in "$STAGE_LIB/libpinyin.so.15.0.0" "$STAGE_LIB/libpinyin.so.15" \
         "$STAGE_LIB/libpinyin.so" "$STAGE_LIB/libpinyin.a" \
         "$STAGE_LIB/libzhuyin.so.15.0.0" "$STAGE_LIB/libzhuyin.so.15" \
         "$STAGE_LIB/libzhuyin.so" "$STAGE_LIB/libzhuyin.a" \
         "$STAGE_PC/libpinyin.pc" "$STAGE_PC/libzhuyin.pc" \
         "$STAGE_INC/pinyin.h" "$STAGE_INC/zhuyin.h" \
         "$STAGE_INC/novel_types.h" "$STAGE_INC/pinyin_custom2.h" \
         "$STAGE_INC/zhuyin_custom2.h"; do
  [ -e "$f" ] || { echo "error: expected staged file $f is missing" >&2; exit 1; }
done
# Nothing beyond that tree may ride along (cargo-c would happily add e.g. a
# bin/ or share/ if a crate grew one).
unexpected="$(find "$DEST" -type f ! -path "$STAGE_LIB/*" ! -path "$STAGE_INC/*")"
[ -z "$unexpected" ] || { echo "error: unexpected staged files:" >&2; echo "$unexpected" >&2; exit 1; }

# Release artifacts ship stripped, as every distro's libpinyin does.
if command -v strip >/dev/null 2>&1; then
  strip --strip-unneeded -- "$STAGE_LIB/libpinyin.so.15.0.0" "$STAGE_LIB/libzhuyin.so.15.0.0"
fi
chmod 0755 -- "$STAGE_LIB/libpinyin.so.15.0.0" "$STAGE_LIB/libzhuyin.so.15.0.0"
chmod 0644 -- "$STAGE_LIB"/*.a "$STAGE_PC"/*.pc "$STAGE_INC"/*.h

# --- gates ----------------------------------------------------------------

fail() { echo "GATE FAILED: $*" >&2; exit 1; }

# SONAME: the string every consumer's DT_NEEDED records.
for lib in libpinyin libzhuyin; do
  soname="$(readelf -d "$STAGE_LIB/$lib.so.15.0.0" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p')"
  [ "$soname" = "$lib.so.15" ] || fail "$lib SONAME is '$soname', want '$lib.so.15'"
done

# The pkg-config reads real consumers perform (fcitx-libpinyin's cmake probes
# among them — docs/findings/installed-naming.md):
export PKG_CONFIG_PATH="$STAGE_PC"
check_pc() { # $1=module $2=what $3=expected
  got="$(pkg-config --variable="$2" "$1")"
  [ "$got" = "$3" ] || fail "$1 $2 is '$got', want '$3'"
}
check_pc libpinyin database_format "$DB_FORMAT"
check_pc libzhuyin database_format "$DB_FORMAT"
check_pc libpinyin libpinyin_binary_version "15.0"
check_pc libzhuyin libzhuyin_binary_version "15.0"
for mod in libpinyin libzhuyin; do
  [ "$(pkg-config --modversion "$mod")" = "2.11.91" ] \
    || fail "$mod modversion is not 2.11.91"
  pkg-config --libs "$mod" | grep -qw -- "-l${mod#lib}" \
    || fail "$mod --libs does not contain -l${mod#lib}"
  pkg-config --cflags "$mod" | grep -q -- "libpinyin-2.11.91" \
    || fail "$mod --cflags does not point into libpinyin-2.11.91"
done
# pkgdatadir exists on libpinyin.pc only — upstream's libzhuyin.pc carries no
# data-dir variable, and mirroring upstream byte-for-byte is the contract.
[ -n "$(pkg-config --variable=pkgdatadir libpinyin)" ] \
  || fail "libpinyin pkgdatadir is empty (the silent-misconfiguration gap)"

# C smoke: two programs, each compiling one public header and linking
# exactly one library — the real consumer pattern (ibus-libpinyin links
# libpinyin only; ibus-libzhuyin links libzhuyin only). <stdbool.h> comes
# first because upstream's headers (mirrored byte-for-byte) use C99 bool
# without including it — real consumers are C++ — and no program can link
# both headers: they re-declare the same enums, and novel_types.h carries
# tentative definitions (null_token, …) that duplicate at link time. The
# staged include/lib dirs are passed explicitly: with a real --prefix the
# .pc's own -I/-L point at system paths that do not exist yet, while
# pkg-config still supplies the system-side Requires (glib-2.0). pinyin_init
# may return NULL in a data-less environment; only the dynamic resolution
# and the no-panic discipline are under test here.
SMOKE="$(mktemp -d)"
trap 'rm -rf -- "$SMOKE"' EXIT
cat > "$SMOKE/smoke_pinyin.c" <<'EOF'
#include <stdbool.h>
#include <pinyin.h>
#include <stdio.h>

int main(void) {
  pinyin_context_t *c = pinyin_init("", "");
  printf("pinyin=%p\n", (void *)c);
  return 0;
}
EOF
cat > "$SMOKE/smoke_zhuyin.c" <<'EOF'
#include <stdbool.h>
#include <zhuyin.h>
#include <stdio.h>

int main(void) {
  zhuyin_context_t *c = zhuyin_init("", "");
  printf("zhuyin=%p\n", (void *)c);
  return 0;
}
EOF
# cflags/libs must word-split into argv.
# shellcheck disable=SC2046
cc "$SMOKE/smoke_pinyin.c" -o "$SMOKE/smoke_pinyin" \
  -I"$STAGE_INC" -L"$STAGE_LIB" $(pkg-config --cflags --libs libpinyin) \
  || fail "libpinyin smoke compile/link against the staged tree failed"
# shellcheck disable=SC2046
cc "$SMOKE/smoke_zhuyin.c" -o "$SMOKE/smoke_zhuyin" \
  -I"$STAGE_INC" -L"$STAGE_LIB" $(pkg-config --cflags --libs libzhuyin) \
  || fail "libzhuyin smoke compile/link against the staged tree failed"
LD_LIBRARY_PATH="$STAGE_LIB" "$SMOKE/smoke_pinyin" || fail "libpinyin smoke run failed"
LD_LIBRARY_PATH="$STAGE_LIB" "$SMOKE/smoke_zhuyin" || fail "libzhuyin smoke run failed"

echo "== staged tree under $DEST =="
find "$DEST" -mindepth 1 -printf '%y %P\n' | sort
echo "== release-stage gates passed (backend: $BACKEND) =="
