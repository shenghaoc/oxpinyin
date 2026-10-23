# Packaging oxpinyin as a C library (cargo-c)

This document records how `oxpinyin-capi` is packaged and installed as a
shared/static C library, and the decisions behind it. The installed tree
is a drop-in for libpinyin: consumers such as `ibus-libpinyin` link the
79-symbol `pinyin.h` surface exactly as they link upstream — `libpinyin.pc`,
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
`libpinyin.pc`, headers under `libpinyin-2.11.92/` — while the source tree
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
  "libpinyin-2.11.92"`. `pinyin.h` and its two companion headers ship
  **verbatim** under libpinyin's version-stamped include subdirectory, never
  regenerated.
- `[package.metadata.capi.pkg_config] name = "libpinyin"`,
  `version = "2.11.92"`. The `.pc` answers to libpinyin's own name and a
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
  Measured against pin-built Kyoto Cabinet and tkrzw oracles by
  `tools/oracle/user-dir-round-trip.sh` (`docs/findings/user-store.md`
  §11).
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
`libpinyin.a` beside the `.so`, and `libpinyin.pc`'s `Libs.private` lists the
platform libraries a static link needs
(`-lgcc_s -lutil -lrt -lpthread -lm -ldl -lc`).

## The four version streams

These are independent and move for different reasons:

1. **Crate version** — `0.1.0`, pre-1.0. The C surface is free to evolve after
   the first release, so the crate stays `0.x` and makes no semver-compat
   promise at the C-ABI level. It makes none for Rust either: native Rust
   interfaces are not supported externally (`AGENTS.md`, "External
   interfaces").
2. **`.so` SONAME** — `libpinyin.so.15` (`libzhuyin.so.15`), from
   `[package.metadata.capi.library] version = "15.0.0"`. This is upstream's
   own ABI number (`libpinyin_abi_current=15` in its configure.ac), and it
   is what makes the drop-in a drop-in: every consumer already records
   `libpinyin.so.15` in `DT_NEEDED`. It moves only when upstream bumps its
   ABI current — never with the crate version.
3. **Pinned oracle** — libpinyin `2.11.92`. Re-pinning the oracle is a
   deliberate event with its own re-freeze (`pin-refreeze-*.md` convention),
   independent of the crate version and SONAME. The drop-in's installed
   identity — the `.pc` `Version:`, the `libpinyin-<version>/` header
   subdirectory, the packages' versioned Provides — is the pin's own
   `configure.ac` version and moves with it: its one source is
   `libpinyin_tag` in `tools/oracle/oracle-pin.txt`, which both capi build
   scripts read (and check their `[package.metadata.capi]` tables against)
   and `tools/packaging/pc-version.sh` prints for the shell side.
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
placeholders and **overwrites** the file cargo-c installed. Both templates
are the pin's own `.pc.in`, and the wrapper substitutes what the pin's
configure does — `exec_prefix=${prefix}`, and `libdir` / `includedir` as
the given `--libdir` / `--includedir` verbatim or, without them, autoconf's
prefix-relative `${exec_prefix}/lib` / `${prefix}/include` — so the
installed file is byte-identical to the pin's for the same layout and
backend (`tools/packaging/check-pc-metadata.sh` diffs the two trees). A bare
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

## Export gate

`tools/abi/check-exports.sh` is the mechanical form of the drop-in
promise on the symbol side: it builds both cdylibs and proves the set of
exported `pinyin_*` / `zhuyin_*` symbols is exactly the pin's version
script — `crates/oxpinyin-capi/libpinyin.ver` (79) and
`crates/oxpinyin-zhuyin-capi/libzhuyin.ver` (52), both checked in
verbatim from the pin — with nothing missing and nothing extra, and that
no other symbol leaks. CI runs it twice on every change: on the
development build, where the two `oxpinyin_*` fixture hooks the ABI
tests drive are the only permitted extras, and with `--features
shipped`, the packaged artifact, where only the version script's set may
remain. Its first run found the shipped library one symbol short
(`pinyin_set_full_pinyin_scheme`, gated out under the retired subset
scoping); nothing in a version script is gated any more.

The version scripts stopped being the record only on 2026-09-14: on Linux
`tools/packaging/install.sh` now relinks the installed shared object from
the staticlib under the crate's `.ver` script
(`tools/packaging/relink-versioned.sh`), so the shipped library carries
upstream's symbol versioning — `pinyin_init@@LIBPINYIN` — and consumers
load it with no glibc version warning. The relink itself verifies the
export set against the `.ver` list in both directions before replacing
anything; `check-exports.sh` keeps comparing the cdylib cargo builds
(names, not versions), which is still the right gate for the development
artifact the relink then supersedes on install.

## Measured function ordering (#986 Phase 2A)

Linux package builds select the checked-in measured policy for their backend,
matching the library's SONAME. The release profile remains fat LTO with one
codegen unit. A selected policy must resolve against the staticlib or the
relink fails; only a linker without `--section-ordering-file` keeps the
original layout.

### Policy selection by backend

Each backend has its own `tools/packaging/function-order/<backend>/` directory:
`bdb`, `kc` (Cargo feature `kyotocabinet`) or `tkrzw`. Each contains
`libpinyin.order` and `libzhuyin.order`, generated from that backend's shipping
build and same-cell profiles. The BDB files retain their previous contents.
`relink-versioned.sh` selects the directory from the build's feature list.
Measurement verification derives the backend independently from the linked
object's `DT_NEEDED`; the caller's environment cannot select another backend's
default. Both paths reject a named policy in a different backend's directory,
including a symlink to it. Experimental or historical policies outside this
layout still must resolve and pass the linked prefix check.

The former `function-order/libpinyin.order` and `libzhuyin.order` paths are
compatibility symlinks to the BDB files, for the existing nightly check.
They cannot select BDB policies for another backend: both selectors resolve
the symlink before checking the backend.

A backend with no registered policy links in the original layout and says
`no checked-in policy for the <backend> backend`. If the directory exists but
its facade policy is missing or stale, the relink fails. All three supported
backends are registered; a missing directory also fails. An explicit opt-out
and the unsupported-linker fallback keep their existing behavior.

### Final-link mechanism and fallback

Apply ordering in `tools/packaging/relink-versioned.sh`, after cargo-c has
produced the static library. That relink produces the installed ELF object;
changing only cargo-c's cdylib link would miss the shipping artifact.

Use GNU ld's `--section-ordering-file` to prepend selected input sections to
the existing `.text` output section. The option augments the default linker
script, avoiding a copied platform-specific script or a new output section.
It does not require `KEEP`, new roots, a version-script change, or a change to
`--whole-archive` or `--gc-sections`. See the
[GNU ld option description](https://sourceware.org/binutils/docs/ld/Options.html#index-_002d_002dsection_002dordering_002dfile).

`tools/packaging/function-order.sh` checks support through the compiler driver
used by the relink, rather than assuming that `ld` on PATH is the selected
linker. It requires GNU BFD: gold uses the same option name with different
syntax. The relink distinguishes four states and reports each on stderr:

| state | outcome |
|---|---|
| a selected policy resolves and the linker supports ordering | ordered link (`using measured policy`) |
| a selected policy does not resolve against the staticlib | the relink fails before linking |
| the linker lacks `--section-ordering-file` | original layout (`the linker does not support`) |
| ordering disabled with an empty `--function-order` | original layout (`disabled by explicit opt-out`) |

The policy is resolved first, with `nm` alone, before the linker is probed.
A stale path, a malformed or ambiguous policy, a missing named or checked-in
policy file, or a failing `nm` or demangler is an error: the resolver prints
`function ordering: unresolved policy path: <name>` for each path, the relink
adds a pointer to "Regeneration" below, exits nonzero and leaves the
installed object untouched (#1071: before this, the same inputs linked the
original layout behind one stderr line, and `main` shipped unordered
libraries after a rename). Only a linker without the option selects the
existing link command without ordering arguments. Probe output stays in
self-cleaning scratch. Verify that the unsupported-feature route has the same
bytes as the original relink for the same inputs. An ordinary link or
export-verification failure still fails; fallback does not retry a failed
shipping link.

A policy that went stale and merged is reported by the nightly
`function-order-policy` job in `.github/workflows/verify-nightly.yml`
(#1071). The job builds both facades' staticlibs for BDB, KC and tkrzw with
the release recipe
(`tools/packaging/install.sh`, the build whose archive the relink reads) and
runs `tools/packaging/function-order.sh resolve ARCHIVE POLICY SONAME` for
all six backend/facade policies. Each policy is checked against its own
backend's shipping staticlib, with separate target directories per backend
and one cargo-c installation for the job. It switches the relink's own
ordering off, as
"Regeneration" below does, so that a stale policy fails this check, with its
message, and not the relink. On an unresolved path the job prints the
resolver's `unresolved policy path` line and points to "Regeneration". It
runs on the schedule and on a manual dispatch of that workflow
(`gh workflow run verify-nightly.yml --ref <branch>`, which also runs every
other nightly job; `windows_only` skips it) and blocks no merge. The check uses
the backend-specific policy paths for all three backends; the legacy BDB
aliases remain available for compatibility.

The relink passes `-Wl,-z,defs` on that one link command, so the ordered and
the unordered link both carry it, on every backend. A reference in the
staticlib that `$GLIB_LIBS`, the backend libraries, `-lpthread`, `-ldl` and
`-lm` do not resolve fails the link, and the linker names the symbol. Without
the option the object would ship with the symbol undefined and no
`DT_NEEDED` entry for it. No route removes the option: a driver or linker
that rejects it fails the shipping link, it does not fall back. The option
changes no layout; the bytes match a link without it whenever that link
resolves every reference.

Measurement tools have a stricter boundary: before reporting a shipping arm,
`tools/packaging/function_order.py` verifies the actual unstripped linked
object's policy prefix at the start of `.text`. It refuses fallback,
inapplicable/missing policy functions, wrong order and stripped objects, with
the arm and a linker diagnostic. A successful install or a stderr message
cannot replace this check. Each arm prints its own result. Use
`--function-order ARM/FACADE=POLICY` to name a historical policy, or an explicit
`--unordered-arm ARM` for an intentional non-shipping experiment. See
[the measurement inventory and refusal rules](runbooks/benches.md#verify-function-ordering-before-shipping-measurements).
Package installation gains no Python dependency: its own refusal of a policy
that does not resolve uses the Bash and `nm` resolver above.

Use `--function-order POLICY` to override the relinker's measured default.
For the full shipping path, set `OXPINYIN_FUNCTION_ORDER_POLICY` for one install invocation;
`install.sh` passes its environment to the relinker. A policy header must
match that invocation's SONAME. For example:

```sh
OXPINYIN_FUNCTION_ORDER_POLICY="$POLICIES/libpinyin.order" \
  just heavy -- tools/packaging/install.sh libpinyin --prefix="$PREFIX" \
  -- --locked --release --no-default-features --features bdb,shipped
```

Install libzhuyin separately with its own policy and normal feature selection.
An explicitly empty `OXPINYIN_FUNCTION_ORDER_POLICY=` (or `--function-order ''`
on the relinker) restores the original command with no inventory or capability
query. An unset variable selects `tools/packaging/function-order/<backend>/libpinyin.order`
or `libzhuyin.order`. A package build does not profile or infer a policy.

Ordering adds no linker replacement or packaging dependency. The relinker
resolves the policy with its existing Bash, binutils and text utilities.
Python and uv are used only for regeneration; the package build does not
require them.

### Regeneration

The policies are generated separately from each backend's shipping release inputs
by `tools/generate-function-order.py`. The generator uses the in-tree
`tools/bisection/bisect.c` worker in its teardown mode: the load constructors,
the `<facade>_init` subtree and the exports the worker calls from there to its
`M1` checkpoint (options, one instance and the first candidate) supply the
startup set, and the existing `bisect --perf` workload supplies the steady
set. No external workload or profiler coordinator is required.

The startup set covers the whole path to `M1` because the kernel maps a
library's text in 64 KiB fault-around windows: one function executed outside
the ordered block makes its whole window resident. With only `<facade>_init`
in the startup set, the code that options, the instance and the first
candidate reach lay in eight (libzhuyin) or nine (libpinyin) scattered
windows: 512 or 576 KiB of resident text (#1032).

Any cargo-c at or above the minimum that `just doctor` reports does the build,
because it only drives rustc. On 2026-10-23 UTC, built as below (Rust 1.97.1,
GNU ld 2.47, linux/amd64), cargo-c 0.10.24, 0.10.25 and 0.10.26 gave
byte-identical `libpinyin.a`, `libzhuyin.a` and relinked `libpinyin.so.15.0.0`
and `libzhuyin.so.15.0.0` at `01549f2f`, and a second 0.10.24 build matched
them too, so their `nm --demangle=rust` symbol sets are equal. On the tree of
`99c5af68` the policies regenerated as below from the 0.10.24 outputs and from
the 0.10.25 outputs were identical to each other and to the checked-in
`libpinyin.order` and `libzhuyin.order`.

Run one backend campaign at a time on ext4, with worktree and `TMPDIR` on the
home disk. Verify all three cells with `tools/cloud/setup.sh --cells 'bdb kc tkrzw'`
and source its environment file in every shell that launches a tool. Set
`CELL=bdb`, `kc` or `tkrzw`; map `kc` to Cargo feature `kyotocabinet` below.
Use one `$BUILD` target directory per backend, and separate `$STAGE`,
`$TEARDOWN` and `$WORK` directories. The #1076 campaign uses cargo-c 0.10.24
and GNU ld 2.47.

For tkrzw, the environment must name the sound libtkrzw supplied by setup
(normally `<cache>/tkrzw/lib/libtkrzw.so.1` on Ubuntu). Its `PKG_CONFIG_PATH`
selects the build dependency and its `LD_LIBRARY_PATH` selects the runtime.
Ubuntu's packaged library is defective; a result using it is invalid (rule 9).
Before each provisioning, generation and measurement campaign, check with
`ldd` that both pin facades, both installed subject facades and every measured
copy resolve exactly one `libtkrzw.so.1` to that cache library. A missing,
duplicate or system-library resolution is a STOP. The runtime guard from
#1068 checks the selected objects against the verified library named by the
environment. A discarded `LD_DEBUG=libs` trace per side using `bisect --perf`
can also confirm that its `calling init` line names the same cache library.

Select the matching verified oracle and build both libraries with ordering
explicitly disabled, then build the worker:

```sh
source "${OXPINYIN_ORACLE_ENV:-$HOME/.oxpinyin-oracle.env}"
FEATURE=$CELL
[ "$CELL" != kc ] || FEATURE=kyotocabinet
PREFIX_VAR=OXPINYIN_ORACLE_PREFIX_${CELL^^}
DATA_VAR=OXPINYIN_ORACLE_DATA_${CELL^^}
PIN=${!PREFIX_VAR}
DATA=${!DATA_VAR}
CARGO_TARGET_DIR="$BUILD" OXPINYIN_FUNCTION_ORDER_POLICY= \
  just heavy -- tools/packaging/install.sh libpinyin --prefix="$STAGE" \
  -- --locked --release --no-default-features --features "$FEATURE,shipped"
CARGO_TARGET_DIR="$BUILD" OXPINYIN_FUNCTION_ORDER_POLICY= \
  just heavy -- tools/packaging/install.sh libzhuyin --prefix="$STAGE" \
  -- --locked --release --no-default-features --features "$FEATURE"
cc -std=gnu11 -Wall -Wextra -Werror -O2 -o "$BISECT" \
  tools/bisection/bisect.c -ldl
```

`$DATA` is that cell's system directory, `$DATA_ID` its SHA-256 under the profile
manifest schema, and `$PROFILES` holds the `phrases` and `journals` fixture
trees with their `user/` directories and `profile.manifest` files. The library
and staticlib for a facade must be the same build.

Provision the profiles with the in-tree teardown provisioner against the
same-cell oracle: `$PIN` is the prefix that `tools/cloud/setup.sh` verified
(`$OXPINYIN_ORACLE_PREFIX_<CELL>` in its environment file), and
`$TEARDOWN` is an empty directory on ext4. The provisioner populates the
profiles through the pin and writes each `profile.manifest`. The `provision`
stage runs on `bdb`, `kc` and `tkrzw`; every later teardown stage stays BDB-only
(`docs/runbooks/benches.md`, "Native teardown workload"). Then compute the
system-data identity:

```sh
just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil \
  python -B -X lazy_imports=normal tools/bisection/contract-diff.py \
  "$CELL" "$PIN" "$STAGE/lib/libpinyin.so.15.0.0" \
  --zhuyin-so "$STAGE/lib/libzhuyin.so.15.0.0" \
  --unordered-arm ox --unordered-arm control \
  --teardown --teardown-stage provision --teardown-work-dir "$TEARDOWN"
PROFILES="$TEARDOWN/profiles"
DATA_ID=$(uv run --no-project --managed-python --python cpython@3.15+gil \
  python -X lazy_imports=normal tools/generate-function-order.py \
  --print-identity "$DATA")
```

```sh
just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil \
  python -X lazy_imports=normal tools/generate-function-order.py \
  --library "pinyin=$STAGE/lib/libpinyin.so.15.0.0" \
  --library "zhuyin=$STAGE/lib/libzhuyin.so.15.0.0" \
  --staticlib "pinyin=$BUILD/x86_64-unknown-linux-gnu/release/libpinyin.a" \
  --staticlib "zhuyin=$BUILD/x86_64-unknown-linux-gnu/release/libzhuyin.a" \
  --bisect "$BISECT" --system-dir "$DATA" --system-identity "$DATA_ID" \
  --profiles-dir "$PROFILES" --work-dir "$WORK" --cpu "$CPU" \
  --output-dir "tools/packaging/function-order/$CELL"
git diff --exit-code -- \
  "tools/packaging/function-order/$CELL/libpinyin.order" \
  "tools/packaging/function-order/$CELL/libzhuyin.order"
```

Run generation twice into separate temporary output directories and use `cmp`
on both facade files before replacing the checked-in policies. Keep policy
files in their own commit. Compare backend path sets before proposing a shared
prefix; implementing one requires a separate maintainer ruling.

The generator stops each measured child at the worker's teardown boundary and
keeps profiler output in memory. It requires both facades and both startup
profiles, unions the two startup sets, subtracts that union from the steady
set, and writes either policy only after every stream resolves. A missing
orderable Rust path is an error; fixed loader/CRT paths are counted and
excluded.

Repeat the campaign with `tools/measure-function-order.py`, which drives the
same worker for native timing, parent-observed `smaps` at the load and
lifecycle checkpoints, Callgrind instruction positions, and the steady
workload. The memory checkpoints are `M0` (after load) and `M1` (after the
first-candidate phase); they are lifecycle readings, not the init-stop figures
from #1000. Its Callgrind startup counters use the generator's own startup
toggles (load constructors, `<facade>_init` and the first-candidate exports),
so they cover every function the policy promotes. Policy regeneration is
separate.

```sh
just heavy -- uv run --no-project --managed-python --python cpython@3.15+gil \
  python -X lazy_imports=normal tools/measure-function-order.py \
  --subject "base=$BASE" --subject "pr=$PR" \
  --subject "control=$CONTROL" --subject "pin=$PIN" \
  --unordered-arm base \
  --bisect "$BISECT" --system-dir "$DATA" --system-identity "$DATA_ID" \
  --profiles-dir "$PROFILES" --work-dir "$WORK" --cpu "$CPU" --rounds 20
```

Here `base` is the intentionally unordered baseline; `pr` and its same-bytes
`control` must pass their policies. A different comparison can retain the
default ordered base expectation or name its own policy explicitly.

Check scoped Ir against the same-bytes control before interpreting it. A
nonexact control makes that comparison INVALID under the runbook's exact-count
rule, even when a diagnostic PR-minus-base difference is inside a pooled
control range. Do not describe it as a regression or gain. Steady Ir remains
limited by the nonexact `RandomState` control until #1031's hasher lands.

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
pkg-config names, versioned Provides at the pin's libpinyin version
(`tools/packaging/pc-version.sh`; 2.11.92), and
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
  (2.10.x) is older than the pin version (2.11.92) the library provides.

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
