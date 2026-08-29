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

# Map a cargo profile name to its target subdirectory ('dev' builds into 'debug').
profile_dir() {
  case "$1" in
    dev) echo "debug" ;;
    *)   echo "$1" ;;
  esac
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
#    build.rs mirrors it to <target-dir>[/<triple>]/<profile>/, so the exact
#    path is derived from the SAME target-dir / --target / profile the build
#    used — not a broad search that could pick up an unrelated crate's `out`
#    artifact or a stale profile. Read those from the passthrough args (which
#    were forwarded to cargo cinstall verbatim); cargo cinstall builds release.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
TRIPLE=""
PROFILE_DIR="release"
i=0
while [ "$i" -lt "${#EXTRA[@]}" ]; do
  arg="${EXTRA[$i]}"
  case "$arg" in
    --target-dir=*) TARGET_DIR="${arg#*=}" ;;
    --target-dir)   i=$((i + 1)); TARGET_DIR="${EXTRA[$i]:-$TARGET_DIR}" ;;
    --target=*)     TRIPLE="${arg#*=}" ;;
    --target)       i=$((i + 1)); TRIPLE="${EXTRA[$i]:-}" ;;
    --profile=*)    PROFILE_DIR="$(profile_dir "${arg#*=}")" ;;
    --profile)      i=$((i + 1)); PROFILE_DIR="$(profile_dir "${EXTRA[$i]:-release}")" ;;
    --release)      PROFILE_DIR="release" ;;
    --debug)        PROFILE_DIR="debug" ;;
  esac
  i=$((i + 1))
done

# cargo cinstall always builds under an explicit target triple (cargo-c sets it
# to rustc.host when --target is absent — build.rs:825-828), so the layout is
# target/<selector>/<profile>/. Resolve the same selector: --target if given,
# else $CARGO_BUILD_TARGET, else rustc's host.
if [ -z "$TRIPLE" ]; then
  TRIPLE="${CARGO_BUILD_TARGET:-}"
fi
if [ -z "$TRIPLE" ]; then
  TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
fi
# A --target given as a custom spec file (…/foo.json, or foo.json found via
# RUST_TARGET_PATH) builds under target/<foo>/ — cargo names the dir after the
# file stem, not the path. Normalize to that stem; a plain triple is unchanged.
if [ -n "$TRIPLE" ]; then
  TRIPLE="$(basename -- "$TRIPLE" .json)"
fi
if [ -z "$TRIPLE" ]; then
  echo "error: could not determine the build target (rustc -vV had no 'host:' line)" >&2
  exit 1
fi

# build.rs writes the baked template to the mirror next to <profile>/ and,
# always (a hard write), to the OUT_DIR copy at <profile>/build/<pkg-hash>/out/.
# Prefer the mirror; if that best-effort write is missing, fall back to the
# OUT_DIR copy — but stay scoped to THIS target+profile's build tree, never an
# unqualified whole-target search, and require exactly one candidate so a stale
# build-hash dir is an ambiguity error rather than a silent wrong pick.
QUALIFIED_DIR="$TARGET_DIR/$TRIPLE/$PROFILE_DIR"
BAKED="$QUALIFIED_DIR/libpinyin.pc.in.baked"
if [ ! -f "$BAKED" ]; then
  BAKED=""
  candidates=()
  if [ -d "$QUALIFIED_DIR/build" ]; then
    while IFS= read -r cand; do
      candidates+=("$cand")
    done < <(find "$QUALIFIED_DIR/build" -mindepth 3 -maxdepth 3 \
                  -path '*/out/libpinyin.pc.in.baked' -type f 2>/dev/null)
  fi
  if [ "${#candidates[@]}" -eq 1 ]; then
    BAKED="${candidates[0]}"
  elif [ "${#candidates[@]}" -gt 1 ]; then
    echo "error: multiple baked pkg-config templates under $QUALIFIED_DIR/build:" >&2
    printf '         %s\n' "${candidates[@]}" >&2
    echo "       remove the stale build dirs (or 'cargo clean') and retry." >&2
    exit 1
  fi
fi
if [ -z "$BAKED" ] || [ ! -f "$BAKED" ]; then
  echo "error: baked pkg-config template not found for this build:" >&2
  echo "         $QUALIFIED_DIR/libpinyin.pc.in.baked" >&2
  echo "         (nor a unique $QUALIFIED_DIR/build/*/out/libpinyin.pc.in.baked)" >&2
  echo "       (target='$TRIPLE' profile='$PROFILE_DIR' target-dir='$TARGET_DIR')" >&2
  echo "       build.rs writes it during 'cargo cinstall'; check the passthrough args match the build." >&2
  exit 1
fi

# 3. Fill the install-time placeholders and overwrite the installed .pc.
PC_DIR="$LIBDIR/pkgconfig"
PC_OUT="$PC_DIR/libpinyin.pc"
mkdir -p "$PC_DIR"
# Escape sed replacement metacharacters — '\', '&', and the '#' delimiter — so a
# path containing any of them substitutes literally instead of corrupting the
# file (e.g. '&' would re-insert the match, a bare '#' would end the s-command).
prefix_esc="$(printf '%s' "$PREFIX" | sed 's/[\\&#]/\\&/g')"
libdir_esc="$(printf '%s' "$LIBDIR" | sed 's/[\\&#]/\\&/g')"
sed -e "s#@prefix@#${prefix_esc}#g" -e "s#@libdir@#${libdir_esc}#g" "$BAKED" > "$PC_OUT"

echo "installed complete pkg-config file: $PC_OUT"
