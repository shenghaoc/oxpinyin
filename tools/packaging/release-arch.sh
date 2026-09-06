#!/usr/bin/env bash
# release-arch.sh — package a staged drop-in tree as an Arch package, in the
# shape of the real one: Arch ships libpinyin as ONE package (library,
# headers, pkg-config and data together), so this is a single package too:
#
#   oxpinyin-libpinyin-<backend>  ~ libpinyin (which bundles libzhuyin's role)
#
# Installing it replaces the distro's libpinyin/libzhuyin in their entirety:
# it answers to their sonames (provides libpinyin.so=15-64, libzhuyin.so=15-64
# — the -64 suffix is pacman's soname-provision form for 64-bit arches),
# provides/conflicts the package names, and pacman offers to remove the
# originals.
#
# ARCH DATA: on Arch the libpinyin package bundles the model data
# (/usr/lib/libpinyin/data), unlike Debian/Fedora where libpinyin-data is a
# separate package that survives the takeover. Removing Arch's libpinyin
# therefore removes the model with it, and there is no data package to depend
# on instead — so this package must carry the data itself or leave
# pinyin_init()/zhuyin_init() with nothing to open. `--data=DIR` installs DIR
# as /usr/lib/libpinyin/data (the caller hands it the data directory
# extracted from Arch's own libpinyin package — same KyotoCabinet format this
# backend reads, same GPL-3.0-or-later licence, and pkgdatadir in our
# libpinyin.pc already points one level above it). Omitting --data is
# allowed for a bare library build but the result cannot initialise on a
# system whose libpinyin it replaced; the release workflow always passes it.
#
# makepkg refuses to run as root, so a build user is created and the package
# is built as that user; the script itself is expected to run as root (CI
# container) or via sudo.
#
# Usage: release-arch.sh <backend> <version> <stage-root> <outdir> [--data=DIR]
#   e.g. release-arch.sh kyotocabinet 0.1.0 stage dist --data=arch-data

set -euo pipefail

BACKEND="${1:?backend required}"
VERSION="${2:?version required}"
STAGE="${3:?stage root required}"
OUTDIR="${4:?outdir required}"
shift 4
DATADIR=""
for arg in "$@"; do
  case "$arg" in
    --data=*) DATADIR="${arg#--data=}" ;;
    *) echo "error: unknown argument '$arg'" >&2; exit 2 ;;
  esac
done
if [ -n "$DATADIR" ]; then
  [ -f "$DATADIR/table.conf" ] \
    || { echo "error: --data=$DATADIR holds no table.conf" >&2; exit 1; }
fi

PCVER="2.11.91"      # mirrors [package.metadata.capi.pkg_config].version
PKGNAME="oxpinyin-libpinyin-${BACKEND}"
ARCH="$(uname -m)"
# pacman's soname provision suffix for the 64-bit arches (x86_64, aarch64).
SOARCH=64

case "$BACKEND" in
  kyotocabinet) EXTRA_DEPS="kyotocabinet" ;;
  tkrzw)        EXTRA_DEPS="tkrzw" ;;
  lmdb)         EXTRA_DEPS="lmdb" ;;
  redb)         EXTRA_DEPS="" ;;
  *) echo "error: unknown backend '$BACKEND'" >&2; exit 2 ;;
esac

[ -d "$STAGE/usr" ] || { echo "error: $STAGE/usr missing" >&2; exit 1; }
mkdir -p -- "$OUTDIR"
OUTDIR="$(cd -- "$OUTDIR" && pwd)"

# Build as an unprivileged user (makepkg refuses root). $WORK stays root-owned
# except the subtree the builder must write.
WORK="$(mktemp -d)"
# mktemp -d is 0700 root-owned; the build user must be able to traverse it
# to reach the chown'd package working dir below it.
chmod 0711 -- "$WORK"
trap 'rm -rf -- "$WORK"' EXIT
PKGWORK="$WORK/pkg"
mkdir -p -- "$PKGWORK"
cp -a -- "$STAGE/usr" "$PKGWORK/stage-usr"
if [ -n "$DATADIR" ]; then
  [ -e "$PKGWORK/stage-usr/lib/libpinyin/data" ] \
    && { echo "error: staged tree already carries lib/libpinyin/data" >&2; exit 1; }
  mkdir -p -- "$PKGWORK/stage-usr/lib/libpinyin"
  cp -a -- "$DATADIR" "$PKGWORK/stage-usr/lib/libpinyin/data"
fi

cat > "$PKGWORK/PKGBUILD" <<EOF
pkgname=$PKGNAME
pkgver=$VERSION
pkgrel=1
pkgdesc='Library to deal with pinyin — oxpinyin drop-in ($BACKEND store backend)'
arch=($ARCH)
url='https://github.com/shenghaoc/oxpinyin'
license=('GPL-3.0-or-later')
depends=(glib2 gcc-libs $EXTRA_DEPS)
provides=(libpinyin=$PCVER libzhuyin=$PCVER libpinyin.so=15-$SOARCH libzhuyin.so=15-$SOARCH)
conflicts=(libpinyin libzhuyin)
# The staged tree is already stripped; never emit a -debug split package.
options=(!debug)

package() {
  cp -a "\$startdir/stage-usr" "\$pkgdir/usr"
  chmod -R u+rwX,go+rX,go-w "\$pkgdir"
}
EOF

BUILDER="oxp-builder"
if ! id -u "$BUILDER" >/dev/null 2>&1; then
  useradd --create-home --shell /bin/bash "$BUILDER"
fi
chown -R -- "$BUILDER" "$PKGWORK"
chmod a+w -- "$OUTDIR"

# --nodeps: every dependency is already installed in this build container and
# makepkg would otherwise want sudo to re-resolve them.
su "$BUILDER" -c "cd '$PKGWORK' && env PKGDEST='$OUTDIR' makepkg --nodeps --noconfirm --force"

pkg="$OUTDIR/${PKGNAME}-${VERSION}-1-${ARCH}.pkg.tar.zst"
[ -f "$pkg" ] || { echo "error: expected package $pkg not found" >&2; exit 1; }

echo "== arch package written to $OUTDIR =="
ls -l "$OUTDIR"
