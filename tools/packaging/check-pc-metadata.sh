#!/usr/bin/env bash
# check-pc-metadata.sh — diff oxpinyin's installed packaging metadata against
# the pin's, and exit non-zero on any difference.
#
# The pin is libpinyin at tools/oracle/oracle-pin.txt, built with its own
# autoconf build (`./configure [--with-dbm=…] --enable-libzhuyin`, then
# `make install DESTDIR=<pin-root>`). The subject is oxpinyin's drop-in,
# installed through tools/packaging/install.sh with the same --prefix /
# --libdir under DESTDIR=<subject-root> — or built here with --build-subject.
# Both trees must carry the same backend; the default build of each is
# Berkeley DB.
#
# Compared, per module (libpinyin, libzhuyin):
#   1. the installed .pc file, line for line;
#   2. the include directory: the version-stamped subdirectory names and the
#      header file set inside each;
#   3. pkg-config --modversion, --cflags, --libs, --print-variables, and
#      --variable=<v> for every variable either side defines — once as
#      installed, once relocated with --define-variable=prefix=<elsewhere>
#      (what exposes a libdir/includedir that is not prefix-relative);
#   4. pkg-config --atleast-version=<the pin's modversion> exit status (both
#      sides must answer 0 for the pin's own version).
# Outputs are compared after replacing each side's root with `<root>`, so the
# two trees may live anywhere; everything else must match byte for byte.
#
# Usage: tools/packaging/check-pc-metadata.sh --pin-root=DIR --subject-root=DIR
#            --prefix=DIR [--libdir=DIR] [--includedir=DIR]
#            [--build-subject=FEATURES] [--modules="libpinyin libzhuyin"]
#   --build-subject  run install.sh for both libraries into --subject-root
#                    first (emptying it), with `--release --locked
#                    --no-default-features --features FEATURES` (libpinyin
#                    also gets `shipped`); FEATURES is the backend
#                    (bdb|kyotocabinet|tkrzw).
#   --libdir / --includedir  the values BOTH sides were installed with;
#                    omitted means autoconf's defaults (<prefix>/lib,
#                    <prefix>/include), the tree layout either side produces
#                    without the flag.
#
# Exits 0 when every item matches; 1 on any difference (each one printed as a
# unified diff); 2 on a usage error or a missing input.

set -euo pipefail
cd "$(dirname "$0")"
SCRIPT_DIR="$(pwd)"

PIN_ROOT=""
SUBJECT_ROOT=""
PREFIX=""
LIBDIR=""
INCLUDEDIR=""
BUILD_FEATURES=""
MODULES="libpinyin libzhuyin"

usage() {
  sed -n '/^# Usage:/,/^# Exits/p' "$0" | sed 's/^# \{0,1\}//' >&2
  exit 2
}

for arg in "$@"; do
  case "$arg" in
    --pin-root=*)      PIN_ROOT="${arg#*=}" ;;
    --subject-root=*)  SUBJECT_ROOT="${arg#*=}" ;;
    --prefix=*)        PREFIX="${arg#*=}" ;;
    --libdir=*)        LIBDIR="${arg#*=}" ;;
    --includedir=*)    INCLUDEDIR="${arg#*=}" ;;
    --build-subject=*) BUILD_FEATURES="${arg#*=}" ;;
    --modules=*)       MODULES="${arg#*=}" ;;
    *) echo "error: unknown argument '$arg'" >&2; usage ;;
  esac
done
[ -n "$PIN_ROOT" ] && [ -n "$SUBJECT_ROOT" ] && [ -n "$PREFIX" ] || usage
command -v pkg-config >/dev/null || { echo "error: pkg-config not found" >&2; exit 2; }

if [ -n "$BUILD_FEATURES" ]; then
  rm -rf -- "$SUBJECT_ROOT"
  layout=(--prefix="$PREFIX" --destdir="$SUBJECT_ROOT")
  [ -n "$LIBDIR" ] && layout+=(--libdir="$LIBDIR")
  [ -n "$INCLUDEDIR" ] && layout+=(--includedir="$INCLUDEDIR")
  "$SCRIPT_DIR/install.sh" libpinyin "${layout[@]}" -- \
    --release --locked --no-default-features --features "$BUILD_FEATURES,shipped"
  "$SCRIPT_DIR/install.sh" libzhuyin "${layout[@]}" -- \
    --release --locked --no-default-features --features "$BUILD_FEATURES"
fi

PIN_ROOT="$(cd -- "$PIN_ROOT" && pwd)" || { echo "error: no pin root" >&2; exit 2; }
SUBJECT_ROOT="$(cd -- "$SUBJECT_ROOT" && pwd)" || { echo "error: no subject root" >&2; exit 2; }
LIBDIR="${LIBDIR:-$PREFIX/lib}"
INCLUDEDIR="${INCLUDEDIR:-$PREFIX/include}"
# A relocation target that shares no prefix with either tree.
RELOCATED="/relocated-by-check-pc-metadata"

WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT
DIFFS=0

# compare <label> <pin-file> <subject-file>
compare() {
  if ! diff -u --label "pin: $1" --label "oxpinyin: $1" -- "$2" "$3"; then
    DIFFS=$((DIFFS + 1))
  fi
}

# pc <root> <args...>: one pkg-config query with <root>'s pkgconfig dir
# searched first (the system dirs stay on the path: Requires: glib-2.0 must
# resolve), stdout, stderr and exit status all recorded, <root> normalized.
pc() {
  local root="$1" out rc=0
  shift
  out="$(PKG_CONFIG_PATH="$root$LIBDIR/pkgconfig" pkg-config "$@" 2>&1)" || rc=$?
  printf '%s\nexit=%s\n' "${out//"$root"/<root>}" "$rc"
}

# The header layout under <root>'s includedir: every file path relative to it.
headers() {
  if [ -d "$1$INCLUDEDIR" ]; then
    (cd -- "$1$INCLUDEDIR" && find . -mindepth 1 | LC_ALL=C sort)
  else
    echo "(no $INCLUDEDIR)"
  fi
}

side() { # side <pin|subject> -> root
  if [ "$1" = pin ]; then echo "$PIN_ROOT"; else echo "$SUBJECT_ROOT"; fi
}

echo "== include directory $INCLUDEDIR"
for s in pin subject; do headers "$(side "$s")" > "$WORK/$s.headers"; done
compare "headers under $INCLUDEDIR" "$WORK/pin.headers" "$WORK/subject.headers"

for mod in $MODULES; do
  echo "== $mod"
  for s in pin subject; do
    root="$(side "$s")"
    pcfile="$root$LIBDIR/pkgconfig/$mod.pc"
    if [ -f "$pcfile" ]; then cp -- "$pcfile" "$WORK/$s.$mod.pc"; else echo "(missing $LIBDIR/pkgconfig/$mod.pc)" > "$WORK/$s.$mod.pc"; fi
  done
  compare "$LIBDIR/pkgconfig/$mod.pc" "$WORK/pin.$mod.pc" "$WORK/subject.$mod.pc"

  # Every variable either side defines, so a variable only one side has
  # shows up as a difference rather than being skipped.
  vars="$( { pc "$PIN_ROOT" --print-variables "$mod"; pc "$SUBJECT_ROOT" --print-variables "$mod"; } \
           | grep -v '^exit=' | grep -v '^$' | LC_ALL=C sort -u)"
  # The pin must answer, or every query below would compare two errors.
  if ! pin_version="$(PKG_CONFIG_PATH="$PIN_ROOT$LIBDIR/pkgconfig" pkg-config --modversion "$mod")"; then
    echo "error: pkg-config cannot resolve $mod in the pin tree ($PIN_ROOT$LIBDIR/pkgconfig)" >&2
    exit 2
  fi

  for s in pin subject; do
    root="$(side "$s")"
    {
      for query in --modversion --cflags --libs --print-variables; do
        echo "## pkg-config $query"
        pc "$root" "$query" "$mod"
      done
      for v in $vars; do
        echo "## pkg-config --variable=$v"
        pc "$root" --variable="$v" "$mod"
      done
      echo "## pkg-config --atleast-version=$pin_version (the pin's modversion)"
      pc "$root" --atleast-version="$pin_version" "$mod"
      echo "## pkg-config --exact-version=$pin_version"
      pc "$root" --exact-version="$pin_version" "$mod"
      for query in --cflags --libs; do
        echo "## pkg-config --define-variable=prefix=$RELOCATED $query"
        pc "$root" --define-variable=prefix="$RELOCATED" "$query" "$mod"
      done
      for v in $vars; do
        echo "## pkg-config --define-variable=prefix=$RELOCATED --variable=$v"
        pc "$root" --define-variable=prefix="$RELOCATED" --variable="$v" "$mod"
      done
    } > "$WORK/$s.$mod.queries"
  done
  compare "pkg-config queries for $mod" "$WORK/pin.$mod.queries" "$WORK/subject.$mod.queries"
done

if [ "$DIFFS" -ne 0 ]; then
  echo "FAIL: $DIFFS item(s) differ from the pin (prefix=$PREFIX libdir=$LIBDIR includedir=$INCLUDEDIR)" >&2
  exit 1
fi
echo "OK: packaging metadata matches the pin (prefix=$PREFIX libdir=$LIBDIR includedir=$INCLUDEDIR)"
