#!/usr/bin/env bash
# install.sh — install the drop-in libpinyin tree with a COMPLETE libpinyin.pc.
#
# Why this wrapper exists (verified against cargo-c 0.10.24 source):
#   1. cargo-c's [package.metadata.capi.pkg_config] has a closed 7-key field
#      set (build.rs:358); it cannot emit the four variables real consumers
#      read — pkgdatadir, database_format, libpinyinincludedir,
#      libpinyin_binary_version — and offers no `generate = false` to opt out.
#   2. cargo-c exposes no install-prefix env var to build scripts (its only
#      set_var calls are CARGO_C_CARGO and INLINE_C_RS_CFLAGS), so build.rs
#      cannot know the --prefix.
#   3. cargo-c installs build-generated data assets under datadir, never the
#      pkgconfig dir (install.rs:233-236), and always installs its own .pc
#      into <libdir>/pkgconfig first (install.rs:216-220).
#
# So build.rs bakes the build-time fields into libpinyin.pc.in.baked (version,
# ABI binary version, database_format), and this wrapper runs `cargo cinstall`,
# fills the install-time @prefix@/@libdir@, and OVERWRITES the incomplete .pc
# cargo-c installed. This wrapper is the ONLY supported path to a correct .pc;
# a plain `cargo capi install` leaves cargo-c's incomplete one — the documented
# silent window (docs/findings/installed-naming.md).
#
# Usage: tools/packaging/install.sh --prefix=DIR [--libdir=DIR] [-- <extra cargo cinstall args>]
# Env:   LIBPINYIN_DATABASE_FORMAT=<name>  overrides the baked database_format
#                                          (e.g. KyotoCabinet, BerkeleyDB).
#
# Exits 0 on success; non-zero on a failed install or a missing baked template.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"
REPO_ROOT="$(cd ../.. && pwd)"
CRATE_DIR="$REPO_ROOT/crates/oxpinyin-capi"

PREFIX=""
LIBDIR=""
EXTRA=()

usage() {
  echo "usage: $0 --prefix=DIR [--libdir=DIR] [-- <extra cargo cinstall args>]" >&2
  exit 2
}

while [ $# -gt 0 ]; do
  case "$1" in
    --prefix=*) PREFIX="${1#*=}" ;;
    --prefix)   shift; PREFIX="${1:-}" ;;
    --libdir=*) LIBDIR="${1#*=}" ;;
    --libdir)   shift; LIBDIR="${1:-}" ;;
    --)         shift; EXTRA+=("$@"); break ;;
    *)          EXTRA+=("$1") ;;
  esac
  shift || true
done

[ -n "$PREFIX" ] || usage
# cargo-c defaults pkgconfigdir to <libdir>/pkgconfig; mirror its libdir default.
LIBDIR="${LIBDIR:-$PREFIX/lib}"

# 1. cargo-c builds + installs (its own incomplete libpinyin.pc lands in
#    <libdir>/pkgconfig). This build (re)generates build.rs's baked template.
( cd "$CRATE_DIR" && cargo cinstall --prefix="$PREFIX" --libdir="$LIBDIR" ${EXTRA[@]+"${EXTRA[@]}"} )

# 2. Locate build.rs's baked template, fresh from the cinstall build above.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
BAKED=""
for cand in "$TARGET_DIR/release/libpinyin.pc.in.baked" "$TARGET_DIR"/*/libpinyin.pc.in.baked; do
  if [ -f "$cand" ]; then BAKED="$cand"; break; fi
done
if [ -z "$BAKED" ]; then
  BAKED="$(find "$TARGET_DIR" -name libpinyin.pc.in.baked -path '*/out/*' 2>/dev/null | head -n1 || true)"
fi
if [ -z "$BAKED" ] || [ ! -f "$BAKED" ]; then
  echo "error: baked pkg-config template (libpinyin.pc.in.baked) not found under $TARGET_DIR" >&2
  echo "       expected build.rs to have written it during cargo cinstall." >&2
  exit 1
fi

# 3. Fill the install-time placeholders and overwrite the installed .pc.
PC_DIR="$LIBDIR/pkgconfig"
PC_OUT="$PC_DIR/libpinyin.pc"
mkdir -p "$PC_DIR"
# '#' delimiter so '/' in paths is literal; prefixes with '#' are not expected.
sed -e "s#@prefix@#${PREFIX}#g" -e "s#@libdir@#${LIBDIR}#g" "$BAKED" > "$PC_OUT"

echo "installed complete pkg-config file: $PC_OUT"
