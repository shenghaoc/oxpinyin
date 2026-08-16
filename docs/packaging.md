# Packaging oxpinyin as a C library (cargo-c)

This document records how `oxpinyin-capi` is packaged and installed as a
shared/static C library, and the decisions behind it. The consumer is the
maintainer's `ibus-libpinyin` fork, which links against the 51-symbol
`pinyin.h` surface and `-lpinyin_capi`.

## Why cargo-c

`oxpinyin-capi` is a `cdylib` that must be installed like any other C library:
a versioned `.so`, an unversioned linker symlink, `pinyin.h`, and a
`pkg-config` file. `cargo-c` is the tooling both distro families document for
exactly this job:

- **Fedora** ships `%cargo_cbuild` / `%cargo_cinstall` RPM macros in the
  `cargo-c` package.
- **Debian's Rust Team Book** prescribes `Build-Depends: cargo-c:native` and
  `cargo cbuild` / `cargo cinstall` in `debian/rules`, with
  `--libdir=/usr/lib/${DEB_HOST_MULTIARCH}`.

No hand-written `Makefile` is needed: `cargo cbuild`/`cargo cinstall` derive
the install layout, SONAME, and `.pc` file from Cargo metadata.

## Metadata on `oxpinyin-capi`

`crates/oxpinyin-capi/Cargo.toml` carries the full contract:

- `[lib] crate-type = ["cdylib", "staticlib", "rlib"]`. `staticlib` is
  required by cargo-c (it builds the `.a` alongside the `.so`); `rlib` is
  retained so `oxpinyin-dictool` can use the crate in-process.
- `[features] capi = []`. cargo-c identifies the crate to package by the
  presence of a `capi` feature; without it the crate is skipped.
- `[package.metadata.capi.header] generation = false` and
  `subdirectory = ""`. `pinyin.h` is shipped **verbatim** — it is
  byte-identical to what the fork compiles against, and that property must
  survive packaging. It is installed to `$includedir/pinyin.h` with no
  regeneration.
- `[package.metadata.capi.install.include] asset = [{ from = "pinyin.h",
  to = "" }]`. cargo-c's pre-generated-header asset mechanism, keeping the
  file where it lives today (the crate root).
- `[package.metadata.capi.pkg_config] name = "oxpinyin"`,
  `filename = "oxpinyin"`. The crate is `oxpinyin-capi` and the library
  `pinyin_capi`, but the `.pc` is `oxpinyin` — consumers depend on
  `oxpinyin`, not the crate's internal hyphenated name.
- `[package.metadata.capi.library] name = "pinyin_capi"`,
  `version = "0.1.0"`, `versioning = true`. Pins the `.so` basename to
  `pinyin_capi` (the fork links `-lpinyin_capi`) and produces the SONAME
  `libpinyin_capi.so.0.1` (see below).

No `"."` first-member entry was needed: that cargo-c requirement applies only
when the exported crate is the workspace **root**. Here the crate is a normal
member (`crates/oxpinyin-capi`) and is selected via the `capi` feature plus
`-p oxpinyin-capi`.

## Static library decision: ship it

cargo-c always builds a `.a` for a `staticlib` crate and has **no** metadata
toggle to suppress it; Debian's guidance notes packagers would otherwise need
a "not-installed" rule to drop it. Decision: **ship the static library**. It
adds negligible install size, is the cargo-c default, and removes per-packager
variance — every packager produces the same artifact set. `oxpinyin.pc`'s
`Libs.private` already lists the platform libraries a static link needs
(`-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`).

## The four version streams

These are independent and move for different reasons:

1. **Crate version** — `0.1.0`, pre-1.0. The C surface is free to evolve after
   the first release, so the crate stays `0.x` and makes no semver-compat
   promise at the C-ABI level.
2. **`.so` SONAME** — `libpinyin_capi.so.0.1`. Bumps only on a **deliberate
   C-ABI break**. This is what protects the 51-symbol bootstrap contract: the
   fork links `-lpinyin_capi` and resolves `libpinyin_capi.so.0.1`; bumping the
   SONAME is the mechanism that makes an ABI break visible to the dynamic
   linker rather than silently corrupting the fork.
3. **Pinned oracle** — libpinyin `2.11.91`. Re-pinning the oracle is a
   deliberate event with its own re-freeze (`pin-refreeze-*.md` convention),
   independent of the crate version and SONAME.
4. **Parity pins** — `10177 / 10189 / 94871` of `98930` candidate symbols,
   `1` absent, `1036` tie-swaps (per `pin-refreeze-2026-08.md`). These freeze
   the decode/predict output the oracle is held to; packaging must not move
   them.

**Coupling caveat at 0.x:** streams 1 and 2 are not yet fully independent.
cargo-c derives the SONAME from `library.version` (defaulting to the crate
version), mapping `X.Y.Z` → SONAME `X.Y` and real file `X.Y.Z`. Because the
crate is `0.x`, the SONAME tracks the *minor* version: a `0.1.0` → `0.2.0`
bump (and a matching `library.version`) changes the SONAME to
`libpinyin_capi.so.0.2`, breaking the fork's dynamic link. At 1.0, decide
whether to hold `library.version` at a stable value so the SONAME bumps only
on a deliberate C-ABI break.

## Fedora recipe

```specfile
BuildRequires: cargo-c
# ...
%build
%cargo_cbuild
%install
%cargo_cinstall
```

`cargo cinstall` derives `--libdir=/usr/lib64` from the target environment.

## Debian recipe

```makefile
# debian/control
Build-Depends: cargo-c:native
```

```makefile
# debian/rules
override_dh_auto_build:
	cargo cbuild

override_dh_auto_install:
	cargo cinstall --destdir=$(CURDIR)/debian/tmp --prefix=/usr \
	  --libdir=/usr/lib/$(DEB_HOST_MULTIARCH)
```

## Locating the model data (`pkgdatadir`)

libpinyin's own `.pc` exports `pkgdatadir` so consumers can find its data
directory. cargo-c does **not** support custom pkg-config variables, so
`oxpinyin.pc` carries only `prefix/exec_prefix/libdir/includedir` plus the
standard `Name/Description/Version/Libs/Cflags/Requires`. Since #84 makes
`pinyin_init` fail closed on a missing model, consumers need another way to
find the `.redb` tables and `interpolation2.text`.

**Limitation:** there is no `pkgdatadir` in `oxpinyin.pc`. Consumers locate the
data as `$(pkg-config --variable=prefix oxpinyin)/share/oxpinyin` (the default
cargo-c `datadir`), or via the standard data-search mechanism of the embedding
application. If a first-class data variable is ever required, it must be added
upstream to cargo-c or emitted by a small post-install `.pc` patch — do not
hand-write the `.pc` wholesale, as that would forfeit cargo-c's relocatable
`${prefix}`-derived paths.

Two consequences worth registering:

1. **The data files are not part of this install.** `cargo cinstall` ships only
   the `.so`/`.a`, `pinyin.h`, and `oxpinyin.pc`. The `.redb` tables and
   `interpolation2.text` come from the migrate/data deliverable and must be
   installed separately by the packager.
2. **The `share/oxpinyin` convention is unenforced.** Nothing installs into it
   today and nothing validates the path, so a generic consumer has no
   guaranteed data location. The fork sidesteps both gaps by passing an
   explicit `--with-oxpinyin-capi-datadir`; a first-class data variable must
   close them in a follow-up.

## Relocation

The generated `.pc` is fully `${prefix}`-derived — no baked absolute paths —
so a `DESTDIR` install relocates cleanly. `pkg-config --cflags --libs oxpinyin`
returns only `-lpinyin_capi` when the prefix is a system path (`/usr`), because
pkg-config elides `-I/usr/include -L/usr/lib64`; `pkg-config --define-prefix`
(or `PKG_CONFIG_SYSROOT_DIR`, used by distro build roots) resolves the staged
paths. This is the standard DESTDIR relocation mechanism, not a defect.
