"""Package identities and consumer fixtures preserved from the U4 shell entry points."""

COPYRIGHT = r'''Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: oxpinyin
Source: $HOMEPAGE

Files: *
Copyright: 2026 oxpinyin contributors
License: GPL-3.0-or-later
 On Debian systems, the complete text of the GNU General Public
 License version 3 can be found in "/usr/share/common-licenses/GPL-3".
'''

DEB_RUNTIME = r'''Package: $BASE
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
'''

DEB_DEV = r'''Package: $BASE-dev
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
'''

RPM_SPEC = r'''%define debug_package %{nil}
Name:           $NAME
Version:        $VERSION
Release:        1%{?dist}
Summary:        Library to deal with pinyin — oxpinyin drop-in, $BACKEND store
License:        GPL-3.0-or-later
URL:            https://github.com/shenghaoc/oxpinyin
Source0:        staged-tree.tar.gz
Obsoletes:      libpinyin libzhuyin
Provides:       libpinyin = $PCVER libzhuyin = $PCVER

%description
oxpinyin is a portable Rust re-expression of libpinyin. This package installs
libpinyin.so.15 and libzhuyin.so.15 built against the $BACKEND store backend,
replacing libpinyin and libzhuyin in their entirety. It carries no data of its
own: the model files remain those of the installed libpinyin-data package.

%package devel
Summary:        Development files for oxpinyin ($BACKEND store)
License:        GPL-3.0-or-later
Requires:       %{name} = %{version}-%{release}
Obsoletes:      libpinyin-devel libzhuyin-devel
Provides:       libpinyin-devel = $PCVER libzhuyin-devel = $PCVER

%description devel
Headers, pkg-config files, linker symlinks and static archives for the
oxpinyin drop-in libpinyin.so.15 / libzhuyin.so.15.

%prep
%setup -q -c -n stage

%build
# The staged tree is prebuilt; nothing to compile.

%install
mkdir -p %{buildroot}
# %setup -c unpacks INSIDE stage/, which is also the build subdirectory
# %install runs from — copy from the explicit builddir path, not "stage/usr".
cp -a %{_builddir}/stage/usr %{buildroot}/

%files
%{_libdir}/libpinyin.so.15*
%{_libdir}/libzhuyin.so.15*

%files devel
%{_includedir}/libpinyin-$PCVER/
%{_libdir}/libpinyin.so
%{_libdir}/libzhuyin.so
%{_libdir}/libpinyin.a
%{_libdir}/libzhuyin.a
%{_libdir}/pkgconfig/libpinyin.pc
%{_libdir}/pkgconfig/libzhuyin.pc

%changelog
* Sat Sep 06 2026 Shenghao Chen <shenghaoc@outlook.com> - $VERSION-1
- Release build of the oxpinyin drop-in ($BACKEND store backend);
  replaces libpinyin/libzhuyin in their entirety.
'''

ARCH_BUILD = r'''pkgname=$PKGNAME
pkgver=$VERSION
pkgrel=1
pkgdesc='$DESC'
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
'''

SMOKE_PINYIN = r'''#include <stdbool.h>
#include <pinyin.h>
#include <stdio.h>

int main(void) {
  pinyin_context_t *c = pinyin_init("", "");
  printf("pinyin=%p\n", (void *)c);
  return 0;
}
'''

SMOKE_ZHUYIN = r'''#include <stdbool.h>
#include <zhuyin.h>
#include <stdio.h>

int main(void) {
  zhuyin_context_t *c = zhuyin_init("", "");
  printf("zhuyin=%p\n", (void *)c);
  return 0;
}
'''

METADATA_USAGE = r'''Usage: tools/packaging/check-pc-metadata.sh --pin-root=DIR --subject-root=DIR
           --prefix=DIR [--libdir=DIR] [--includedir=DIR]
           [--build-subject=FEATURES] [--modules="libpinyin libzhuyin"]
  --build-subject  run install.sh for both libraries into --subject-root
                   first (emptying it), with `--profile dev --locked
                   --no-default-features --features FEATURES` (libpinyin
                   also gets `shipped`); FEATURES is the backend
                   (bdb|kyotocabinet|tkrzw).
  --libdir / --includedir  the values BOTH sides were installed with;
                   omitted means autoconf's defaults (<prefix>/lib,
                   <prefix>/include), the tree layout either side produces
                   without the flag.

Exits 0 when every item matches; 1 on any difference (each one printed as a
unified diff); 2 on a usage error or a missing input.
'''

