# The Linux build and install of the two drop-in libraries (docs/packaging.md#export-gate).
#
# The release recipes run it from the distribution's own build driver:
# dh_auto_build and dh_auto_install, %make_build and %make_install, makepkg's
# build() and package(). `just packaging install` runs it on Linux too, so
# Linux has one link. It is GNU make, and it is for Linux only: the link is an
# ELF link with a version script.
#
#   make -f tools/dev/packaging.mk BACKEND=tkrzw prefix=/usr libdir=/usr/lib/x86_64-linux-gnu
#   make -f tools/dev/packaging.mk BACKEND=tkrzw prefix=/usr libdir=... install DESTDIR=...
#
# The build step runs cargo cinstall for each C crate into a scratch stage
# under the target directory: the cargo-c staticlib and the headers that the
# crate's metadata names. It then links lib<name>.so.15.0.0 from the staged
# archive with the crate's .ver script, as the pin's libtool link does, and
# verifies the export set, the version tag of each export and the SONAME
# before it keeps the object. The install step only copies: the staged
# archive and headers, the linked object and its two symlinks, and the
# complete .pc written from the crate's .pc.in, as the pin's config.status
# does.
#
# The build runs cinstall, not cbuild, so the Rust code compiles once:
# cargo-c 0.10 fingerprints the C crate differently for cbuild and cinstall,
# and for each destdir, so a cbuild followed by a cinstall compiles it twice.
# The stage path is fixed, so a second cinstall into it is fresh.
#
# The link takes the environment's CC and LDFLAGS: the distribution supplies
# its hardening there, and the link adds none. LDFLAGS follow the link's own
# options, as automake orders $(AM_LDFLAGS) $(LDFLAGS), so they can override
# one (-Wl,-z,nopack-relative-relocs), and precede the inputs, so
# --as-needed reaches the libraries. cc-rs reads CFLAGS and CXXFLAGS for the
# C++ bridges.

top_srcdir := $(abspath $(dir $(lastword $(MAKEFILE_LIST)))../..)

BACKEND ?=
CARGO ?= cargo
RUSTC ?= rustc
PKG_CONFIG ?= pkg-config
NM ?= nm
READELF ?= readelf
INSTALL ?= install
# Arguments for cargo cbuild and cargo cinstall: the profile and the lock mode.
CARGO_FLAGS ?= --release --locked
FEATURES ?= --no-default-features --features $(BACKEND)
# The profile's directory under the target triple: debug for the dev profile.
PROFILE ?= release
CARGO_TARGET_DIR ?= $(top_srcdir)/target
TRIPLE ?= $(shell $(RUSTC) -vV | sed -n 's/^host: //p')

prefix ?= /usr/local
# Only the command line sets libdir and includedir: a value inherited from the
# environment is not used. Without one, the .pc keeps autoconf's
# prefix-relative defaults, as the pin's configure writes them.
ifneq ($(origin libdir),command line)
libdir = $(prefix)/lib
pc_libdir = $${exec_prefix}/lib
else
pc_libdir = $(libdir)
endif
ifneq ($(origin includedir),command line)
includedir = $(prefix)/include
pc_includedir = $${prefix}/include
else
pc_includedir = $(includedir)
endif
DESTDIR ?=

LIBRARIES := libpinyin libzhuyin
libpinyin_crate ?= capi
libzhuyin_crate ?= zhuyin-capi
# The libtool current.revision of [package.metadata.capi.library].version.
BINARY_VERSION := 15.0
VERSION := $(shell sed -n 's/^libpinyin_tag=//p' $(top_srcdir)/tools/oracle/oracle-pin.txt)
DATABASE_FORMAT = $(DATABASE_FORMAT_$(BACKEND))
DATABASE_FORMAT_bdb := BerkeleyDB
DATABASE_FORMAT_kyotocabinet := KyotoCabinet
DATABASE_FORMAT_tkrzw := Tkrzw

ifeq ($(filter $(BACKEND),bdb kyotocabinet tkrzw),)
$(error BACKEND must be one of bdb, kyotocabinet or tkrzw, not '$(BACKEND)')
endif
ifeq ($(VERSION),)
$(error tools/oracle/oracle-pin.txt has no libpinyin_tag line)
endif

hash := \#
TARGET := $(abspath $(CARGO_TARGET_DIR))
# The staged trees, the copied archives and the linked objects. cargo-c's own
# outputs are one level up.
OUT := $(TARGET)/$(TRIPLE)/$(PROFILE)/packaging
# A crate's stage: its cinstall tree, under the install's prefix and libdir.
STAGE = $(OUT)/stage-$*

# glibc targets pack their relative relocations, as the cargo cdylibs of a
# `gnu` target do (capi/build.rs). The compiler decides, not the triple:
# Fedora's gcc reports x86_64-redhat-linux, with no -gnu.
RELOCS = $(shell printf '$(hash)include <features.h>\n' | $(CC) -dM -E -x c - 2>/dev/null \
  | grep -q '^$(hash)define __GLIBC__ ' && echo -Wl,-z,pack-relative-relocs)
GLIB_LIBS = $(shell $(PKG_CONFIG) --libs glib-2.0 gobject-2.0 2>/dev/null || echo -lglib-2.0 -lgobject-2.0)
BACKEND_LIBS_bdb = -ldb
BACKEND_LIBS_tkrzw = $(shell $(PKG_CONFIG) --libs tkrzw 2>/dev/null || echo -ltkrzw)
# The Kyoto Cabinet shim is C++: its pkg-config output can already name the runtime.
KYOTOCABINET_LIBS = $(shell $(PKG_CONFIG) --libs kyotocabinet 2>/dev/null || echo -lkyotocabinet)
BACKEND_LIBS_kyotocabinet = $(KYOTOCABINET_LIBS) $(if $(filter -lstdc++,$(KYOTOCABINET_LIBS)),,-lstdc++)
LIBS = $(GLIB_LIBS) $(BACKEND_LIBS_$(BACKEND)) -lpthread -ldl -lm

ARCHIVES := $(LIBRARIES:%=$(OUT)/%.a)
OBJECTS := $(LIBRARIES:%=$(OUT)/%.so.15.0.0)
INSTALLS := $(LIBRARIES:%=install-%)

# One cargo at a time: both crates share one target directory.
.NOTPARALLEL:
.PHONY: all install $(LIBRARIES) $(INSTALLS) FORCE
.DELETE_ON_ERROR:

all: $(OBJECTS)
$(LIBRARIES): %: $(OUT)/%.so.15.0.0
install: $(INSTALLS)
FORCE:

# cargo decides whether the crate is stale. The archive leaves the stage only
# when its content changed, so its timestamp tracks the Rust build.
$(ARCHIVES): $(OUT)/%.a: FORCE
	rm -rf '$(STAGE)'
	cd '$(top_srcdir)/$($*_crate)' && $(CARGO) cinstall --library-type staticlib \
	  --prefix='$(prefix)' --libdir='$(libdir)' --includedir='$(includedir)' \
	  --destdir='$(STAGE)' --target-dir='$(TARGET)' $(CARGO_FLAGS) $(FEATURES)
	cmp -s '$(STAGE)$(libdir)/$*.a' '$@' || cp -f '$(STAGE)$(libdir)/$*.a' '$@'

# Link beside the object, verify, then replace it. The link runs every time:
# it is cheap, and its other inputs (CC, LDFLAGS, the libraries) are not files.
.SECONDEXPANSION:
$(OBJECTS): $(OUT)/%.so.15.0.0: $(OUT)/%.a $$(top_srcdir)/$$($$*_crate)/$$*.ver FORCE
	$(CC) -shared -o '$@.tmp' -Wl,-z,defs -Wl,-soname,$*.so.15 \
	  -Wl,--version-script='$(top_srcdir)/$($*_crate)/$*.ver' -Wl,--gc-sections $(RELOCS) $(LDFLAGS) \
	  -Wl,--whole-archive '$<' -Wl,--no-whole-archive $(LIBS)
	@set -eu; ver='$(top_srcdir)/$($*_crate)/$*.ver'; object='$@.tmp'; \
	node=$$(sed -n 's/^\([A-Za-z_][A-Za-z0-9_]*\) {.*/\1/p' "$$ver" | head -n 1); \
	[ -n "$$node" ] || { echo "error: no version node found in $$ver" >&2; exit 1; }; \
	expected=$$(sed -n '/global:/,/local:/{s/global://;s/local://;p;}' "$$ver" | tr -d ' \t\n' | tr ';' '\n' | grep . | LC_ALL=C sort); \
	symbols=$$($(NM) -D --defined-only "$$object"); \
	exports=$$(printf '%s\n' "$$symbols" | awk -v node="$$node" 'NF >= 3 && !($$2 == "A" && $$3 == node) { print $$3 }'); \
	actual=$$(printf '%s\n' "$$exports" | sed 's/@.*//' | grep . | LC_ALL=C sort); \
	if [ "$$expected" != "$$actual" ]; then \
	  echo "error: the linked $*.so.15.0.0 exports differ from $$ver (- expected, + actual):" >&2; \
	  printf '%s\n' "$$expected" > "$$object.expected"; printf '%s\n' "$$actual" > "$$object.actual"; \
	  diff -u "$$object.expected" "$$object.actual" | sed -n '3,40p' >&2; \
	  rm -f "$$object.expected" "$$object.actual"; exit 1; fi; \
	unversioned=$$(printf '%s\n' "$$exports" | grep -v "@@$$node\$$" || true); \
	if [ -n "$$unversioned" ]; then \
	  echo "error: exported symbols without the default version @@$$node:" >&2; printf '  %s\n' $$unversioned >&2; exit 1; fi; \
	dynamic=$$($(READELF) -d "$$object"); \
	soname=$$(printf '%s\n' "$$dynamic" | sed -n 's/.*(SONAME).*\[\(.*\)\].*/\1/p'); \
	[ "$$soname" = '$*.so.15' ] || { echo "error: linked SONAME is '$${soname:-<none>}', expected $*.so.15" >&2; exit 1; }; \
	echo "linked $@: soname $$soname, version node $$node, $$(printf '%s\n' "$$expected" | wc -l) versioned exports (backend $(BACKEND))"
	mv -f '$@.tmp' '$@'

# The install copies the staged archive and headers and the linked object.
# cargo-c's incomplete .pc stays in the stage; the complete one is written here.
$(INSTALLS): install-%: $(OUT)/%.so.15.0.0
	$(INSTALL) -d '$(DESTDIR)$(libdir)' '$(DESTDIR)$(includedir)'
	$(INSTALL) -m 0644 '$(OUT)/$*.a' '$(DESTDIR)$(libdir)/$*.a'
	cp -R '$(STAGE)$(includedir)/.' '$(DESTDIR)$(includedir)/'
	# cargo-c copies each header with its checkout mode; the pin installs them 0644.
	find '$(DESTDIR)$(includedir)/libpinyin-$(VERSION)' -type f -exec chmod 0644 {} +
	$(INSTALL) -d '$(DESTDIR)$(libdir)/pkgconfig'
	$(INSTALL) -m 0755 '$<' '$(DESTDIR)$(libdir)/$*.so.15.0.0'
	ln -sf '$*.so.15.0.0' '$(DESTDIR)$(libdir)/$*.so.15'
	ln -sf '$*.so.15.0.0' '$(DESTDIR)$(libdir)/$*.so'
	# One sed, no pipeline: a failed render stops here and never replaces the .pc.
	# The header's comment lines go first, then the blank lines before the body.
	sed -e 's/\r$$//' -e '/^[[:space:]]*$(hash)/d' -e '/./,$$!d' \
	  -e 's|@VERSION@|$(VERSION)|g' -e 's|@LIBPINYIN_BINARY_VERSION@|$(BINARY_VERSION)|g' \
	  -e 's|@DATABASE_FORMAT@|$(DATABASE_FORMAT)|g' -e 's|@prefix@|$(prefix)|g' \
	  -e 's|@exec_prefix@|$${prefix}|g' -e 's|@libdir@|$(pc_libdir)|g' -e 's|@includedir@|$(pc_includedir)|g' \
	  '$(top_srcdir)/$($*_crate)/$*.pc.in' > '$(DESTDIR)$(libdir)/pkgconfig/$*.pc.tmp'
	mv -f '$(DESTDIR)$(libdir)/pkgconfig/$*.pc.tmp' '$(DESTDIR)$(libdir)/pkgconfig/$*.pc'
	@echo 'installed complete pkg-config file: $(DESTDIR)$(libdir)/pkgconfig/$*.pc'
