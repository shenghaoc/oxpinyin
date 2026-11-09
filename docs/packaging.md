# Packaging oxpinyin as a C library (cargo-c)

This document records how `oxpinyin-capi` is packaged and installed as a
shared/static C library, and the decisions behind it. The installed tree
is a drop-in for libpinyin: consumers such as `ibus-libpinyin` link the
79-symbol `pinyin.h` surface exactly as they link upstream — `libpinyin.pc`,
`-lpinyin`, `libpinyin.so.15` — and need no source changes. The same
applies to `oxpinyin-zhuyin-capi` and `libzhuyin`.

## Trainer utility layout

Peng Wu's [epico/trainer](https://github.com/epico/trainer/tree/b1927376735c0c1042d72af2eca0e25c53595724)
at `b1927376735c0c1042d72af2eca0e25c53595724` changes into a libpinyin
tree's `data/` directory before invoking utilities. Build the tools with the
[native setup](runbooks/setup.md), then stage two new trees:

```sh
just build --locked -p oxpinyin-utils --bins
just trainer-layout /absolute/path/trainer-tools --bin-dir target/debug
just trainer-layout /absolute/path/trainer-evals --bin-dir target/debug
```

The recipe delegates to `tools.dev.packaging`. It copies all 21 pin-named
utilities into `utils/storage/`, `utils/segment/` and `utils/training/` and
writes `data/Makefile`. Copies preserve executable permissions and survive
Cargo cleanup; native shared-library dependencies must still be available.
The destination must not exist. `--bin-dir` selects another build/profile
directory; its default is `$CARGO_TARGET_DIR/debug` or `target/debug`.
Nothing is installed system-wide and no trainer code or model data is copied.

In the trainer's existing `lib/myconfig.py` configuration, set `m_tools_dir`
(returned by `getToolsDir()`) to `/absolute/path/trainer-tools` and
`m_evals_dir` (returned by `getEvalsDir()`) to `/absolute/path/trainer-evals`.
Use the tree roots, not their `data/` subdirectories. Keep them separate:
evaluation replaces `interpolation2.text` and regenerates its binary data.
Populate each `data/` with the model's textual tables, `interpolation2.text`
and a configured `table.conf` matching the tools' DBM backend, then run
`make -C /absolute/path/trainer-tools/data` (and likewise for the eval tree).
Supply the trainer's held-out inputs too: tools `data/estimates.db`, and eval
`data/deleted_bigram.db` and `data/evals2.text`. This recipe stages a layout;
it does not prepare a corpus, download model20 or run the trainer.

The generated Makefile provides the trainer's default build, `clean` and
`modify LAMBDA_PARAMETER=...` targets. Build and clean use the pin's file
lists and the exact relative utility calls from
[`data/Makefile.am:18–69` at `074a2219c90feaf962d0d24f034514033ece5f99`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/data/Makefile.am#L18-L69).
`modify` keeps the first `table.conf` as `data/.trainer-table.conf` and
reapplies the pin's lambda substitution to that baseline on every call.
It does not run the pin's checkout-wide `git reset --hard` in a caller's
directory. Remove that baseline when deliberately replacing the configuration.
The trainer requires `/usr/bin/make`; the staged Makefile uses `rm`, `cp`
and `sed`. The pin's optional model-download rule (`wget`, `tar`) is not
staged: supply the data first.

The complete reached-utility inventory follows. Paths are relative to `data/`;
each maps to the **same basename** in `oxpinyin-utils`. All 15 are ported.

| Trainer source at `b1927376` / pin make rule | Relative utility path |
|---|---|
| `segment.py:32,35` | `../utils/segment/spseg`, `../utils/segment/ngseg` |
| `mergeseq.py:35` | `../utils/segment/mergeseq` |
| `generate.py:32` | `../utils/training/gen_k_mixture_model` |
| `estimate.py:39` | `../utils/training/estimate_k_mixture_model` |
| `tryprune.py:22` | `../utils/training/validate_k_mixture_model` |
| `tryprune.py:35` | `../utils/training/export_k_mixture_model` |
| `tryprune.py:53` | `../utils/training/k_mixture_model_to_interpolation` |
| `tryprune.py:78` | `../utils/training/merge_k_mixture_model` |
| `tryprune.py:119` | `../utils/training/prune_k_mixture_model` |
| `evaluate.py:59,101` | `../utils/training/estimate_interpolation`, `../utils/training/eval_correction_rate` |
| `evaluate.py:45` → pin `data/Makefile.am:60–62` | `../utils/storage/gen_binary_files`, `../utils/storage/import_interpolation`, `../utils/training/gen_unigram` |

`gen_pinyin_table` appears only in trainer helper comments, not as a call.
The six additional staged pin utilities are storage `gen_pinyin_table`,
`gen_zhuyin_table`, `export_interpolation`, and training `gen_ngram`,
`gen_deleted_ngram`, `import_k_mixture_model`. There are no missing utility
ports in this inventory; the trainer's own Python scripts remain external.

Run `just test-tools` for the packaging fixtures. After the build above,
run the explicit real-tool integration test in the same Python test file:

```sh
uv run --no-project --managed-python python -X lazy_imports=normal tools/dev/packaging.test.py TrainerLayoutTests.check_real_tools
```

It checks every reached path and runs `../utils/training/gen_unigram --help`
from the created `data/`. Set `OXPINYIN_TRAINER_BIN_DIR` for another binary
directory. Missing tools fail this test; it does not substitute fixtures.

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
below), which is why `just packaging install` wraps `cargo cinstall`.
Run from the checkout with the [ordinary native setup](runbooks/setup.md);
`just python` provisions its managed interpreter. The packaging commands
run builds, relinks and package builders supervised, with cancellation handling.

## Metadata on `oxpinyin-capi`

`crates/oxpinyin-capi/Cargo.toml` carries the full contract. The installed
tree takes libpinyin's own binary identity — SONAME `libpinyin.so.15`,
`libpinyin.pc`, headers under `libpinyin-2.11.92/` — while the source tree
keeps ours; the full rationale and the measured gates live in
`docs/findings/installed-naming.md`. In short:

- `[lib] crate-type = ["cdylib", "staticlib", "rlib"]`. `staticlib` is
  required by cargo-c (it builds the `.a` alongside the `.so`); `rlib` is
  retained so the fuzz targets and Rust tests can use the crate in-process.
- `[features] capi = []`. cargo-c identifies the crate to package by the
  presence of a `capi` feature; without it the crate is skipped.
  `[features] shipped = []` compiles out the fixture hooks no real consumer
  calls; it is enabled only for the shipped drop-in artifact.
- `[package.metadata.capi.header] generation = false`, `subdirectory =
  "libpinyin-2.11.92"`. `pinyin.h` and its two companion headers ship
  **verbatim** under libpinyin's version-stamped include subdirectory, never
  regenerated.
- `[package.metadata.capi.pkg_config] name = "libpinyin"`,
  `version = "2.11.92"`. The `.pc` answers to libpinyin's own name and a
  libpinyin version, so consumers' `>=` constraints resolve; cargo-c's own
  `.pc` is incomplete (closed seven-key field set) and is overwritten from
  the build.rs-baked template by `just packaging install`.
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
PKG_CHECK_MODULES(LIBPINYIN, [libpinyin >= 2.11.92])
```

Nothing in the installed tree carries the `oxpinyin` or `pinyin_capi` name;
those exist only in the source tree and the Rust artifact names under
`target/`.

## User data across the takeover

An oxpinyin lane is built on the same backend as the distro's libpinyin
— that is the point of the lane table above — and keeps the user dir as
that backend's native user files, exactly as the libpinyin build it
replaces does (maintainer ruling 2026-09-09). The profile in question is
the one ibus-libpinyin keeps under `~/.cache/ibus/libpinyin/` — the
directory the [libpinyin FAQ](https://github.com/libpinyin/libpinyin/wiki/libpinyin-FAQ)
tells users to reset by deleting its `user.conf`:

- **Same backend: nothing changes.** `user.conf`, `user_bigram.db`,
  `user_pinyin_index.bin`, `user_phrase_index.bin`, `user.bin` and the
  `*.dbin` logs are the backend's native files. The Kyoto Cabinet lanes
  (Fedora, Arch) keep the same ones a KC-built libpinyin keeps; the
  tkrzw lane (Debian) the same ones a tkrzw-built libpinyin keeps.
  The recorded evidence is the round trip against pin-built oracles in
  [user-store §11](findings/user-store.md#11-implementation-record--the-store-persists-in-libpinyins-own-files-2026-09-09).
- **Different backend: fresh start, as with libpinyin.** A build on
  another backend cannot read the user dir and starts fresh: libpinyin's
  tkrzw build cannot read its Berkeley DB user files either
  (`check_format`; Debian's BerkeleyDB→Tkrzw switch carried a
  `debian/NEWS` warning). A distro whose users cross a backend boundary
  by installing a takeover package should carry the same
  `debian/NEWS`-style notice. Users who want to keep their phrases can
  use their frontend's dictionary export and import (ibus-libpinyin's
  Import/Export) — libpinyin's ordinary ABI, which carries the user
  phrase dictionary but not the trained bigram.

## Static library decision: ship it

cargo-c always builds a `.a` for a `staticlib` crate and has **no** metadata
toggle to suppress it; Debian's guidance notes packagers would otherwise need
a "not-installed" rule to drop it. Decision: **ship the static library**. It
adds negligible install size, is the cargo-c default, and removes per-packager
variance — every packager produces the same artifact set. It installs as
`libpinyin.a` beside the `.so`. The `.pc` files mirror the pin's and have no
`Libs.private`. A static link against `libpinyin.a` must pass the platform
libraries itself (`-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`), plus the
selected backend library.

## The four version streams

These are independent and move for different reasons:

1. **Crate version** — `0.1.0`, pre-1.0. The C surface is free to evolve after
   the first release, so the crate stays `0.x` and makes no semver-compat
   promise at the C-ABI level. It makes none for Rust either: native Rust
   interfaces are not supported externally ([interface ruling](constitution.md#external-interfaces)).
2. **`.so` SONAME** — `libpinyin.so.15` (`libzhuyin.so.15`), from
   `[package.metadata.capi.library] version = "15.0.0"`. This is upstream's
   own ABI number (`libpinyin_abi_current=15` in its configure.ac), and it
   is what makes the drop-in a drop-in: every consumer already records
   `libpinyin.so.15` in `DT_NEEDED`. It moves only when upstream bumps its
   ABI current — never with the crate version.
3. **Pinned oracle** — libpinyin `2.11.92`. Re-pinning the oracle is a
   deliberate event with its own re-freeze
   ([goldens and pins](runbooks/goldens-and-pins.md#3-the-oracle-pin)), independent of the crate version and SONAME. The drop-in's installed
   identity — the `.pc` `Version:`, the `libpinyin-<version>/` header
   subdirectory, the packages' versioned Provides — is the pin's own
   `configure.ac` version and moves with it: its one source is
   `libpinyin_tag` in `tools/oracle/oracle-pin.txt`, which both capi build
   scripts read (and check their `[package.metadata.capi]` tables against)
   and `just packaging pc-version` prints for package assembly.
4. **Parity pins** — `10190 / 10190 / 98930` of `98930` candidate symbols,
   `0` absent, `0` tie-swaps
   ([frozen parity numbers](runbooks/goldens-and-pins.md#2-the-frozen-parity-numbers)). These freeze the decode/predict output the
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
`just packaging install <libpinyin|libzhuyin> --prefix=/usr …` in place
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

## Kyoto Cabinet: build headers must match the installed library

The Kyoto Cabinet backend compiles Kyoto Cabinet's header code into the
facade, as libpinyin's Kyoto Cabinet build does. The cxx bridge's adapter
(`crates/oxpinyin-store/src/kyotocabinet/adapter.cc`) constructs `TreeDB`,
`HashDB`, `ProtoTreeDB` and `StashDB` from the installed headers. That
compiled code calls the system libkyotocabinet's out-of-line functions and
shares its object layouts, so the headers a package builds against must be
the same Kyoto Cabinet version as the libkyotocabinet it runs with. Build
against the distribution's `libkyotocabinet-dev` or `kyotocabinet-devel`
for the `libkyotocabinet` it ships, and rebuild the facade whenever that
library's version changes, as libpinyin's package must be rebuilt.
The backend checks only that the running library is 1.2.x
(`check_runtime_version` in `crates/oxpinyin-store/src/kyotocabinet/ffi.rs`);
it does not detect a header/library mismatch.

## Locating the model data (`pkgdatadir`)

libpinyin's own `.pc` exports `pkgdatadir` (plus `database_format`,
`libpinyinincludedir` and `libpinyin_binary_version`), and consumers read
them — ibus-libpinyin's build resolves its system data directory from
`pkgdatadir` and refuses to configure without it. cargo-c cannot emit custom
pkg-config variables (its `[package.metadata.capi.pkg_config]` is a closed
seven-key set) and offers no way to opt out of
writing its own incomplete `libpinyin.pc`.

The contract is therefore carried outside cargo-c: each crate's build.rs
bakes a complete `.pc` template (`libpinyin.pc.in.baked`) with the
build-time fields, and `just packaging install` fills the install-time
placeholders and **overwrites** the file cargo-c installed. Both templates
are the pin's own `.pc.in`, and the wrapper substitutes what the pin's
configure does — `exec_prefix=${prefix}`, and `libdir` / `includedir` as
the given `--libdir` / `--includedir` verbatim or, without them, autoconf's
prefix-relative `${exec_prefix}/lib` / `${prefix}/include` — so the
installed file is byte-identical to the pin's for the same layout and
backend (`just packaging check-pc-metadata` diffs the two trees). A bare
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
place and is read as-is. Generating data with the pin's program sequence
from `oxpinyin-utils` is a separate deliverable, not part of `cargo cinstall`.

## Relocation

The generated `.pc` is fully `${prefix}`-derived — no baked absolute paths —
so a `DESTDIR` install relocates cleanly. `pkg-config --cflags --libs
libpinyin` returns only `-lpinyin` (plus glib from `Requires`) when the
prefix is a system path (`/usr`), because pkg-config elides
`-I/usr/include -L/usr/lib64`; `pkg-config --define-prefix` (or
`PKG_CONFIG_SYSROOT_DIR`, used by distro build roots) resolves the staged
paths. This is the standard DESTDIR relocation mechanism, not a defect.

## Drop-in identity

A consumer loads the replacement with `dlopen("libpinyin.so.15")`, or
through `DT_NEEDED`, with no source change. Three properties decide
whether that works.

**SONAME.** The pin's `configure.ac` sets `libpinyin_abi_current = 15` and
`libpinyin_abi_revision = 0`. libtool's `-version-info 15:0` gives the SONAME
`libpinyin.so.15`, and a consumer records that string in `DT_NEEDED`.
`crates/oxpinyin-capi/build.rs` passes it as a `cdylib` link argument;
`[package.metadata.capi.library]` `name = "pinyin"` / `version = "15.0.0"`
names the installed file. `libzhuyin.so.15` follows the same rule.

**Symbol versioning.** The pin defines its symbols under the `LIBPINYIN`
version node (`pinyin_init@@LIBPINYIN`), so a linked consumer records
`File: libpinyin.so.15 → Name: LIBPINYIN` in `.gnu.version_r`. Three library
shapes behave differently against that reference:

| Library shape | Outcome |
| --- | --- |
| No version definitions | loads and runs; glibc warns `no version information available` once per start |
| Symbols versioned under `LIBPINYIN` | loads and runs clean, the pin's shape |
| Version definitions present, symbols unversioned | **hard failure**: `version 'LIBPINYIN' not found` |

rustc always gives a cdylib its own anonymous version script, and a second,
named script cannot be combined with it. GNU ld rejects the pair. rust-lld
accepts the definitions but does not reassign the symbols, which gives the
third shape. So the cargo-built cdylib has no version script and is the
first shape. On Linux, `just packaging install` relinks the installed object
from the staticlib with `-Wl,-soname` and the crate's `.ver` script, which
gives the second shape (see [Export gate](#export-gate)). `libzhuyin.so.15`
uses `LIBZHUYIN` and its 52 symbols the same way.

**Symbol scope.** The shipped library exports every symbol in its `.ver`
script and nothing else. The `shipped` feature compiles out the fixture
hooks (`oxpinyin_init_for_fixtures` and `oxpinyin_test_set_user_bigram`).
The feature is subtractive: an additive, default-on hook feature would ride
on the default features that also select the backend, so every
non-default-backend build would drop the hooks.

## Export gate

`just abi exports` is the mechanical form of the drop-in promise on the
symbol side (`check_exports` in `tools/dev/abi.py`). It proves that the set
of exported `pinyin_*` / `zhuyin_*` symbols is exactly the pin's version
script — `crates/oxpinyin-capi/libpinyin.ver` (79) and
`crates/oxpinyin-zhuyin-capi/libzhuyin.ver` (52), both checked in verbatim
from the pin — with nothing missing and nothing extra, and that no other
symbol leaks. CI runs it twice on every change: on the development build,
where the two `oxpinyin_*` fixture hooks the ABI tests drive are the only
permitted extras, and with `--shipped`, the packaged artifact, where only
the version script's set may remain. No symbol in a version script is
feature-gated.

On Linux `just packaging install` relinks the installed shared object from
the staticlib under the crate's `.ver` script
(`just packaging relink-versioned`), with `-Wl,-z,defs`, so the shipped
library carries the pin's symbol versioning. The relink verifies the export
set and the version tag of every export against the `.ver` list in both
directions before it replaces the installed file. It keeps the same file
name, so cargo-c's symlinks keep resolving. `just abi exports` compares the
cdylib that cargo builds (names, not versions), which is the gate for the
development artifact.

On macOS, `just packaging install` also relinks from the cargo-c staticlib
(`relink_darwin` in `tools/dev/packaging.py`). The Apple linker rejects
`--exclude-libs`, cannot combine nonempty exported and unexported lists, and
takes the union of two exported lists. A second list on rustc's cdylib link
would therefore add exports, not restrict them. The relink uses one
`-exported_symbols_list`, derived from the `.ver` globals with each name
prefixed by `_`, so the restriction holds for every archive symbol,
including the transitive `#[export_name]` symbols. The replacement keeps
cargo-c's install name, compatibility and current versions, architecture
and deployment target. Before the atomic replacement, both `nm -gU` (all
external defined types, including data, absolute and weak symbols) and the
export trie (`dyld_info -exports`) must equal the `.ver` set. The installed
native dylib must then load and resolve its entry point. The nightly
`test-macos` job provisions cargo-c, stages both release libraries and runs
`just abi exports --shipped` with `OXPINYIN_CAPI_SO` and `OXPINYIN_ZHUYIN_SO`
pointing to those installed files. Without overrides, that command checks
Cargo development artifacts with fixture hooks disabled. The packaging and
ABI tests in `tools/dev/packaging.test.py` and `tools/dev/abi.test.py`
cover identity preservation, failure atomicity and leak detection.

The tkrzw and Kyoto Cabinet backends' cxx bridges link the `cxx` crate,
whose Rust runtime defines `#[export_name]` symbols (`cxxbridge1$slice$*`,
`cxxbridge1$string$*`, `cxxbridge1$exception`, ...). rustc lists every such
upstream symbol as a C-level export of a `cdylib`, so without a
counter-measure the cargo-built cdylibs export the whole runtime. Both
relinks above exclude them from the shipped libraries: the Linux version
script's `local: *` and the macOS `-exported_symbols_list`. For the
development cdylibs the gate checks, both capi build scripts pass
`-Wl,--exclude-libs,ALL` to the Linux cdylib link when the tkrzw or
kyotocabinet feature is on: GNU ld gives every symbol defined in an archive —
the rlibs and the bridge's static library — hidden visibility, which rustc's
anonymous version script cannot override, while the `pinyin_*` and
`zhuyin_*` symbols come from the capi crates' own object files and are
untouched. No such flag exists on macOS, where the development cdylibs
carry the extra symbols; `just abi exports` there is run against the
relinked installed files, as the nightly job does.

## Function layout

The compiler and linker control function layout. oxpinyin does not keep function-order policies or enforce function placement.
Source, dependency and toolchain changes need no policy regeneration.
Do not add PGO, BOLT, linker call-graph sorting or another layout system.

Linux keeps the versioned relink described in [Export gate](#export-gate), including the version script and `-Wl,-z,defs`.
macOS uses the export-controlled relink described in [Export gate](#export-gate); `just packaging install` then loads the replaced file with `dlopen` and looks up `pinyin_init` or `zhuyin_init`, and fails if either step fails.
On macOS, install with `--prefix=/usr/local` or another non-system prefix. A `/usr` prefix gives the dylibs `/usr/lib` install names, which macOS reserves for its shared cache; the link then fails with "Shared cache eligible dylib cannot link to ineligible dylib" against a library outside the shared cache, such as Homebrew's tkrzw.
A target whose architecture differs from the running process's (`--target x86_64-apple-darwin` on Apple silicon) is not loaded: the install prints one line saying the load check was skipped.
The load check catches a dylib that dyld refuses, such as one with a misaligned `LINKEDIT` string pool ([rust-lang/rust#157750](https://github.com/rust-lang/rust/issues/157750)).
The release profile keeps fat LTO and one codegen unit.
Export verification, backend identification, artifact provenance, input-digest checks and runtime compatibility checks still apply.
This internal Stage 2 change must preserve exported symbols, symbol versions, the C ABI, outputs and data files.

## Shipping-library sizes

[Constitution §2](constitution.md) requires the stripped sizes of the shipping libraries in each change that affects them.
The `shipping-sizes` job in `.github/workflows/ci.yml` produces them.
Lanes take the numbers from its job summary.
Lanes do not build release libraries locally for this purpose.

**When the job runs.**
It runs on a pull request or a push to main when a file that can change a shipping library changes.
Those files are the twelve crates in the dependency graph of `oxpinyin-capi` and `oxpinyin-zhuyin-capi`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/`, and the packaging tooling.
The job also runs when its own files change.
`tools/dev/ship_sizes.py` holds the path list.
The job is skipped in other cases, for example a documentation-only change.
`ci-aggregate` requires the job when it applies and accepts `skipped` only when it does not apply.
Tree reuse does not skip the job: a reused tree proves the tests, not the sizes against this base.

**What it builds.**
For the PR head and for its base, the job exports the commit with `git archive`.
It builds each facade with `just packaging install` and the release recipe on the [default backend](runbooks/backends.md), plus `shipped` on libpinyin.
It copies `libpinyin.so.15.0.0` and `libzhuyin.so.15.0.0`, strips each copy with `strip --strip-unneeded`, and counts its bytes.
It then checks that the stripped library exports exactly the symbols of its version script, `libpinyin.ver` (79) or `libzhuyin.ver` (52), and no `oxpinyin_*` symbol.
It writes a table to the job summary: base bytes, PR bytes, difference and ratio for each library.
A failed build, install or export check fails the job.
A size increase does not fail the job: §2 requires a report, not a limit.

The PR side builds the PR head, not the merge ref.
Both sides build from the same source path, with the same path remapping, in one shared target directory.
A path therefore never changes a byte, and the dependencies compile once.
The job restores the dependency cache that main saves, and a pull request never saves it.

**Where base sizes come from.**
A second full build on every PR is the largest cost, so the base result is cached.
The cache key is `ship-sizes-v1-` plus a hash of two things:

- the Git tree ids of every input that can change a library byte;
- the toolchain fingerprint: `rustc -vV`, the first line of `cc --version`, `strip --version` and `cargo cinstall --version`, and the versions of `binutils`, `gcc`, `g++`, `libc6`, `libglib2.0-dev` and the reference backend's packages (`BACKEND_PACKAGES` in `tools/dev/ship_sizes.py`).

The key names no commit.
Every main commit with the same inputs shares one result.
On a push to main the job builds the head and saves its sizes under its key, unless the key exists.
On a pull request the job restores the key of the base commit.
A hit costs one build.
A miss costs two builds and is correct: the job builds the base, reports it as "built in this run", and saves it under the key for later pushes to the same pull request.
`debian:testing` is unpinned, so a package update changes the fingerprint and causes one miss.
A stale base can never match: a changed input or toolchain changes the key.

**Reproduce the table locally.**
The recipe is Linux-only.
Run it in the Linux reference container, on committed work:

```bash
just packaging sizes run --base origin/main
```

`run` builds the base commit and `HEAD`, then prints the same table.
Use `--base-cache FILE` to reuse a base result and `--work DIR` and `--target DIR` to keep the build directories.
`just packaging sizes build --commit REV --out FILE` writes one side as JSON.
`just packaging sizes key --commit REV` prints the cache key.
Use the local command to debug a CI failure, not to collect sizes for a PR.

## Release artifacts (`release-packages.yml`)

Every published GitHub release triggers
`.github/workflows/release-packages.yml`, which builds drop-in packages for
the three distro families and attaches them (plus a `SHA256SUMS`) to the
release with `gh release upload`.

Each lane builds on its distro's rolling branch: Debian testing, Fedora
Rawhide and Arch. Fedora's lane uses `fedora:rawhide`, not `fedora:latest`,
which is the current stable release.

Each lane builds under the ONE store backend whose model format that
distro's own `libpinyin` data is encoded in, so the drop-in reads the data
already on the system:

| lane | image | backend | because |
|---|---|---|---|
| Debian | `debian:testing` | tkrzw | Debian's libpinyin 2.11.91 (testing/forky) switched BerkeleyDB → Tkrzw (`libtkrzw1t64`); stable (trixie) still ships 2.8.1 on BerkeleyDB, which no backend reads |
| Fedora | `fedora:rawhide` | kyotocabinet | Fedora's libpinyin still links KyotoCabinet (`kyotocabinet-libs`) |
| Arch | `archlinux:latest` | kyotocabinet | Arch's libpinyin still links KyotoCabinet |

`just packaging release-stage` builds and stages through the supported
path: `just packaging install` (cargo cinstall plus the complete `.pc`)
once per library, with `--destdir` pointing at the staging root and the
backend selected by cargo-c's ordinary
`--no-default-features --features <backend>` (plus `shipped` on the pinyin
crate). cargo-c forwards both flags to `cargo build`: its subcommands
register cargo's full feature argument set. Each lane uses its distro's own `cargo-c`
package, the one the distro's libpinyin packaging would use; rustc and
cargo still come from `rust-toolchain.toml`. The script then strips the
shared objects and re-gates the tree (the fixed file list of
`docs/findings/installed-naming.md`, SONAME, the five pkg-config reads real
consumers perform, and a C compile/link/run) before any packaging runs.

The per-distro makers wrap that staged tree in the shape the distro's real
libpinyin packaging uses, and every package takes the distro's
libpinyin/libzhuyin over **in their entirety** — same sonames, same
pkg-config names, versioned Provides at the pin's libpinyin version
(`just packaging pc-version`; 2.11.92), and
Conflicts/Replaces (deb) / Obsoletes (rpm) / `conflicts=` (pacman) on the
originals:

- `just packaging release-deb` — `oxpinyin-libpinyin15-<backend>` +
  `-dev` (~ libpinyin15 + libzhuyin15, and their -dev packages). Depends is
  computed from the shipped ELF's DT_NEEDED via ldconfig + `dpkg -S`, so
  t64-era names (`libglib2.0-0t64`, `libtkrzw1t64`) resolve on whatever
  suite the build runs on, plus a `libc6 (>= N)` floor taken from the
  highest `GLIBC_*` symbol version referenced. The `-dev` package depends
  on the matching runtime package and `libglib2.0-dev` for the GLib headers
  included by `pinyin.h` and `zhuyin.h`.
- `just packaging release-rpm` — `oxpinyin-libpinyin-<backend>` +
  `-devel` (~ libpinyin + libpinyin-devel). rpm's dependency generator
  emits the soname Provides (`libpinyin.so.15()(64bit)`) and the backend
  Requires automatically; Obsoletes (not Conflicts — a package may not
  conflict with a name it provides) performs the swap.
- `just packaging release-arch` — one `oxpinyin-libpinyin-<backend>`
  package, since Arch ships libpinyin undivided, with soname Provides in
  pacman's form (`libpinyin.so=15-64`) and, via `--data=DIR`, the model
  directory installed as `/usr/lib/libpinyin/data`; `--data-version=VER`
  records the origin package version in the description, since Arch's data
  (2.10.x) is older than the pin version (2.11.92) the library provides.
  Its existing `glib2` dependency already includes the GLib headers.

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
