#!/usr/bin/env bash
# release-stage.sh — build the two shipped cdylibs under ONE store backend and
# stage the complete drop-in libpinyin/libzhuyin install tree for packaging.
#
# Why plain `cargo build` and not `cargo cinstall` (tools/packaging/install.sh):
# the release lanes select NON-default store backends (kyotocabinet on
# Fedora/Arch, whose distro data is KC-format), and cargo-c does not forward
# --no-default-features to cargo (verified against cargo-c 0.10.24; see the
# `shipped` feature note in crates/oxpinyin-capi/Cargo.toml), so only the
# workspace-default backend can be selected through it. Nothing else of
# cargo-c's output is lost by building directly:
#   - the SONAMEs (libpinyin.so.15 / libzhuyin.so.15) are stamped by each
#     crate's build.rs as cdylib link args, not by cargo-c;
#   - the complete .pc files come from the same build.rs-baked templates
#     install.sh consumes, with the same install-time substitutions;
#   - the install layout is the fixed one verified against Ubuntu's
#     libpinyin15-dev in docs/findings/installed-naming.md.
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

# Build both cdylibs under exactly one backend. `shipped` compiles out the
# fixture hooks no real consumer calls and exists only on the pinyin crate.
# --locked keeps the release lanes on the committed Cargo.lock.
echo "== building oxpinyin-capi (backend: $BACKEND, features: shipped) =="
cargo build --release --locked -p oxpinyin-capi \
  --no-default-features --features "$BACKEND,shipped"
echo "== building oxpinyin-zhuyin-capi (backend: $BACKEND) =="
cargo build --release --locked -p oxpinyin-zhuyin-capi \
  --no-default-features --features "$BACKEND"

TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
PROFILE_DIR="$TARGET_DIR/release"

PINYIN_SO="$PROFILE_DIR/libpinyin_capi.so"
PINYIN_A="$PROFILE_DIR/libpinyin_capi.a"
ZHUYIN_SO="$PROFILE_DIR/libzhuyin_capi.so"
ZHUYIN_A="$PROFILE_DIR/libzhuyin_capi.a"
PINYIN_PC_BAKED="$PROFILE_DIR/libpinyin.pc.in.baked"
ZHUYIN_PC_BAKED="$PROFILE_DIR/libzhuyin.pc.in.baked"

for f in "$PINYIN_SO" "$PINYIN_A" "$ZHUYIN_SO" "$ZHUYIN_A" \
         "$PINYIN_PC_BAKED" "$ZHUYIN_PC_BAKED"; do
  if [ ! -f "$f" ]; then
    echo "error: expected build artifact $f is missing" >&2
    exit 1
  fi
done

# --- stage ---------------------------------------------------------------

rm -rf -- "$DEST"
STAGE_LIB="$DEST$LIBDIR"
STAGE_INC="$DEST$PREFIX/include/libpinyin-2.11.91"
STAGE_PC="$STAGE_LIB/pkgconfig"
mkdir -p -- "$STAGE_LIB" "$STAGE_INC" "$STAGE_PC"

install_so() { # $1=source $2=dest base name (libpinyin / libzhuyin)
  install -m0755 -- "$1" "$STAGE_LIB/$2.so.15.0.0"
  # Release artifacts ship stripped, as every distro's libpinyin does.
  if command -v strip >/dev/null 2>&1; then
    strip --strip-unneeded -- "$STAGE_LIB/$2.so.15.0.0"
  fi
  ln -sfn -- "$2.so.15.0.0" "$STAGE_LIB/$2.so.15"
  ln -sfn -- "$2.so.15" "$STAGE_LIB/$2.so"
}

install_so "$PINYIN_SO" libpinyin
install_so "$ZHUYIN_SO" libzhuyin
install -m0644 -- "$PINYIN_A" "$STAGE_LIB/libpinyin.a"
install -m0644 -- "$ZHUYIN_A" "$STAGE_LIB/libzhuyin.a"

# The closed header set each crate's Cargo.toml declares as include assets.
for h in pinyin.h novel_types.h pinyin_custom2.h; do
  install -m0644 -- "$PINYIN_CRATE_DIR/$h" "$STAGE_INC/$h"
done
for h in zhuyin.h zhuyin_custom2.h novel_types.h pinyin_custom2.h; do
  install -m0644 -- "$ZHUYIN_CRATE_DIR/$h" "$STAGE_INC/$h"
done

# Escape a value for the sed replacement side (same helper contract as
# install.sh): '\', '&', and the '#' delimiter substitute literally.
sed_escape() {
  printf '%s' "$1" | sed 's/[\\&#]/\\&/g'
}

prefix_esc="$(sed_escape "$PREFIX")"
libdir_esc="$(sed_escape "$LIBDIR")"

# libpinyin: the template hardcodes exec_prefix/includedir off ${prefix}, so
# only @prefix@ and @libdir@ are install-time (mirrors install.sh exactly).
sed -e "s#@prefix@#${prefix_esc}#g" -e "s#@libdir@#${libdir_esc}#g" \
  "$PINYIN_PC_BAKED" > "$STAGE_PC/libpinyin.pc"

# libzhuyin: all four placeholders are install-time; @exec_prefix@ and
# @includedir@ get the symbolic values autoconf substitutes upstream.
sed -e "s#@prefix@#${prefix_esc}#g" \
  -e "s#@exec_prefix@#\${prefix}#g" \
  -e "s#@libdir@#${libdir_esc}#g" \
  -e "s#@includedir@#\${prefix}/include#g" \
  "$ZHUYIN_PC_BAKED" > "$STAGE_PC/libzhuyin.pc"
chmod 0644 -- "$STAGE_PC/libpinyin.pc" "$STAGE_PC/libzhuyin.pc"

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
