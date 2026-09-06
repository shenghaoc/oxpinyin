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

`crates/oxpinyin-capi/Cargo.toml` carries the full contract. The installed
tree takes libpinyin's own binary identity — SONAME `libpinyin.so.15`,
`libpinyin.pc`, headers under `libpinyin-2.11.91/` — while the source tree
keeps ours; the full rationale and the measured gates live in
`docs/findings/installed-naming.md`. In short:

- `[lib] crate-type = ["cdylib", "staticlib", "rlib"]`. `staticlib` is
  required by cargo-c (it builds the `.a` alongside the `.so`); `rlib` is
  retained so `oxpinyin-dictool` can use the crate in-process.
- `[features] capi = []`. cargo-c identifies the crate to package by the
  presence of a `capi` feature; without it the crate is skipped.
  `[features] shipped = []` compiles out the fixture hooks no real consumer
  calls; it is enabled only for the shipped drop-in artifact.
- `[package.metadata.capi.header] generation = false`, `subdirectory =
  "libpinyin-2.11.91"`. `pinyin.h` and its two companion headers ship
  **verbatim** under libpinyin's version-stamped include subdirectory, never
  regenerated.
- `[package.metadata.capi.pkg_config] name = "libpinyin"`,
  `version = "2.11.91"`. The `.pc` answers to libpinyin's own name and a
  libpinyin version, so consumers' `>=` constraints resolve; cargo-c's own
  `.pc` is incomplete (closed seven-key field set) and is overwritten from
  the build.rs-baked template by `tools/packaging/install.sh`.
- `[package.metadata.capi.library] name = "pinyin"`, `version = "15.0.0"`,
  `versioning = true`. The INSTALLED artifact is the drop-in
  `libpinyin.so.15`; the Rust `[lib] name` stays `pinyin_capi` so in-tree
  gates keep finding `target/debug/libpinyin_capi.so`.

No `"."` first-member entry was needed: that cargo-c requirement applies only
when the exported crate is the workspace **root**. Here the crate is a normal
member (`crates/oxpinyin-capi`) and is selected via the `capi` feature plus
`-p oxpinyin-capi`.

## Consumer detection

The ibus-libpinyin fork (and any other C consumer) detects oxpinyin via
pkg-config using the `.pc` name `oxpinyin`:

```autoconf
PKG_CHECK_MODULES(LIBPINYIN, [oxpinyin])
```

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
4. **Parity pins** — `10190 / 10190 / 98930` of `98930` candidate symbols,
   `0` absent, `0` tie-swaps (per `pin-refreeze-2026-08.md`
   2026-08-22 amendment). These freeze the decode/predict output the
   oracle is held to; packaging must not move them.

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
find the store tables (`.kct` by default) and `interpolation2.text`.

**Limitation:** there is no `pkgdatadir` in `oxpinyin.pc`. Consumers locate the
data as `$(pkg-config --variable=prefix oxpinyin)/share/oxpinyin` (the default
cargo-c `datadir`), or via the standard data-search mechanism of the embedding
application. If a first-class data variable is ever required, it must be added
upstream to cargo-c or emitted by a small post-install `.pc` patch — do not
hand-write the `.pc` wholesale, as that would forfeit cargo-c's relocatable
`${prefix}`-derived paths.

Two consequences worth registering:

1. **The data files are not part of this install.** `cargo cinstall` ships only
   the `.so`/`.a`, `pinyin.h`, and `oxpinyin.pc`. The store tables and
   `interpolation2.text` come from the migrate/data deliverable and must be
   installed separately by the packager.
2. **The `share/oxpinyin` convention is unenforced.** Nothing installs into it
   today and nothing validates the path, so a generic consumer has no
   guaranteed data location. The fork sidesteps both gaps by passing an
   explicit `--with-oxpinyin-capi-datadir`; a first-class data variable must
   close them in a follow-up.

## Relocation

The generated `.pc` is fully `${prefix}`-derived — no baked absolute paths —
so a `DESTDIR` install relocates cleanly. `pkg-config --cflags --libs
libpinyin` returns only `-lpinyin` (plus glib from `Requires`) when the
prefix is a system path (`/usr`), because pkg-config elides
`-I/usr/include -L/usr/lib64`; `pkg-config --define-prefix` (or
`PKG_CONFIG_SYSROOT_DIR`, used by distro build roots) resolves the staged
paths. This is the standard DESTDIR relocation mechanism, not a defect.

## Release artifacts (`release-packages.yml`)

Every published GitHub release triggers
`.github/workflows/release-packages.yml`, which builds drop-in packages for
the three distro families and attaches them (plus a `SHA256SUMS`) to the
release with `gh release upload`.

Each lane builds under the ONE store backend whose model format that
distro's own `libpinyin` data is encoded in, so the drop-in reads the data
already on the system:

| lane | image | backend | because |
|---|---|---|---|
| Debian | `debian:latest` | tkrzw | Debian's libpinyin 2.11.91 switched BerkeleyDB → Tkrzw (`libtkrzw1t64`) |
| Fedora | `fedora:latest` | kyotocabinet | Fedora's libpinyin still links KyotoCabinet (`kyotocabinet-libs`) |
| Arch | `archlinux:latest` | kyotocabinet | Arch's libpinyin still links KyotoCabinet |

`tools/packaging/release-stage.sh` builds both cdylibs with plain
`cargo build --no-default-features --features <backend>,shipped` — NOT
`cargo cinstall` — because cargo-c does not forward `--no-default-features`
(verified against cargo-c 0.10.24), so the kyotocabinet lanes cannot select
their backend through it. Nothing is lost by building directly: build.rs
stamps the SONAMEs and bakes the complete `.pc` templates, the staged
layout is the fixed tree of `docs/findings/installed-naming.md`, and the
script re-gates it (SONAME, the five pkg-config reads real consumers
perform, and a C compile/link/run) before any packaging runs.

The per-distro makers wrap that staged tree in the shape the distro's real
libpinyin packaging uses, and every package takes the distro's
libpinyin/libzhuyin over **in their entirety** — same sonames, same
pkg-config names, versioned Provides at 2.11.91, and
Conflicts/Replaces (deb) / Obsoletes (rpm) / `conflicts=` (pacman) on the
originals:

- `tools/packaging/release-deb.sh` — `oxpinyin-libpinyin15-<backend>` +
  `-dev` (~ libpinyin15 + libzhuyin15, and their -dev packages). Depends is
  computed from the shipped ELF's DT_NEEDED via ldconfig + `dpkg -S`, so
  t64-era names (`libglib2.0-0t64`, `libtkrzw1t64`) resolve on whatever
  suite the build runs on, plus a `libc6 (>= N)` floor taken from the
  highest `GLIBC_*` symbol version referenced.
- `tools/packaging/release-rpm.sh` — `oxpinyin-libpinyin-<backend>` +
  `-devel` (~ libpinyin + libpinyin-devel). rpm's dependency generator
  emits the soname Provides (`libpinyin.so.15()(64bit)`) and the backend
  Requires automatically; Obsoletes (not Conflicts — a package may not
  conflict with a name it provides) performs the swap.
- `tools/packaging/release-arch.sh` — one `oxpinyin-libpinyin-<backend>`
  package, since Arch ships libpinyin undivided, with soname Provides in
  pacman's form (`libpinyin.so=15-64`).

No lane ships data. Debian and Fedora keep `libpinyin-data` — a separate
package there, it stays installed through the takeover and is only a
Recommends on ours. On Arch the data lives inside the libpinyin package the
takeover removes, and oxpinyin's own generated tables are not shippable
yet; users must restore `/usr/lib/libpinyin/data` from the Arch package
archive until that changes (see the caveat header of
`tools/packaging/release-arch.sh`).

Each CI lane finishes by INSTALLING its own packages back into its build
container and re-running the gates against `/usr` — the same five
pkg-config reads and a C compile/link/run — so a release never attaches a
package that does not install or does not answer as libpinyin/libzhuyin.
