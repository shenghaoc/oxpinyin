# Packaging oxpinyin as a C library (cargo-c)

This document records how `oxpinyin-capi` is packaged and installed as a shared and static C library, and the decisions behind it.
The installed tree is a drop-in for libpinyin.
Consumers such as `ibus-libpinyin` link the 79-symbol `pinyin.h` surface exactly as they link upstream (`libpinyin.pc`, `-lpinyin`, `libpinyin.so.15`) and need no source changes.
The same applies to `oxpinyin-zhuyin-capi` and `libzhuyin`.

## Trainer utility layout

Peng Wu's [epico/trainer](https://github.com/epico/trainer/tree/b1927376735c0c1042d72af2eca0e25c53595724)
at `b1927376735c0c1042d72af2eca0e25c53595724` changes into the `data/` directory of a libpinyin tree before it calls the utilities.
Build the tools with the [native setup](runbooks/setup.md), then stage two new trees:

```sh
just build --locked -p oxpinyin-utils --bins
just trainer-layout /absolute/path/trainer-tools --bin-dir target/debug
just trainer-layout /absolute/path/trainer-evals --bin-dir target/debug
```

The recipe delegates to `tools.dev.packaging`.
It copies all 21 pin-named utilities into `utils/storage/`, `utils/segment/` and `utils/training/`.
It writes `data/Makefile` and copies the [configuration template](../data/table.conf.in) to `data/table.conf.in`.
The copies keep executable permissions and survive a Cargo clean; native shared-library dependencies must still be available.
The destination must not exist.
The recipe resolves symlinks in the destination path and refuses a destination under a directory that has a `.git` entry (a directory, a gitfile or a symlink).
It also refuses, before it writes anything, when it cannot inspect the destination or one of those entries.
It does not detect a work tree that has no `.git` entry, such as a `core.worktree` setup.
`--bin-dir` selects another build or profile directory; its default is `$CARGO_TARGET_DIR/debug` or `target/debug`.
The recipe installs nothing system-wide and copies no trainer code or model data.

In the trainer's existing `lib/myconfig.py` configuration, set `m_tools_dir` (returned by `getToolsDir()`) to `/absolute/path/trainer-tools`.
Set `m_evals_dir` (returned by `getEvalsDir()`) to `/absolute/path/trainer-evals`.
Use the tree roots, not their `data/` subdirectories.
Keep the two trees separate: evaluation replaces `interpolation2.text` and regenerates its binary data.

Put into each `data/` the model's textual tables and `interpolation2.text`.
From that directory, render `table.conf` for the DBM backend of the tools and run `make`.
Use these commands for the default tkrzw build:

```sh
sed 's/@DATABASE_FORMAT@/Tkrzw/' table.conf.in > table.conf
make
```

For the peer backends, use `BerkeleyDB` or `KyotoCabinet` in place of `Tkrzw`.
Also supply the trainer's held-out inputs: `data/estimates.db` for the tools tree, and `data/deleted_bigram.db` and `data/evals2.text` for the eval tree.
This recipe stages a layout; it does not prepare a corpus, download model20 or run the trainer.

The generated Makefile provides the trainer's default build, `clean` and `modify LAMBDA_PARAMETER=...` targets.
Build and clean use the pin's file lists and the exact relative utility calls from
[`data/Makefile.am:18–69` at `074a2219c90feaf962d0d24f034514033ece5f99`](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/data/Makefile.am#L18-L69).
`modify` keeps the first `table.conf` as `data/.trainer-table.conf` and applies the pin's lambda substitution again to that baseline on every call.
The generated Makefile runs no `git` command; it does not run the pin's checkout-wide `git reset --hard`.
Remove that baseline when you deliberately replace the configuration.

The trainer requires `/usr/bin/make`; the staged Makefile uses `rm`, `cp` and `sed`.
The pin's optional model-download rule (`wget`, `tar`) is not staged: supply the data first.

The table lists every utility that the trainer reaches.
Paths are relative to `data/`, and each path maps to the **same basename** in `oxpinyin-utils`.
All 15 are ported.

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
The six additional staged pin utilities are storage `gen_pinyin_table`, `gen_zhuyin_table` and `export_interpolation`, and training `gen_ngram`, `gen_deleted_ngram` and `import_k_mixture_model`.
No utility port in this inventory is missing; the trainer's own Python scripts stay external.

Run `just test-tools` for the packaging fixtures.
After the build above, run the explicit real-tool integration test in the same Python test file:

```sh
uv run --no-project --managed-python python -X utf8 tools/dev/packaging.test.py TrainerLayoutTests.check_real_tools
```

It checks every reached path and runs `../utils/training/gen_unigram --help` from the created `data/`.
Set `OXPINYIN_TRAINER_BIN_DIR` for another binary directory.
A missing tool fails this test; the test does not substitute fixtures.

## Why cargo-c

`oxpinyin-capi` is a `cdylib` that must be installed like any other C library: a versioned `.so`, an unversioned linker symlink, `pinyin.h` and a `pkg-config` file.
`cargo cbuild` and `cargo cinstall` derive the install layout and SONAME from Cargo metadata.
cargo-c cannot produce a complete libpinyin `.pc` ([Locating the model data](#locating-the-model-data-pkgdatadir)), so `just packaging install` wraps `cargo cinstall`.
Run from the checkout with the [ordinary native setup](runbooks/setup.md); `just python` provisions its managed interpreter.
The packaging commands supervise builds, relinks and package builders, with cancellation handling.

## Metadata on `oxpinyin-capi`

`crates/oxpinyin-capi/Cargo.toml` carries the full contract.
The installed tree takes libpinyin's own binary identity: SONAME `libpinyin.so.15`, `libpinyin.pc` and headers under `libpinyin-2.11.92/`.
The source tree keeps the oxpinyin names.
`docs/findings/installed-naming.md` holds the full rationale and the measured gates.
In short:

- `[lib] crate-type = ["cdylib", "staticlib", "rlib"]`.
  cargo-c requires `staticlib`: it builds the `.a` beside the `.so`.
  `rlib` stays so that the fuzz targets and Rust tests can use the crate in-process.
- `[features] capi = []`.
  cargo-c identifies the crate to package by the presence of a `capi` feature; without it, cargo-c skips the crate.
  `[features] shipped = []` compiles out the two `fuzz-api` helpers and gates no exported symbol; only the shipped drop-in artifact enables it.
- `[package.metadata.capi.header] generation = false`, `subdirectory = "libpinyin-2.11.92"`.
  `pinyin.h` and its two companion headers ship **verbatim** under libpinyin's version-stamped include subdirectory and are never regenerated.
- `[package.metadata.capi.pkg_config] name = "libpinyin"`, `version = "2.11.92"`.
  The `.pc` answers to libpinyin's own name and a libpinyin version, so the `>=` constraints of consumers resolve.
  cargo-c's own `.pc` is incomplete (a closed seven-key field set); `just packaging install` overwrites it from the template that build.rs bakes.
- `[package.metadata.capi.library] name = "pinyin"`, `version = "15.0.0"`, `versioning = true`.
  The INSTALLED artifact is the drop-in `libpinyin.so.15`.
  The Rust `[lib] name` stays `pinyin_capi`, so in-tree gates keep finding `target/debug/libpinyin_capi.so`.

No `"."` first-member entry is necessary: that cargo-c requirement applies only when the exported crate is the workspace **root**.
Here the crate is a normal member (`crates/oxpinyin-capi`), selected by the `capi` feature and `-p oxpinyin-capi`.

## Consumer detection

ibus-libpinyin, like any other C consumer, detects the library exactly as it detects upstream.
The `.pc` name is `libpinyin`, not `oxpinyin`, and the version it reports is libpinyin's, so existing `>=` constraints resolve:

```autoconf
PKG_CHECK_MODULES(LIBPINYIN, [libpinyin >= 2.11.92])
```

Nothing in the installed tree carries the `oxpinyin` or `pinyin_capi` name.
Those names exist only in the source tree and in the Rust artifact names under `target/`.

## User data across the takeover

Each oxpinyin release lane is built on the same backend as the distribution's libpinyin ([lane table](#release-artifacts-release-packagesyml)).
It keeps the user directory as that backend's native user files, exactly as the libpinyin build it replaces does (maintainer ruling 2026-09-09).
The profile is the one that ibus-libpinyin keeps under `~/.cache/ibus/libpinyin/`.
The [libpinyin FAQ](https://github.com/libpinyin/libpinyin/wiki/libpinyin-FAQ) tells users to reset that directory by deleting its `user.conf`:

- **Same backend: nothing changes.** `user.conf`, `user_bigram.db`, `user_pinyin_index.bin`, `user_phrase_index.bin`, `user.bin` and the `*.dbin` logs are the backend's native files.
  The Kyoto Cabinet lanes (Fedora, Arch) keep the same files that a KC-built libpinyin keeps.
  The tkrzw lane (Debian) keeps the same files that a tkrzw-built libpinyin keeps.
  The recorded evidence is the round trip against pin-built oracles in [user-store §11](findings/user-store.md#11-implementation-record--the-store-persists-in-libpinyins-own-files-2026-09-09).
- **Different backend: fresh start, as with libpinyin.** A build on another backend cannot read the user directory and starts fresh.
  libpinyin's tkrzw build cannot read its Berkeley DB user files either (`check_format`; Debian's BerkeleyDB→Tkrzw switch carried a `debian/NEWS` warning).
  A distribution whose users cross a backend boundary when they install a takeover package should carry the same `debian/NEWS`-style notice.
  Users who want to keep their phrases can use the dictionary export and import of their frontend (ibus-libpinyin's Import/Export).
  These use libpinyin's ordinary ABI, which carries the user phrase dictionary but not the trained bigram.

## Static library decision: ship it

cargo-c always builds a `.a` for a `staticlib` crate and has **no** metadata setting to suppress it.
Debian's guidance notes that packagers would otherwise need a "not-installed" rule to drop it.
Decision: **ship the static library**.
It adds negligible install size, is the cargo-c default, and removes variance between packagers: every packager produces the same artifact set.
It installs as `libpinyin.a` beside the `.so`.

The `.pc` files mirror the pin's and have no `Libs.private`.
A static link against `libpinyin.a` must pass the platform libraries itself (`-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`), plus the selected backend library.

## The four version streams

The four streams are independent and change for different reasons:

1. **Crate version:** `0.1.0`, pre-1.0.
   The C surface is free to evolve after the first release, so the crate stays `0.x` and makes no semver-compatibility promise at the C-ABI level.
   It makes none for Rust either: native Rust interfaces are not supported externally ([interface ruling](constitution.md#external-interfaces)).
2. **`.so` SONAME:** `libpinyin.so.15` (`libzhuyin.so.15`), from `[package.metadata.capi.library] version = "15.0.0"`.
   This is upstream's own ABI number (`libpinyin_abi_current=15` in its configure.ac).
   It makes the drop-in a drop-in, because every consumer already records `libpinyin.so.15` in `DT_NEEDED`.
   It changes only when upstream increases its ABI current, never with the crate version.
3. **Pinned oracle:** libpinyin `2.11.92`.
   A re-pin of the oracle is a deliberate event with its own re-freeze ([goldens and pins](runbooks/goldens-and-pins.md#3-the-oracle-pin)), independent of the crate version and SONAME.
   The installed identity of the drop-in (the `.pc` `Version:`, the `libpinyin-<version>/` header subdirectory and the versioned Provides of the packages) is the pin's own `configure.ac` version and changes with it.
   Its one source is `libpinyin_tag` in `tools/oracle/oracle-pin.txt`.
   Both capi build scripts read it and check their `[package.metadata.capi]` tables against it, and `just packaging pc-version` prints it for package assembly.
4. **Parity pins:** `10190 / 10190 / 98930` of `98930` candidate symbols, `0` absent, `0` tie-swaps ([frozen parity numbers](runbooks/goldens-and-pins.md#2-the-frozen-parity-numbers)).
   These freeze the decode and predict output that the oracle is held to; packaging must not move them.

Streams 1 and 2 are decoupled by construction.
Without an explicit setting, cargo-c derives the SONAME from the crate version (`0.x` gives SONAME `libpinyin.so.0.x`, which changes on every minor bump).
So `library.version` is set explicitly and must stay at upstream's ABI number, whatever the crate version does.
A package version (stream 1, the release tag) therefore never appears in a filename that the dynamic linker reads.

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

Both recipes have the shape that the distributions document.
A real package build must run `just packaging install <libpinyin|libzhuyin> --prefix=/usr …` instead of the bare `cargo cinstall`, or run it again afterwards.
The reason is that cargo-c's own `.pc` lacks the variables that consumers read ([Locating the model data](#locating-the-model-data-pkgdatadir)).
The release packages that `release-packages.yml` builds go through exactly that wrapper ([Release artifacts](#release-artifacts-release-packagesyml)).

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

The Kyoto Cabinet backend compiles Kyoto Cabinet's header code into the facade, as libpinyin's Kyoto Cabinet build does.
The adapter of the cxx bridge (`crates/oxpinyin-store/src/kyotocabinet/adapter.cc`) constructs `TreeDB`, `HashDB`, `ProtoTreeDB` and `StashDB` from the installed headers.
That compiled code calls the out-of-line functions of the system libkyotocabinet and shares its object layouts.
So the headers that a package builds against must be the same Kyoto Cabinet version as the libkyotocabinet it runs with.

Build against the distribution's `libkyotocabinet-dev` or `kyotocabinet-devel` for the `libkyotocabinet` it ships.
Rebuild the facade whenever the version of that library changes, as libpinyin's package must be rebuilt.
The backend checks only that the running library is 1.2.x (`check_runtime_version` in `crates/oxpinyin-store/src/kyotocabinet/ffi.rs`); it does not detect a header/library mismatch.

## Locating the model data (`pkgdatadir`)

libpinyin's own `.pc` exports `pkgdatadir`, `database_format`, `libpinyinincludedir` and `libpinyin_binary_version`, and consumers read them.
ibus-libpinyin's build resolves its system data directory from `pkgdatadir` and refuses to configure without it.
cargo-c cannot emit custom pkg-config variables: its `[package.metadata.capi.pkg_config]` is a closed seven-key set.
It also cannot skip writing its own incomplete `libpinyin.pc`.

The contract is therefore carried outside cargo-c.
Each crate's build.rs bakes a complete `.pc` template (`libpinyin.pc.in.baked`) with the build-time fields.
`just packaging install` fills the install-time placeholders and **overwrites** the file that cargo-c installed.
Both templates are the pin's own `.pc.in`, and the wrapper substitutes what the pin's configure substitutes:

- `exec_prefix=${prefix}`;
- `libdir` and `includedir` as the given `--libdir` and `--includedir`, verbatim;
- without those options, autoconf's prefix-relative `${exec_prefix}/lib` and `${prefix}/include`.

The installed file is therefore byte-identical to the pin's for the same layout and backend; `just packaging check-pc-metadata` diffs the two trees.
A bare `cargo cinstall` without the wrapper leaves the incomplete file.
`docs/findings/installed-naming.md` records that silent window and its gates.

`pkgdatadir` follows upstream's convention exactly: it points one level *above* the data, at `${libdir}/libpinyin`.
The model is in `${pkgdatadir}/data`: `table.conf`, the phrase and pinyin indexes, `bigram.db` and the chunk file of each library.
`pinyin_init` and `zhuyin_init` take that `data` directory as their `systemdir`.
They fail closed (NULL) on a missing or unreadable model, including a model in the wrong store format.
For that reason, each release lane is built under the one backend that matches the distribution's data ([Release artifacts](#release-artifacts-release-packagesyml)).

**The library install ships no data of its own**, except the Arch release package.
The distributions' `libpinyin-data` (Debian, Fedora) stays in place and is read as it is.
Data generation with the pin's program sequence from `oxpinyin-utils` is a separate deliverable, not part of `cargo cinstall`.

## Relocation

The generated `.pc` derives every path from `${prefix}` and contains no baked absolute paths, so a `DESTDIR` install relocates cleanly.
When the prefix is a system path (`/usr`), `pkg-config --cflags --libs libpinyin` returns only `-lpinyin`, plus glib from `Requires`.
The reason is that pkg-config omits `-I/usr/include -L/usr/lib64`.
`pkg-config --define-prefix`, or `PKG_CONFIG_SYSROOT_DIR` as distribution build roots use it, resolves the staged paths.
This is the standard DESTDIR relocation mechanism, not a defect.

## Drop-in identity

A consumer loads the replacement with `dlopen("libpinyin.so.15")`, or through `DT_NEEDED`, with no source change.
Three properties decide whether that works.

**SONAME.** The pin's `configure.ac` sets `libpinyin_abi_current = 15` and `libpinyin_abi_revision = 0`.
libtool's `-version-info 15:0` gives the SONAME `libpinyin.so.15`, and a consumer records that string in `DT_NEEDED`.
`crates/oxpinyin-capi/build.rs` passes it as a `cdylib` link argument.
`[package.metadata.capi.library]` `name = "pinyin"` / `version = "15.0.0"` names the installed file.
`libzhuyin.so.15` follows the same rule.

**Symbol versioning.** The pin defines its symbols under the `LIBPINYIN` version node (`pinyin_init@@LIBPINYIN`).
A linked consumer therefore records `File: libpinyin.so.15 → Name: LIBPINYIN` in `.gnu.version_r`.
Three library shapes behave differently against that reference:

| Library shape | Outcome |
| --- | --- |
| No version definitions | loads and runs; glibc warns `no version information available` once per start |
| Symbols versioned under `LIBPINYIN` | loads and runs clean, the pin's shape |
| Version definitions present, symbols unversioned | **hard failure**: `version 'LIBPINYIN' not found` |

rustc always gives a cdylib its own anonymous version script, and a second, named script cannot be combined with it.
GNU ld rejects the pair.
rust-lld accepts the definitions but does not reassign the symbols, which gives the third shape.
So the cargo-built cdylib has no version script and has the first shape.
On Linux, `just packaging install` relinks the installed object from the staticlib with `-Wl,-soname` and the crate's `.ver` script, which gives the second shape ([Export gate](#export-gate)).
`libzhuyin.so.15` uses `LIBZHUYIN` and its 52 symbols the same way.

**Symbol scope.** The shipped library exports every symbol in its `.ver` script and nothing else.
The `shipped` feature gates no exported symbol.
It compiles out only the `fuzz-api` helpers `oxpinyin_import_aborts` and `oxpinyin_train_aborts`, which are Rust functions and not exports.
The feature is subtractive.
An additive, default-on helper feature would depend on the default features, which also select the backend.
Every build on a non-default backend would then drop the helpers.

## Export gate

`just abi exports` is the mechanical form of the drop-in promise for symbols (`check_exports` in `tools/dev/abi.py`).
It proves that the set of exported `pinyin_*` / `zhuyin_*` symbols is exactly the pin's version script, with nothing missing and nothing extra.
The version scripts are `crates/oxpinyin-capi/libpinyin.ver` (79) and `crates/oxpinyin-zhuyin-capi/libzhuyin.ver` (52), both checked in verbatim from the pin.
It also proves that no other symbol leaks.

CI runs it twice on every change:

- on the development build, which may export no `oxpinyin_*` symbol, the same as the shipped build;
- with `--shipped`, on the packaged artifact, where only the set of the version script may remain.

No symbol in a version script is feature-gated.

On Linux, `just packaging install` relinks the installed shared object from the staticlib under the crate's `.ver` script (`just packaging relink-versioned`), with `-Wl,-z,defs`.
The shipped library therefore carries the pin's symbol versioning.
Before it replaces the installed file, the relink verifies the export set and the version tag of every export against the `.ver` list, in both directions.
It keeps the same file name, so cargo-c's symlinks keep resolving.
`just abi exports` compares the cdylib that cargo builds (names, not versions); that is the gate for the development artifact.

On macOS, `just packaging install` also relinks from the cargo-c staticlib (`relink_darwin` in `tools/dev/packaging.py`).
The Apple linker rejects `--exclude-libs`, cannot combine nonempty exported and unexported lists, and takes the union of two exported lists.
A second list on rustc's cdylib link would therefore add exports, not restrict them.
The relink uses one `-exported_symbols_list`, derived from the `.ver` globals with a `_` prefix on each name.
The restriction therefore holds for every archive symbol, including the transitive `#[export_name]` symbols.
The replacement keeps cargo-c's install name, compatibility and current versions, architecture and deployment target.

Before the atomic replacement, both `nm -gU` and the export trie (`dyld_info -exports`) must equal the `.ver` set.
`nm -gU` covers all external defined types, including data, absolute and weak symbols.
The installed native dylib must then load and resolve its entry point.
The nightly `test-macos` job provisions cargo-c, stages both release libraries and runs `just abi exports --shipped`, with `OXPINYIN_CAPI_SO` and `OXPINYIN_ZHUYIN_SO` set to those installed files.
Without overrides, that command checks Cargo development artifacts built with the `shipped` feature.
The packaging and ABI tests in `tools/dev/packaging.test.py` and `tools/dev/abi.test.py` cover identity preservation, failure atomicity and leak detection.

The cxx bridges of the tkrzw and Kyoto Cabinet backends link the `cxx` crate.
Its Rust runtime defines `#[export_name]` symbols (`cxxbridge1$slice$*`, `cxxbridge1$string$*`, `cxxbridge1$exception`, ...).
rustc lists every such upstream symbol as a C-level export of a `cdylib`, so without a countermeasure the cargo-built cdylibs export the whole runtime.
Both relinks above exclude these symbols from the shipped libraries: the Linux version script's `local: *` and the macOS `-exported_symbols_list`.

For the development cdylibs that the gate checks, both capi build scripts pass `-Wl,--exclude-libs,ALL` to the Linux cdylib link when the tkrzw or kyotocabinet feature is on.
GNU ld then gives hidden visibility to every symbol defined in an archive: the rlibs and the static library of the bridge.
rustc's anonymous version script cannot override that visibility.
The `pinyin_*` and `zhuyin_*` symbols come from the capi crates' own object files and are not affected.
No such flag exists on macOS, where the development cdylibs carry the extra symbols.
On macOS, `just abi exports` runs against the relinked installed files, as the nightly job does.

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
Each side runs its own revision's `just packaging install`, so the host needs just and a uv that can install each revision's interpreter ([setup](runbooks/setup.md#comparing-against-another-revision)).
Use `--base-cache FILE` to reuse a base result and `--work DIR` and `--target DIR` to keep the build directories.
`just packaging sizes build --commit REV --out FILE` writes one side as JSON.
`just packaging sizes key --commit REV` prints the cache key.
Use the local command to debug a CI failure, not to collect sizes for a PR.

## Release artifacts (`release-packages.yml`)

Every published GitHub release triggers `.github/workflows/release-packages.yml`.
The workflow builds drop-in packages for the three distribution families and attaches them, with a `SHA256SUMS`, to the release with `gh release upload`.
A pull request that changes a packaging path runs the workflow as a dry run that stops before the upload ([release runbook](runbooks/release.md#what-each-lane-gates)).

Each lane builds on the rolling branch of its distribution: Debian testing, Fedora Rawhide and Arch.
Fedora's lane uses `fedora:rawhide`, not `fedora:latest`, which is the current stable release.

Each lane builds under the ONE store backend whose model format encodes that distribution's own `libpinyin` data.
The drop-in therefore reads the data already on the system:

| lane | image | backend | because |
|---|---|---|---|
| Debian | `debian:testing` | tkrzw | Debian's libpinyin 2.11.91 (testing/forky) switched BerkeleyDB → Tkrzw (`libtkrzw1t64`); stable (trixie) still ships 2.8.1 on BerkeleyDB, which no backend reads |
| Fedora | `fedora:rawhide` | kyotocabinet | Fedora's libpinyin still links KyotoCabinet (`kyotocabinet-libs`) |
| Arch | `archlinux:latest` | kyotocabinet | Arch's libpinyin still links KyotoCabinet |

`just packaging release-stage` builds and stages through the supported path.
It runs `just packaging install` (cargo cinstall and the complete `.pc`) once for each library, with `--destdir` set to the staging root.
cargo-c's ordinary `--no-default-features --features <backend>` selects the backend, with `shipped` added on the pinyin crate.
cargo-c forwards both flags to `cargo build`, because its subcommands register cargo's full set of feature arguments.
See [Installing uv and cargo-c](runbooks/setup.md#installing-uv-and-cargo-c) for host setup.
Rustc and Cargo come from `rust-toolchain.toml`.

The script then strips the shared objects and checks the tree again before any packaging runs.
The checks are the fixed file list of `docs/findings/installed-naming.md`, the SONAME, the five pkg-config reads that real consumers perform, and a C compile/link/run.
The run certifies the `libtkrzw` that the smoke binaries load against `OXPINYIN_TKRZW_LIBRARY`. The Debian lane sets it to the dpkg-owned `libtkrzw.so.1` that its package depends on.

The maker for each distribution wraps that staged tree in the shape that the distribution's real libpinyin packaging uses.
Every package takes over the distribution's libpinyin and libzhuyin **in their entirety**:

- the same sonames and the same pkg-config names;
- versioned Provides at the pin's libpinyin version (`just packaging pc-version`; 2.11.92);
- Conflicts/Replaces (deb), Obsoletes (rpm) or `conflicts=` (pacman) on the originals.

The makers are:

- `just packaging release-deb`: `oxpinyin-libpinyin15-<backend>` and `-dev`, which correspond to libpinyin15 and libzhuyin15 and their -dev packages.
  Depends is computed from the DT_NEEDED entries of the shipped ELF through ldconfig and `dpkg -S`.
  t64-era names (`libglib2.0-0t64`, `libtkrzw1t64`) therefore resolve on the suite that the build runs on.
  Depends also has a `libc6 (>= N)` floor, taken from the highest `GLIBC_*` symbol version referenced.
  The `-dev` package depends on the matching runtime package and on `libglib2.0-dev` for the GLib headers that `pinyin.h` and `zhuyin.h` include.
- `just packaging release-rpm`: `oxpinyin-libpinyin-<backend>` and `-devel`, which correspond to libpinyin and libpinyin-devel.
  rpm's dependency generator emits the soname Provides (`libpinyin.so.15()(64bit)`) and the backend Requires automatically.
  Obsoletes performs the swap; Conflicts cannot, because a package may not conflict with a name it provides.
- `just packaging release-arch`: one `oxpinyin-libpinyin-<backend>` package, because Arch ships libpinyin undivided.
  It has soname Provides in pacman's form (`libpinyin.so=15-64`).
  With `--data=DIR`, it installs the model directory as `/usr/lib/libpinyin/data`.
  `--data-version=VER` records the origin package version in the description, because Arch's data (2.10.x) is older than the pin version (2.11.92) that the library provides.
  Its existing `glib2` dependency already includes the GLib headers.

Data: Debian and Fedora keep `libpinyin-data`.
It is a separate package on those distributions, stays installed through the takeover, and is only a Recommends of the oxpinyin package.
Arch has no data package: the model is inside the libpinyin package that the takeover removes.
So the Arch lane downloads that package (`pacman -Sw`) and ships its `usr/lib/libpinyin/data` inside the oxpinyin package.
These are the same KyotoCabinet-format files, under the same licence, at the same path, `${pkgdatadir}/data`.

At the end, each CI lane INSTALLS its own packages back into its build container, with `libpinyin-data` on Debian and Fedora.
It then runs the gates again against `/usr`:

- the same five pkg-config reads;
- a C compile/link/run;
- a real `pinyin_init`/`zhuyin_init` on `$(pkg-config --variable=pkgdatadir libpinyin)/data`, which must return a context.

A release therefore never attaches a package that does not install, does not answer as libpinyin/libzhuyin, or cannot open the system's model.
