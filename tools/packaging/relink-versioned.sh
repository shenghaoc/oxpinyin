#!/usr/bin/env bash
# relink-versioned.sh — relink one drop-in shared object from its staticlib
# under upstream's ELF symbol versioning.
#
# The cdylib cargo builds cannot carry the version definitions: rustc always
# emits its own anonymous version script for a cdylib, and a second, named
# script cannot ride the same link (GNU ld rejects the pair; rust-lld produces
# the version-definitions-without-versioned-symbols shape that hard-fails at
# load) — docs/findings/drop-in-abi-identity.md §2. Linking the shared object
# from the staticlib here, where rustc's link line is not in play, produces
# upstream's shape instead: every exported symbol versioned
# (`pinyin_init@@LIBPINYIN`), so consumers linked against the real
# libpinyin/libzhuyin load with no `no version information available` warning.
#
# The symbol set is verified against the crate's .ver file in BOTH directions
# (no missing export, no leak past `local: *;`) before anything is installed,
# and the SONAME is set to the drop-in name.
#
# Usage: relink-versioned.sh --staticlib PATH --ver PATH --soname NAME
#                             --dest PATH [--features LIST] [--cc CC]
#   --staticlib  the cargo staticlib artifact (e.g. …/release/libpinyin_capi.a)
#   --ver        the upstream version script copied verbatim at the pin
#   --soname     the drop-in SONAME (libpinyin.so.15 / libzhuyin.so.15)
#   --dest       the REAL file to replace (e.g. …/lib/libpinyin.so.15.0.0);
#               the .so and .so.<major> symlinks cargo-c installed next to it
#               keep resolving (same file name), and are asserted to
#   --features  the cargo feature list of the build that produced the
#               staticlib, for the store-backend link flags (same precedence
#               as the crates: tkrzw > kyotocabinet > bdb;
#               default tkrzw, the workspace default)
#
# Linux + GNU ld only: the ELF versioning this implements has no macOS
# counterpart (the cdylib as built is correct there). Exits 0 on success,
# non-zero on any failed link or verification.

set -euo pipefail

STATICLIB=""
VER=""
SONAME=""
DEST=""
FEATURES="${OXPINYIN_RELINK_FEATURES:-}"
CC_CMD="${CC:-cc}"

usage() {
  echo "usage: $0 --staticlib PATH --ver PATH --soname NAME --dest PATH [--features LIST] [--cc CC]" >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --staticlib=*) STATICLIB="${1#*=}" ;;
    --staticlib)   shift; STATICLIB="${1:-}" ;;
    --ver=*)       VER="${1#*=}" ;;
    --ver)         shift; VER="${1:-}" ;;
    --soname=*)    SONAME="${1#*=}" ;;
    --soname)      shift; SONAME="${1:-}" ;;
    --dest=*)      DEST="${1#*=}" ;;
    --dest)        shift; DEST="${1:-}" ;;
    --features=*)  FEATURES="${1#*=}" ;;
    --features)    shift; FEATURES="${1:-}" ;;
    --cc=*)        CC_CMD="${1#*=}" ;;
    --cc)          shift; CC_CMD="${1:-}" ;;
    *) usage ;;
  esac
  shift || true
done

[ -n "$STATICLIB" ] && [ -n "$VER" ] && [ -n "$SONAME" ] && [ -n "$DEST" ] || usage

if [ "$(uname -s)" != "Linux" ]; then
  echo "error: relink-versioned.sh is Linux-only (ELF symbol versioning); run it in the Linux packaging lane" >&2
  exit 1
fi

for tool in "$CC_CMD" nm readelf; do
  command -v "$tool" >/dev/null 2>&1 || {
    echo "error: required tool '$tool' not found" >&2
    exit 1
  }
done

[ -f "$STATICLIB" ] || { echo "error: staticlib not found: $STATICLIB" >&2; exit 1; }
[ -f "$VER" ] || { echo "error: version script not found: $VER" >&2; exit 1; }
[ -f "$DEST" ] || { echo "error: installed object not found: $DEST" >&2; exit 1; }

# The store backend the staticlib was built with, from the feature list —
# the same precedence the crates' build.rs uses. Every backend
# leaves C-library references in the staticlib that the shared-object link
# must resolve; prefer pkg-config (it drags the backend's own dependencies,
# e.g. tkrzw's -llzma) and fall back to the plain -l name build.rs uses when
# pkg-config cannot help (libdb ships no .pc on Debian).
BACKEND="tkrzw"
for feature in $(echo "$FEATURES" | tr ',' ' '); do
  case "$feature" in
    tkrzw|kyotocabinet|bdb) BACKEND="$feature" ;;
  esac
done

backend_libs() {
  case "$1" in
    tkrzw)         pkg-config --libs tkrzw 2>/dev/null || echo -ltkrzw ;;
    kyotocabinet)  pkg-config --libs kyotocabinet 2>/dev/null || echo -lkyotocabinet ;;
    bdb)           echo -ldb ;;
  esac
}

# glib is an unconditional dependency of the C ABI (the export surface hands
# out glib-allocated strings); resolve it through pkg-config the way the
# glib-sys crate does.
GLIB_LIBS="$(pkg-config --libs glib-2.0 gobject-2.0 2>/dev/null || echo '-lglib-2.0 -lgobject-2.0')"

BACKEND_LIBS="$(backend_libs "$BACKEND")"

TMP_DEST="$(mktemp "${DEST%.so*}.relink.XXXXXX")"
trap 'rm -f "$TMP_DEST"' EXIT

# shellcheck disable=SC2086
"$CC_CMD" -shared -o "$TMP_DEST" \
  -Wl,-soname,"$SONAME" \
  -Wl,--version-script="$VER" \
  -Wl,--whole-archive "$STATICLIB" -Wl,--no-whole-archive \
  $GLIB_LIBS $BACKEND_LIBS \
  -lpthread -ldl -lm

# ── Verification, both directions, before anything is installed ────────────

# The version node the script defines (its first non-empty word).
VER_NODE="$(sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\) {.*/\1/p' "$VER" | head -1)"
[ -n "$VER_NODE" ] || { echo "error: no version node found in $VER" >&2; exit 1; }

# The .ver global list — one symbol per line.
VER_SYMBOLS="$(mktemp)"; trap 'rm -f "$TMP_DEST" "$VER_SYMBOLS"' EXIT
sed -n '/global:/,/local:/p' "$VER" | sed -e 's/global://' -e 's/local://' \
  | tr ';' '\n' | sed -e 's/[[:space:]]//g' -e '/^$/d' | sort > "$VER_SYMBOLS"

# The object's defined dynamic symbols, version tag stripped. The version
# node itself appears as a defined dynsym entry (GNU ld's verdef carrier —
# upstream's own .so shows it too), so it is expected and filtered.
SO_SYMBOLS="$(mktemp)"; trap 'rm -f "$TMP_DEST" "$VER_SYMBOLS" "$SO_SYMBOLS"' EXIT
nm -D --defined-only "$TMP_DEST" | awk '{print $NF}' | sed 's/@.*//g' \
  | grep -v -x "$VER_NODE" | sort > "$SO_SYMBOLS"

if ! diff -u "$VER_SYMBOLS" "$SO_SYMBOLS" > /dev/null; then
  echo "error: relinked object's export set differs from $VER:" >&2
  diff -u "$VER_SYMBOLS" "$SO_SYMBOLS" | sed -n '3,40p' >&2
  exit 1
fi

# Every export except the node carrier must carry the version tag, and the
# SONAME must be the drop-in name — the two properties the cdylib could not
# be given.
UNVERSIONED="$(nm -D --defined-only "$TMP_DEST" | awk -v node="$VER_NODE" '$NF !~ /@/ && $NF != node {print $NF}')"
[ -z "$UNVERSIONED" ] || {
  echo "error: exported symbols without a version tag:" >&2
  printf '  %s\n' $UNVERSIONED >&2
  exit 1
}
ACTUAL_SONAME="$(readelf -d "$TMP_DEST" | awk '/SONAME/ {gsub(/[\[\]]/, ""); print $NF}' | head -1)"
[ "$ACTUAL_SONAME" = "$SONAME" ] || {
  echo "error: relinked SONAME is '${ACTUAL_SONAME:-<none>}', expected $SONAME" >&2
  exit 1
}

# Install: replace the real file; the .so / .so.<major> symlinks cargo-c put
# next to it still resolve (same file name).
mv -f "$TMP_DEST" "$DEST"
trap 'rm -f "$VER_SYMBOLS" "$SO_SYMBOLS"' EXIT

echo "relinked $DEST"
echo "  soname: $SONAME · version node: $VER_NODE · exports: $(wc -l < "$VER_SYMBOLS" | tr -d ' ') versioned (backend: $BACKEND)"
