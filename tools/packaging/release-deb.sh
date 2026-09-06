#!/usr/bin/env bash
# release-deb.sh — split a staged drop-in tree into Debian runtime/dev binary
# packages, in the shape of the real libpinyin packaging:
#
#   oxpinyin-libpinyin15-<backend>         ~ libpinyin15 + libzhuyin15
#   oxpinyin-libpinyin15-<backend>-dev     ~ libpinyin15-dev + libzhuyin-dev
#
# Installing the runtime package replaces the distro's libpinyin15/libzhuyin15
# in their entirety: it answers to their sonames (libpinyin.so.15,
# libzhuyin.so.15), Provides their names at the pinned pc version, and
# Conflicts/Replaces them. It does NOT touch libpinyin-data: the data is a
# separate package on Debian and the library reads it in place — it is only
# Recommends here, so an uninstall of ours keeps the data intact too.
#
# Package Version is the oxpinyin release tag (0.x stream); the
# consumer-facing version stays 2.11.91 in the .pc and the Provides, the same
# split docs/packaging.md "The four version streams" records.
#
# Depends is computed, not hardcoded: every DT_NEEDED soname of the shipped
# .so files is mapped to its owning Debian package via ldconfig + dpkg -S, so
# the t64-era names (libglib2.0-0t64, libtkrzw1t64, ...) are correct for
# whatever suite the build runs on, plus a libc6 (>= N) floor taken from the
# highest GLIBC_* symbol version actually referenced.
#
# Usage: release-deb.sh <backend> <version> <stage-root> <libdir> <outdir>
#   e.g. release-deb.sh tkrzw 0.1.0 stage /usr/lib/x86_64-linux-gnu dist

set -euo pipefail

BACKEND="${1:?backend required}"
VERSION="${2:?version required}"
STAGE="${3:?stage root required}"
LIBDIR="${4:?libdir required}"
OUTDIR="${5:?outdir required}"

ARCH="$(dpkg --print-architecture)"
DEBVER="${VERSION}-1"
BASE="oxpinyin-libpinyin15-${BACKEND}"
MAINTAINER="Shenghao Chen <shenghaoc@outlook.com>"
HOMEPAGE="https://github.com/shenghaoc/oxpinyin"
# Mirror [package.metadata.capi.pkg_config].version: the libpinyin release
# whose ABI and consumers this drop-in answers to.
PCVER="2.11.91"

STAGE_LIB="$STAGE$LIBDIR"
for f in libpinyin.so.15.0.0 libzhuyin.so.15.0.0; do
  [ -f "$STAGE_LIB/$f" ] || { echo "error: $STAGE_LIB/$f missing" >&2; exit 1; }
done
mkdir -p -- "$OUTDIR"

# --- Depends computation ---------------------------------------------------

# Highest GLIBC_* symbol version referenced by the shipped libraries; becomes
# the libc6 (>= N) floor, matching how the real libpinyin15 carries it.
glibc_floor() {
  { objdump -T "$STAGE_LIB/libpinyin.so.15.0.0" "$STAGE_LIB/libzhuyin.so.15.0.0"; } \
    | sed -n 's/.*GLIBC_\([0-9.]*\).*/\1/p' | sort -Vu | tail -n1
}

# Every DT_NEEDED soname -> library path -> owning package. Fails loudly on
# any soname no installed package owns: shipping an unsatisfiable Depends
# would be worse than stopping the build. Called via command substitution so
# a failure propagates through set -e (process substitution would swallow it).
needed_pkgs() {
  local sonames s path pkg
  sonames="$(objdump -p "$STAGE_LIB/libpinyin.so.15.0.0" "$STAGE_LIB/libzhuyin.so.15.0.0" \
    | awk '/NEEDED/ {print $2}' | sort -u)"
  for s in $sonames; do
    # Exclude our own sonames; nothing else links them.
    case "$s" in libpinyin.so.15|libzhuyin.so.15) continue ;; esac
    path="$(ldconfig -p | awk -v s="$s" '$1 == s {print $NF; exit}')"
    [ -n "$path" ] || { echo "error: soname $s not in ldconfig cache" >&2; exit 1; }
    # Canonicalize past merged-usr /lib -> /usr/lib: dpkg's database records
    # only the /usr/lib spelling, and dpkg -S does not resolve symlinks.
    path="$(readlink -f -- "$path")"
    pkg="$(dpkg -S -- "$path" 2>/dev/null | head -n1 | cut -d: -f1 || true)"
    [ -n "$pkg" ] || { echo "error: no package owns $path ($s)" >&2; exit 1; }
    printf '%s\n' "${pkg%%:*}"
  done | sort -u
}

PKGS="$(needed_pkgs)"
DEPS="libc6 (>= $(glibc_floor))"
for p in $PKGS; do
  [ "$p" = "libc6" ] || DEPS="$DEPS, $p"
done

# --- tree assembly ----------------------------------------------------------

WORK="$(mktemp -d)"
trap 'rm -rf -- "$WORK"' EXIT
RT="$WORK/runtime"
DEV="$WORK/dev"

# runtime: the versioned shared objects only
mkdir -p -- "$RT$LIBDIR"
install -m0755 -- "$STAGE_LIB/libpinyin.so.15.0.0" "$RT$LIBDIR/"
install -m0755 -- "$STAGE_LIB/libzhuyin.so.15.0.0" "$RT$LIBDIR/"
ln -sfn -- libpinyin.so.15.0.0 "$RT$LIBDIR/libpinyin.so.15"
ln -sfn -- libzhuyin.so.15.0.0 "$RT$LIBDIR/libzhuyin.so.15"

# dev: unversioned linker symlinks, static archives, headers, .pc files
mkdir -p -- "$DEV$LIBDIR" "$DEV$LIBDIR/pkgconfig" "$DEV/usr/include/libpinyin-2.11.91" \
         "$DEV/usr/share/doc/$BASE-dev"
install -m0644 -- "$STAGE_LIB/libpinyin.a" "$STAGE_LIB/libzhuyin.a" "$DEV$LIBDIR/"
install -m0644 -- "$STAGE_LIB/pkgconfig/libpinyin.pc" \
                  "$STAGE_LIB/pkgconfig/libzhuyin.pc" "$DEV$LIBDIR/pkgconfig/"
install -m0644 -- "$STAGE/usr/include/libpinyin-2.11.91/"*.h \
                  "$DEV/usr/include/libpinyin-2.11.91/"
ln -sfn -- libpinyin.so.15 "$DEV$LIBDIR/libpinyin.so"
ln -sfn -- libzhuyin.so.15 "$DEV$LIBDIR/libzhuyin.so"

copyright() { # $1=dir
  cat > "$1/usr/share/doc/$2/copyright" <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: oxpinyin
Source: $HOMEPAGE

Files: *
Copyright: 2026 oxpinyin contributors
License: GPL-3.0-or-later
 On Debian systems, the complete text of the GNU General Public
 License version 3 can be found in "/usr/share/common-licenses/GPL-3".
EOF
}

write_md5sums() { # $1=tree
  ( cd "$1" && find . -type f -printf '%P\0' | sort -z \
      | xargs -0 --no-run-if-empty md5sum > DEBIAN/md5sums )
}

mkdir -p -- "$RT/DEBIAN" "$RT/usr/share/doc/$BASE" \
         "$DEV/DEBIAN" "$DEV/usr/share/doc/$BASE-dev"

cat > "$RT/DEBIAN/control" <<EOF
Package: $BASE
Version: $DEBVER
Architecture: $ARCH
Section: libs
Priority: optional
Maintainer: $MAINTAINER
Homepage: $HOMEPAGE
Depends: $DEPS
Recommends: libpinyin-data
Provides: libpinyin15 (= $PCVER), libzhuyin15 (= $PCVER)
Conflicts: libpinyin15, libzhuyin15
Replaces: libpinyin15, libzhuyin15
Description: library to deal with PinYin - oxpinyin drop-in ($BACKEND store)
 oxpinyin is a portable Rust re-expression of libpinyin. This package
 installs libpinyin.so.15 and libzhuyin.so.15 built against the $BACKEND
 store backend, replacing libpinyin15 and libzhuyin15 in their entirety.
 .
 It carries no data of its own: the model files remain those of the
 installed libpinyin-data package.
EOF
copyright "$RT" "$BASE"
write_md5sums "$RT"

cat > "$DEV/DEBIAN/control" <<EOF
Package: $BASE-dev
Version: $DEBVER
Architecture: $ARCH
Section: libdevel
Priority: optional
Maintainer: $MAINTAINER
Homepage: $HOMEPAGE
Depends: $BASE (= $DEBVER)
Provides: libpinyin15-dev (= $PCVER), libzhuyin-dev (= $PCVER)
Conflicts: libpinyin15-dev, libzhuyin-dev
Replaces: libpinyin15-dev, libzhuyin-dev
Description: development files for oxpinyin ($BACKEND store)
 Headers, pkg-config files, linker symlinks and static archives for the
 oxpinyin drop-in libpinyin.so.15 / libzhuyin.so.15. Replaces
 libpinyin15-dev and libzhuyin-dev in their entirety.
EOF
copyright "$DEV" "$BASE-dev"
write_md5sums "$DEV"

dpkg-deb --root-owner-group --build "$RT" "$OUTDIR/${BASE}_${DEBVER}_${ARCH}.deb"
dpkg-deb --root-owner-group --build "$DEV" "$OUTDIR/${BASE}-dev_${DEBVER}_${ARCH}.deb"

echo "== debs written to $OUTDIR =="
ls -l "$OUTDIR"
