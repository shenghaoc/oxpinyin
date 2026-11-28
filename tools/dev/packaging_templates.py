"""Package identities and consumer fixtures preserved from the U4 shell entry points."""

TRAINER_MAKEFILE = r'''# External interface: epico/trainer evaluate.py invokes /usr/bin/make here.
# Thin recipes adapted from libpinyin 074a2219 data/Makefile.am:18-69
# (Copyright (C) 2011 Peng Wu, GPL-3.0-or-later).
# Supply the model's textual tables and interpolation2.text, and fill in
# the backend in table.conf.in to make table.conf before running make.
top_srcdir = ..
tablefiles = gb_char.table gbk_char.table opengram.table merged.table \
             art.table culture.table economy.table geology.table history.table \
             life.table nature.table people.table science.table society.table \
             sport.table technology.table punct.table
binfiles = ${tablefiles:.table=.bin}
textual_model_data = interpolation2.text $(tablefiles)
binary_model_data = phrase_index.bin pinyin_index.bin \
                    addon_phrase_index.bin addon_pinyin_index.bin bigram.db $(binfiles)

.PHONY: all clean modify
all: $(binary_model_data) table.conf

$(tablefiles) table.conf: interpolation2.text

bigram.db: $(textual_model_data)
	$(RM) $(binary_model_data)
	../utils/storage/gen_binary_files --gen-punct-table --table-dir $(top_srcdir)/data
	../utils/storage/import_interpolation --table-dir $(top_srcdir)/data < $(top_srcdir)/data/interpolation2.text
	../utils/training/gen_unigram --table-dir $(top_srcdir)/data

addon_phrase_index.bin phrase_index.bin addon_pinyin_index.bin pinyin_index.bin $(binfiles): bigram.db

clean:
	$(RM) $(binary_model_data)

# Retain the initial table.conf locally so every lambda update starts
# from the same baseline.
export LAMBDA_PARAMETER
modify: table.conf
	test -f .trainer-table.conf || cp table.conf .trainer-table.conf
	sed -E "s'lambda parameter:0\.[0-9]{3,6}'lambda parameter:$$LAMBDA_PARAMETER'" .trainer-table.conf > table.conf
'''

# The release recipes (`just packaging release-recipe`). Each one runs the build and
# the install through tools/dev/packaging.mk from its distribution's own driver,
# with cargo offline on the vendored crates. Fields are $UPPERCASE; `\$` is a
# literal dollar.

# Source replacement for the vendored crates. %cargo_prep -v vendor writes its own.
CARGO_CONFIG = r'''[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"

[net]
offline = true
'''

DEB_CONTROL = r'''Source: oxpinyin
Section: libs
Priority: optional
Maintainer: $MAINTAINER
Build-Depends: debhelper-compat (= 13), cargo, rustc, cargo-c, make, pkgconf, g++, libglib2.0-dev, $BUILD_DEPS
Standards-Version: 4.7.2
Homepage: $HOMEPAGE
Rules-Requires-Root: no

Package: $BASE
Architecture: any
Depends: \${shlibs:Depends}, \${misc:Depends}, libpinyin-data (>= 2.11.91-1~exp1)
Provides: libpinyin, libzhuyin, libpinyin15 (= $PCVER), libzhuyin15 (= $PCVER)
Conflicts: libpinyin15, libzhuyin15
Replaces: libpinyin15, libzhuyin15
Description: library to deal with PinYin - oxpinyin drop-in ($BACKEND store)
 oxpinyin is an idiomatic Rust rewrite of libpinyin. This package
 installs libpinyin.so.15 and libzhuyin.so.15 built against the $BACKEND
 store backend, replacing libpinyin15 and libzhuyin15 in their entirety.
 .
 It carries no data of its own: the model files remain those of the
 installed libpinyin-data package.

Package: $BASE-dev
Section: libdevel
Architecture: any
Depends: $BASE (= \${binary:Version}), \${misc:Depends}, libglib2.0-dev, libpinyin-common-dev (>= $PCVER), libpinyin-common-dev (<< $NEXTPCVER)
Provides: libpinyin-dev, libpinyin15-dev (= $PCVER), libzhuyin-dev (= $PCVER)
Conflicts: libpinyin15-dev, libzhuyin-dev
Replaces: libpinyin15-dev, libzhuyin-dev
Description: development files for oxpinyin ($BACKEND store)
 Headers, pkg-config files, linker symlinks and static archives for the
 oxpinyin drop-in libpinyin.so.15 / libzhuyin.so.15. Replaces
 libpinyin15-dev and libzhuyin-dev in their entirety. The two common
 headers, novel_types.h and pinyin_custom2.h, come from
 libpinyin-common-dev, as in Debian's libpinyin15-dev.
'''

# hardening=+all is the option of Debian's libpinyin debian/rules: without it
# dpkg-buildflags omits -Wl,-z,now. dpkg-buildpackage has not exported the
# flags since dpkg 1.16.1, so buildflags.mk exports them.
DEB_RULES = r'''#!/usr/bin/make -f
export DEB_BUILD_MAINT_OPTIONS = hardening=+all
DPKG_EXPORT_BUILDFLAGS = 1
include /usr/share/dpkg/buildflags.mk
include /usr/share/dpkg/architecture.mk

PACKAGING = $(MAKE) -f tools/dev/packaging.mk BACKEND=$BACKEND CARGO_FLAGS='--release --frozen' \
	prefix=/usr libdir=/usr/lib/$(DEB_HOST_MULTIARCH)

%:
	dh $@

# dh_clean deletes every *.orig file; the vendored crates' checksums list Cargo.toml.orig.
override_dh_clean:
	dh_clean -XCargo.toml.orig

override_dh_auto_build:
	$(PACKAGING)

override_dh_auto_install:
	$(PACKAGING) install DESTDIR=$(CURDIR)/debian/tmp
'''

DEB_CHANGELOG = r'''oxpinyin ($DEBVER) unstable; urgency=medium

  * Release build of the oxpinyin drop-in ($BACKEND store backend).

 -- $MAINTAINER  $DATE
'''

DEB_RUNTIME_INSTALL = r'''usr/lib/*/libpinyin.so.15*
usr/lib/*/libzhuyin.so.15*
'''

# Debian's libpinyin-common-dev owns novel_types.h and pinyin_custom2.h.
DEB_DEV_INSTALL = r'''usr/include/libpinyin-$PCVER/pinyin.h
usr/include/libpinyin-$PCVER/zhuyin.h
usr/include/libpinyin-$PCVER/zhuyin_custom2.h
usr/lib/*/libpinyin.a
usr/lib/*/libzhuyin.a
usr/lib/*/libpinyin.so
usr/lib/*/libzhuyin.so
usr/lib/*/pkgconfig/libpinyin.pc
usr/lib/*/pkgconfig/libzhuyin.pc
'''

DEB_NOT_INSTALLED = r'''usr/include/libpinyin-$PCVER/novel_types.h
usr/include/libpinyin-$PCVER/pinyin_custom2.h
'''

COPYRIGHT_HEADER = r'''Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: oxpinyin
Source: $HOMEPAGE

Files: *
Copyright: 2026 oxpinyin contributors
License: GPL-3.0-or-later
'''

# Licences that Debian keeps in /usr/share/common-licenses, by SPDX identifier.
COMMON_LICENSES = {'Apache-2.0': 'Apache-2.0', 'GPL-3.0-or-later': 'GPL-3'}

# The full text of each other licence of a shipped crate, without its copyright
# line. A crate under a licence missing here stops the recipe generation.
LICENSE_TEXTS = {
    'MIT': '''Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the "Software"), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.''',
    'Zlib': '''This software is provided 'as-is', without any express or implied warranty. In no event will the authors be held liable for any damages arising from the use of this software.

Permission is granted to anyone to use this software for any purpose, including commercial applications, and to alter it and redistribute it freely, subject to the following restrictions:

1. The origin of this software must not be misrepresented; you must not claim that you wrote the original software. If you use this software in a product, an acknowledgment in the product documentation would be appreciated but is not required.

2. Altered source versions must be plainly marked as such, and must not be misrepresented as being the original software.

3. This notice may not be removed or altered from any source distribution.''',
    'BSD-3-Clause': '''Redistribution and use in source and binary forms, with or without modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the following disclaimer in the documentation and/or other materials provided with the distribution.

3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.''',
}

# Fedora's packaging of vendored crates: %cargo_prep -v vendor sets up the
# offline build. The License tag, the bundled(crate(...)) Provides and
# LICENSE.dependencies name the crates that the two libraries link: their
# normal dependency tree, the set of the Debian copyright stanzas. %install
# copies those crates' licence files, which %license ships in the layout of
# the Arch package. cargo2rpm's
# vendor manifest would name every vendored crate, the dev and build ones too,
# and it cannot be narrowed.
RPM_SPEC = r'''Name:           $NAME
Version:        $VERSION
Release:        1%{?dist}
Summary:        Library to deal with pinyin — oxpinyin drop-in, $BACKEND store
License:        $LICENSE
URL:            $HOMEPAGE
Source0:        $SOURCE
Source1:        LICENSE.dependencies

BuildRequires:  cargo-rpm-macros
BuildRequires:  rust
BuildRequires:  cargo
BuildRequires:  cargo-c
BuildRequires:  make
BuildRequires:  gcc
BuildRequires:  gcc-c++
BuildRequires:  pkgconf
BuildRequires:  binutils
BuildRequires:  glib2-devel
BuildRequires:  $BUILD_DEPS

Requires:       libpinyin-data%{?_isa} >= 2.8.1-5.fc39
# Fedora's form for a package that replaces another: a versioned Obsoletes and
# Provides. The bound is one release past the pin version's first Fedora release,
# so the Fedora builds of the pin version are obsoleted. The Provides has no
# release, so it still satisfies libpinyin-data's Requires on libpinyin's exact
# version and release.
Obsoletes:      libpinyin < $PCVER-2, libzhuyin < $PCVER-2
Provides:       libpinyin = $PCVER, libzhuyin = $PCVER
$BUNDLED

%description
oxpinyin is an idiomatic Rust rewrite of libpinyin. This package installs
libpinyin.so.15 and libzhuyin.so.15 built against the $BACKEND store backend,
replacing libpinyin and libzhuyin in their entirety. It carries no data of its
own: the model files remain those of the installed libpinyin-data package.

%package devel
Summary:        Development files for oxpinyin ($BACKEND store)
Requires:       %{name}%{?_isa} = %{version}-%{release}
Obsoletes:      libpinyin-devel < $PCVER-2, libzhuyin-devel < $PCVER-2
Provides:       libpinyin-devel = $PCVER, libzhuyin-devel = $PCVER

%description devel
Headers, pkg-config files and linker symlinks for the oxpinyin drop-in
libpinyin.so.15 / libzhuyin.so.15.

%prep
%autosetup -n $TOPDIR
%cargo_prep -v vendor
cp -p %{SOURCE1} .

%build
%{make_build} -f tools/dev/packaging.mk BACKEND=$BACKEND PROFILE=rpm CARGO_FLAGS='--profile rpm --frozen' prefix=%{_prefix} libdir=%{_libdir}

%install
%{make_install} -f tools/dev/packaging.mk BACKEND=$BACKEND PROFILE=rpm CARGO_FLAGS='--profile rpm --frozen' prefix=%{_prefix} libdir=%{_libdir}
# Fedora's libpinyin-devel ships no static archives: its spec configures with
# --disable-static. Remove them by name, so any other unpackaged file still fails.
rm -- %{buildroot}%{_libdir}/libpinyin.a %{buildroot}%{_libdir}/libzhuyin.a
# The licence files of the crates that the libraries link, one directory per crate.
$LICENSE_INSTALL

%files
%license LICENSE LICENSE.dependencies crate-licenses/*
%{_libdir}/libpinyin.so.15*
%{_libdir}/libzhuyin.so.15*

%files devel
%{_includedir}/libpinyin-$PCVER/
%{_libdir}/libpinyin.so
%{_libdir}/libzhuyin.so
%{_libdir}/pkgconfig/libpinyin.pc
%{_libdir}/pkgconfig/libzhuyin.pc

%changelog
* $DATE $MAINTAINER - $VERSION-1
- Release build of the oxpinyin drop-in ($BACKEND store backend);
  replaces libpinyin/libzhuyin in their entirety.
'''

# makepkg's defaults apply: its build flags, strip, debug, lto and !staticlibs.
# The soname Provides are versioned by makepkg from the built libraries.
ARCH_BUILD = r'''pkgname=$PKGNAME
pkgver=$VERSION
pkgrel=1
pkgdesc='$DESC'
arch=($ARCH)
url='$HOMEPAGE'
license=($LICENSES)
depends=(glib2 gcc-libs $EXTRA_DEPS)
makedepends=(cargo cargo-c make pkgconf)
provides=(libpinyin=$PCVER libzhuyin=$PCVER libpinyin.so libzhuyin.so)
conflicts=(libpinyin libzhuyin)
source=('$SOURCE' '$DATA')
noextract=('$DATA')
sha256sums=('$SOURCE_SUM' '$DATA_SUM')

_packaging() {
  make -f tools/dev/packaging.mk BACKEND=$BACKEND CARGO_FLAGS='--release --frozen' \
    prefix=/usr libdir=/usr/lib "\$@"
}

build() {
  cd '$TOPDIR'
  _packaging
}

package() {
  cd '$TOPDIR'
  _packaging install DESTDIR="\$pkgdir"
  # Arch has no data package: the model lives inside the libpinyin package
  # that this one replaces, so it moves into this package.
  bsdtar -xf "\$srcdir/$DATA" -C "\$pkgdir" usr/lib/libpinyin/data
  test -f "\$pkgdir/usr/lib/libpinyin/data/table.conf"
  # The licences of the crates that the libraries link, with their copyright lines.
$LICENSE_INSTALL
}
'''

METADATA_USAGE = r'''Usage: just packaging check-pc-metadata --pin-root=DIR --subject-root=DIR
           --prefix=DIR [--libdir=DIR] [--includedir=DIR]
           [--build-subject=FEATURES] [--modules="libpinyin libzhuyin"]
  --build-subject  run the install for both libraries into --subject-root
                   first (emptying it), with `--profile dev --locked
                   --no-default-features --features FEATURES`; FEATURES is the backend
                   (bdb|kyotocabinet|tkrzw).
  --libdir / --includedir  the values BOTH sides were installed with;
                   omitted means autoconf's defaults (<prefix>/lib,
                   <prefix>/include), the tree layout either side produces
                   without the flag.

Exits 0 when every item matches; 1 on any difference (each one printed as a
unified diff); 2 on a usage error or a missing input.
'''

