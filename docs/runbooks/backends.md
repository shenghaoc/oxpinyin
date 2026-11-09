# Backends — building under each store backend

`oxpinyin-store` compiles exactly one peer backend per binary;
the store refuses zero or more than one with a `compile_error!`. tkrzw is
the workspace default. The crate map (`docs/architecture.md`) and
`backend-selection-audit` carry the why.

## Selecting a backend

```sh
cargo test --locked --workspace                                                    # default
cargo test --locked --workspace --no-default-features --features kyotocabinet
cargo test --locked --workspace --no-default-features --features bdb
```

`--no-default-features` is required for a peer: the default set already
selects one backend, and two backends at once is refused.
The store build script finds tkrzw only through `pkg-config tkrzw`; a
self-built one (`just tkrzw-build DIR`, what CI uses) is selected with
`PKG_CONFIG_PATH` and the runtime library path
([discovery and overrides](#discovery-and-overrides)).

## System libraries

| backend | Debian/Ubuntu packages | macOS (Homebrew) |
| --- | --- | --- |
| tkrzw | `libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev pkg-config g++` (the cxx bridge is C++17; no libclang) | `tkrzw`, and `export LIBRARY_PATH="$(brew --prefix)/lib"` (see [setup](setup.md#macos): `cargo test` links lz4/zstd from there, `cargo check`/`clippy` never link and are not evidence) |
| kyotocabinet | `libkyotocabinet-dev pkg-config g++` (the cxx bridge is C++) | `kyoto-cabinet`; the keg ships `kyotocabinet.pc`, pkg-config resolves it, and the store suite passes (verified on 1.2.80, 2026-09-13). The old "the KC dylib does not dlopen on macOS" note does not reproduce on today's bottled formula — the dylib dlopens by absolute path, and the backend links at link time, so bare-name lookup never enters the picture |
| bdb | `libdb-dev` (resolves to `libdb5.3-dev`) `libclang-dev pkg-config`; Fedora: `libdb-devel` | `berkeley-db@5` — 5.3.28 under the Sleepycat license, the surveyed version; the default `berkeley-db` formula is 18.1 AGPL-3.0-only and stays unusable. The keg is keg-only and ships no `.pc`, so point the [overrides](#discovery-and-overrides) at it (the build's rpath flag lets the test binaries find the dylib). Clippy and the full suite pass with the same counts as Linux — 36/0/4 (verified 2026-09-13) |

Every backend links a **system** library, the one the distribution
ships and patches. oxpinyin vendors none of them. Berkeley DB is bound
through its C header, and no copy of it is compiled into any oxpinyin
artifact. tkrzw is bound through a cxx bridge over its C++ classes
(`crates/oxpinyin-store/src/tkrzw/bridge.rs`, with the adapter
`adapter.cc`), compiled against the installed tkrzw headers and linked
to the system libtkrzw. Kyoto Cabinet is used as libpinyin uses it:
a cxx bridge (`crates/oxpinyin-store/src/kyotocabinet/bridge.rs`, with the
adapter `adapter.cc`) constructs its database classes, which are header code, so the build compiles them
from the installed Kyoto Cabinet headers into the facade, as libpinyin's
build does. The system libkyotocabinet supplies the rest. A build with
the development package missing fails at `build.rs` with a message
naming the package; it never falls back to downloading or compiling its
own copy.

### Packaging: runtime versus build dependencies

For a downstream Linux package built with a native-library backend
(tkrzw, Kyoto Cabinet, Berkeley DB), `pkg-config`, `libclang`
(Berkeley DB) and a C++ compiler (tkrzw, Kyoto Cabinet) are
build-time only: `system-deps` locates the library, `bindgen` reads the
C headers of Berkeley DB, and `cxx-build` compiles the tkrzw and Kyoto
Cabinet bridges against their C++ headers. Neither is linked, and neither appears in the
runtime dependency set — each backend splits into its own
`lib*-dev` / `lib*` pair (`libdb5.3` / `libdb5.3-dev` on Debian,
`libdb` / `libdb-devel` on Fedora). The Kyoto Cabinet build also needs a
C++ compiler at build time. Its facades then list `libstdc++.so.6` and
`libm.so.6` in `DT_NEEDED`, as libpinyin's Kyoto Cabinet build does; both
are already dependencies of libkyotocabinet.

## Discovery and overrides

`system-deps` finds every backend, from `[package.metadata.system-deps]` in
`crates/oxpinyin-store/Cargo.toml`, as libpinyin's `configure.ac` does.
tkrzw and Kyoto Cabinet are found through `tkrzw.pc` and `kyotocabinet.pc`
and a missing `.pc` file is a hard error. Berkeley DB is never looked up
through pkg-config: with no override, `db.h` is read from the compiler's
default include path and the build links `-ldb`. A self-built tkrzw is
selected with `PKG_CONFIG_PATH` and the runtime library path.

An installation outside the defaults is reached with these
`system-deps` variables. `<NAME>` is `KYOTOCABINET` or `BDB`. The first
three replace what the `.pc` file reports, and each is tracked by
`build.rs`:

| variable | effect |
| --- | --- |
| `SYSTEM_DEPS_<NAME>_INCLUDE` | the header directory (bindgen and the tkrzw and Kyoto Cabinet bridges read it) |
| `SYSTEM_DEPS_<NAME>_SEARCH_NATIVE` | the link-search path, also the rpath |
| `SYSTEM_DEPS_<NAME>_LIB` | the library name, for a name other than `db` or `kyotocabinet` (vcpkg installs `libdb48.lib`: `SYSTEM_DEPS_BDB_LIB=libdb48`) |
| `SYSTEM_DEPS_KYOTOCABINET_NO_PKG_CONFIG` | `1` skips the `.pc` lookup; `SYSTEM_DEPS_KYOTOCABINET_LIB` must then name the library |
| `SYSTEM_DEPS_BDB_NO_PKG_CONFIG` | `build.rs` defaults it to `1` and `SYSTEM_DEPS_BDB_LIB` to `db`; a value you set wins |
| `SYSTEM_DEPS_LINK`, `SYSTEM_DEPS_<NAME>_LINK` | `static` links statically |

`_INCLUDE` and `_SEARCH_NATIVE` take path lists (split on `:`, or `;` on Windows), and `_LIB` takes space-separated names.

Homebrew's `berkeley-db@5` is keg-only and ships no `.pc`. The macOS
`sanitize` recipe sets, from `brew --prefix berkeley-db@5`:
`SYSTEM_DEPS_BDB_NO_PKG_CONFIG=1`, `SYSTEM_DEPS_BDB_LIB=db`,
`SYSTEM_DEPS_BDB_SEARCH_NATIVE=<prefix>/lib` and
`SYSTEM_DEPS_BDB_INCLUDE=<prefix>/include`. Export the same four to run
`cargo` or `just check bdb` by hand. The build script never calls `brew`.

`BINDGEN_EXTRA_CLANG_ARGS` reaches bindgen as usual. Changing any of these
regenerates the declarations instead of leaving a stale set behind.
The generated bindings are not committed, deliberately — they cross the
ABI by layout rather than as opaque handles, and generating them from
the installed header keeps the declarations and the linked `.so` in
lockstep by construction. `crates/oxpinyin-store/build.rs` carries the
full reasoning.

`OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION` is CI-only. Unset or blank, the
Berkeley DB backend refuses any linked libdb other than 5.3 at open
(`check_runtime_version` in `crates/oxpinyin-store/src/bdb/ffi.rs`). Set
to a non-empty value, that comparison alone warns once — the line names
the linked version and the surveyed 5.3 — and the open continues. It
does not skip any other check. The nightly `test-windows` job
(`verify-nightly.yml`) sets it because the vcpkg
`berkeleydb` port is 4.8.30 and no surveyed build is packagable on that
runner today. A developer recipe does not set it: Linux `libdb-dev` and
Homebrew `berkeley-db@5` are the surveyed 5.3.

The two C-ABI crates additionally need `libglib2.0-dev` and `g++`; they
are Linux-first.

## Fixtures per backend

`fixtures/w3/<db|kct|tkt>/` is the committed mini data set, one
directory per backend, with each backend's native file names.
The retired generator used a Python subset of model20 followed by the pin's
program sequence from `oxpinyin-utils`, built per backend. It compared chunks
byte for byte and DBMs record by record. The frozen provenance is in
`fixtures/w3/README.md`; no generation or comparison CLI remains.

## Switching is a format transition

A user dir written by one backend is not opened by another: `user.conf`'s
`database format` line names the backend family, and a profile that does
not conform is wiped on open exactly as libpinyin's `check_format` does;
the DBM files carry libpinyin's names on Kyoto Cabinet, tkrzw and
Berkeley DB. This is what happens when a distribution switches
libpinyin's own backend. A change of the default backend is the same
transition for from-source builds: a user store written by a build on
the earlier default is wiped on open under the same rule (see
`docs/findings/compatibility-policy.md`).
