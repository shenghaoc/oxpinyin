# Backends — building under each store backend

`oxpinyin-store` compiles exactly one peer backend per binary;
the store refuses zero or more than one with a `compile_error!`. tkrzw is
the workspace default (maintainer decision 2026-10-30 UTC: tkrzw is maintained upstream; Berkeley DB and Kyoto Cabinet are not). The crate map (`docs/architecture.md`) and
`docs/findings/backend-selection-audit.md` carry the why.

## Selecting a backend

```sh
cargo test --locked --workspace                                                    # tkrzw (the default)
cargo test --locked --workspace --no-default-features --features kyotocabinet
cargo test --locked --workspace --no-default-features --features bdb
```

`--no-default-features` is required for a peer: the default set already
selects tkrzw (since 2026-10-30; Berkeley DB was the default 2026-09-20 →
then, tkrzw 2026-09-05 → 2026-09-20), and two backends at once is refused.
The store build script finds tkrzw only through `pkg-config tkrzw`; a
self-built one (`just tkrzw-build DIR`, what CI uses) is selected with
`PKG_CONFIG_PATH` and the runtime library path
([discovery and overrides](#discovery-and-overrides)).

## System libraries

| backend | Debian/Ubuntu packages | macOS (Homebrew) |
| --- | --- | --- |
| tkrzw (the default) | `libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev pkg-config g++` (the cxx bridge is C++17; no libclang) | `tkrzw`, and `export LIBRARY_PATH="$(brew --prefix)/lib"` (see [setup](setup.md#macos): `cargo test` links lz4/zstd from there, `cargo check`/`clippy` never link and are not evidence) |
| kyotocabinet | `libkyotocabinet-dev libclang-dev pkg-config g++` (the shim is C++) | `kyoto-cabinet`; the keg ships `kyotocabinet.pc`, pkg-config resolves it, and the store suite passes (verified on 1.2.80, 2026-09-13). The old "the KC dylib does not dlopen on macOS" note does not reproduce on today's bottled formula — the dylib dlopens by absolute path, and the backend links at link time, so bare-name lookup never enters the picture |
| bdb | `libdb-dev` (resolves to `libdb5.3-dev`) `libclang-dev pkg-config`; Fedora: `libdb-devel` | `berkeley-db@5` — 5.3.28 under the Sleepycat license, the surveyed version; the default `berkeley-db` formula is 18.1 AGPL-3.0-only and stays unusable. The keg is keg-only and ships no `.pc`, so point the [overrides](#discovery-and-overrides) at it (the build's rpath flag lets the test binaries find the dylib). Clippy and the full suite pass with the same counts as Linux — 36/0/4 (verified 2026-09-13) |

Every backend links a **system** library, the one the distribution
ships and patches. oxpinyin vendors none of them. tkrzw and Berkeley DB
are bound through their C headers, and no copy of either is compiled
into any oxpinyin artifact. Kyoto Cabinet is used as libpinyin uses it:
a C++ shim (`crates/oxpinyin-store/src/kyotocabinet/shim.cc`) constructs
its database classes, which are header code, so the build compiles them
from the installed Kyoto Cabinet headers into the facade, as libpinyin's
build does. The system libkyotocabinet supplies the rest. A build with
the development package missing fails at `build.rs` with a message
naming the package; it never falls back to downloading or compiling its
own copy.

### Packaging: runtime versus build dependencies

For a downstream Linux package built with a native-library backend
(tkrzw, Kyoto Cabinet, Berkeley DB), `pkg-config`, `libclang` (Kyoto
Cabinet and Berkeley DB) and a C++ compiler (tkrzw, Kyoto Cabinet) are
build-time only: `system-deps` locates the library, `bindgen` reads the
C headers of Berkeley DB and the Kyoto Cabinet shim, and `cxx-build`
compiles the tkrzw bridge against tkrzw's C++ headers. Neither is linked, and neither appears in the
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
| `SYSTEM_DEPS_<NAME>_INCLUDE` | the header directory (bindgen, the Kyoto Cabinet shim and the tkrzw bridge read it) |
| `SYSTEM_DEPS_<NAME>_SEARCH_NATIVE` | the link-search path, also the rpath |
| `SYSTEM_DEPS_<NAME>_LIB` | the library name, for a name other than `db` or `kyotocabinet` (vcpkg installs `libdb48.lib`: `SYSTEM_DEPS_BDB_LIB=libdb48`) |
| `SYSTEM_DEPS_KYOTOCABINET_NO_PKG_CONFIG` | `1` skips the `.pc` lookup; `SYSTEM_DEPS_KYOTOCABINET_LIB` must then name the library |
| `SYSTEM_DEPS_BDB_NO_PKG_CONFIG` | `build.rs` defaults it to `1` and `SYSTEM_DEPS_BDB_LIB` to `db`; a value you set wins |
| `SYSTEM_DEPS_LINK`, `SYSTEM_DEPS_<NAME>_LINK` | `static` links statically |

`_INCLUDE` and `_SEARCH_NATIVE` take path lists (split on `:`, or `;` on Windows), and `_LIB` takes space-separated names.

Homebrew's `berkeley-db@5` is keg-only and ships no `.pc`. The macOS
`sanitize` and `profile-w8` recipes set, from `brew --prefix berkeley-db@5`:
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

## The exactly-one-backend gate

```sh
uv run --no-project --managed-python python -X lazy_imports=normal -m tools.dev backend-matrix
```

Runs `cargo check --locked -p oxpinyin-store` for the default selection
and each of the four explicit ones, and proves
every multi-backend and zero-backend combination is refused. CI runs it
on every store-affecting change (`store-backends.yml`, `backend-matrix`).

## Fixtures per backend

`fixtures/w3/<db|kct|tkt>/` is the committed mini data set, one
directory per backend, with each backend's native file names.
`just w3-fixtures generate` writes it: a Python subset of the model20
text, then the pin's own program sequence from `oxpinyin-utils`, built
for each backend (`fixtures/w3/README.md`). `just w3-fixtures check
--cells bdb,kc,tkrzw` regenerates every directory into a temporary
location and compares it with the committed tree, chunk files byte for
byte and DBM files record by record (`goldens-and-pins.md`).

## Switching is a format transition

A user dir written by one backend is not opened by another: `user.conf`'s
`database format` line names the backend family, and a profile that does
not conform is wiped on open exactly as libpinyin's `check_format` does;
the DBM files carry libpinyin's names on Kyoto Cabinet, tkrzw and
Berkeley DB. This
matches what distributions do for libpinyin's own backend switches
(`ROADMAP.md`, the dated default-backend notes). Swapping the default
from tkrzw to Berkeley DB (2026-09-20) is one such transition for
from-source builds: a user store written by a pre-2026-09-20
default-features build is wiped on open under the same rule, and its
family line changes `Tkrzw` → `BerkeleyDB` (see
`docs/findings/compatibility-policy.md`).
