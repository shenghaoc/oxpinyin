# Packaging oxpinyin as a C library (cargo-c)

This document records how `oxpinyin-capi` is packaged and installed as a
shared/static C library, and the decisions behind it. The installed tree
is a drop-in for libpinyin: consumers such as `ibus-libpinyin` link the
51-symbol `pinyin.h` surface exactly as they link upstream — `libpinyin.pc`,
`-lpinyin`, `libpinyin.so.15` — and need no source changes. The same
applies to `oxpinyin-zhuyin-capi` and `libzhuyin`.

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
the install layout and SONAME from Cargo metadata. The one thing cargo-c
cannot produce is a complete libpinyin `.pc` (see "Locating the model data"
below), which is why `tools/packaging/install.sh` wraps `cargo cinstall`.

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

ibus-libpinyin (and any other C consumer) detects the library exactly as it
detects upstream — the `.pc` name is `libpinyin`, not `oxpinyin`, and the
version it reports is libpinyin's, so existing `>=` constraints resolve:

```autoconf
PKG_CHECK_MODULES(LIBPINYIN, [libpinyin >= 2.11.91])
```

Nothing in the installed tree carries the `oxpinyin` or `pinyin_capi` name;
those exist only in the source tree and the Rust artifact names under
`target/`.

## Static library decision: ship it

cargo-c always builds a `.a` for a `staticlib` crate and has **no** metadata
toggle to suppress it; Debian's guidance notes packagers would otherwise need
a "not-installed" rule to drop it. Decision: **ship the static library**. It
adds negligible install size, is the cargo-c default, and removes per-packager
variance — every packager produces the same artifact set. It installs as
`libpinyin.a` beside the `.so`, and `libpinyin.pc`'s `Libs.private` lists the
platform libraries a static link needs
(`-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`).

## The four version streams

These are independent and move for different reasons:

1. **Crate version** — `0.1.0`, pre-1.0. The C surface is free to evolve after
   the first release, so the crate stays `0.x` and makes no semver-compat
   promise at the C-ABI level.
2. **`.so` SONAME** — `libpinyin.so.15` (`libzhuyin.so.15`), from
   `[package.metadata.capi.library] version = "15.0.0"`. This is upstream's
   own ABI number (`libpinyin_abi_current=15` in its configure.ac), and it
   is what makes the drop-in a drop-in: every consumer already records
   `libpinyin.so.15` in `DT_NEEDED`. It moves only when upstream bumps its
   ABI current — never with the crate version.
3. **Pinned oracle** — libpinyin `2.11.91`. Re-pinning the oracle is a
   deliberate event with its own re-freeze (`pin-refreeze-*.md` convention),
   independent of the crate version and SONAME.
4. **Parity pins** — `10190 / 10190 / 98930` of `98930` candidate symbols,
   `0` absent, `0` tie-swaps (per `pin-refreeze-2026-08.md`
   2026-08-22 amendment). These freeze the decode/predict output the
   oracle is held to; packaging must not move them.

Streams 1 and 2 are decoupled by construction: cargo-c would otherwise
derive the SONAME from the crate version (`0.x` → SONAME `libpinyin.so.0.x`,
moving on every minor bump), so `library.version` is set explicitly and
must stay at upstream's ABI number regardless of what the crate version
does. A package version (stream 1, the release tag) therefore never shows
up in a filename the dynamic linker reads.

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

Both recipes are the distro-documented shape; a real packaging must run
`tools/packaging/install.sh <libpinyin|libzhuyin> --prefix=/usr …` in place
of the bare `cargo cinstall` (or re-run it afterwards), because cargo-c's
own `.pc` lacks the variables consumers read — see the next section. The
release packages built by `release-packages.yml` go through exactly that
wrapper (below).

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

libpinyin's own `.pc` exports `pkgdatadir` (plus `database_format`,
`libpinyinincludedir` and `libpinyin_binary_version`), and consumers read
them — ibus-libpinyin's build resolves its system data directory from
`pkgdatadir` and refuses to configure without it. cargo-c cannot emit custom
pkg-config variables (its `[package.metadata.capi.pkg_config]` is a closed
seven-key set, verified against 0.10.24) and offers no way to opt out of
writing its own incomplete `libpinyin.pc`.

The contract is therefore carried outside cargo-c: each crate's build.rs
bakes a complete `.pc` template (`libpinyin.pc.in.baked`) with the
build-time fields, and `tools/packaging/install.sh` fills the install-time
placeholders and **overwrites** the file cargo-c installed. The result is
byte-for-byte the shape of upstream's `.pc` — every variable
`${prefix}`-derived, so `DESTDIR` relocation still works (below). A bare
`cargo cinstall` without the wrapper leaves the incomplete file; that
silent window and its gates are recorded in
`docs/findings/installed-naming.md`.

`pkgdatadir` follows upstream's convention exactly: it points one level
*above* the data, at `${libdir}/libpinyin`, and the model lives in
`${pkgdatadir}/data` (`table.conf`, the phrase/pinyin indexes, `bigram.db`,
the per-library chunk files). `pinyin_init`/`zhuyin_init` take that `data`
directory as their `systemdir` and fail closed (NULL) on a missing or
unreadable model — including a model in the wrong store format, which is
why each release lane is built under the one backend matching the distro's
data (next section).

**The library install ships no data of its own** apart from the Arch
release package: the distros' `libpinyin-data` (Debian, Fedora) stays in
place and is read as-is. Generating data with `oxpinyin-datagen` is a
separate deliverable, not part of `cargo cinstall`.

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
| Debian | `debian:testing` | tkrzw | Debian's libpinyin 2.11.91 (testing/forky) switched BerkeleyDB → Tkrzw (`libtkrzw1t64`); stable (trixie) still ships 2.8.1 on BerkeleyDB, which no backend reads |
| Fedora | `fedora:latest` | kyotocabinet | Fedora's libpinyin still links KyotoCabinet (`kyotocabinet-libs`) |
| Arch | `archlinux:latest` | kyotocabinet | Arch's libpinyin still links KyotoCabinet |

`tools/packaging/release-stage.sh` builds and stages through the supported
path: `tools/packaging/install.sh` (cargo cinstall plus the complete `.pc`)
once per library, with `--destdir` pointing at the staging root and the
backend selected by cargo-c's ordinary
`--no-default-features --features <backend>` (plus `shipped` on the pinyin
crate). cargo-c forwards both flags — its subcommands register cargo's
full feature argument set (checked in its source and exercised on 0.10.24
by building the kyotocabinet drop-in through it), so an earlier note here
saying it did not was wrong. Each lane uses its distro's own `cargo-c`
package, the one the distro's libpinyin packaging would use; rustc and
cargo still come from `rust-toolchain.toml`. The script then strips the
shared objects and re-gates the tree (the fixed file list of
`docs/findings/installed-naming.md`, SONAME, the five pkg-config reads real
consumers perform, and a C compile/link/run) before any packaging runs.

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
  pacman's form (`libpinyin.so=15-64`) and, via `--data=DIR`, the model
  directory installed as `/usr/lib/libpinyin/data`; `--data-version=VER`
  records the origin package version in the description, since Arch's data
  (2.10.x) is older than the 2.11.91 the library provides.

Data: Debian and Fedora keep `libpinyin-data` — a separate package there,
it stays installed through the takeover and is only a Recommends on ours.
Arch has no data package: the model lives inside the libpinyin package the
takeover removes, so the Arch lane downloads that package (`pacman -Sw`)
and ships its `usr/lib/libpinyin/data` inside ours — the same
KyotoCabinet-format files, under the same licence, at the same path
`pkgdatadir` already points above.

Each CI lane finishes by INSTALLING its own packages back into its build
container (plus `libpinyin-data` on Debian/Fedora) and re-running the
gates against `/usr` — the same five pkg-config reads, a C
compile/link/run, and a real `pinyin_init`/`zhuyin_init` on
`$(pkg-config --variable=pkgdatadir libpinyin)/data` that must return a
context — so a release never attaches a package that does not install,
does not answer as libpinyin/libzhuyin, or cannot open the system's model.
