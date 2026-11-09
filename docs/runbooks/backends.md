# Backends — building under each store backend

This procedure builds and tests oxpinyin under each store backend.
`oxpinyin-store` compiles exactly one peer backend into each binary.
The store refuses zero backends, or more than one, with a `compile_error!`. tkrzw is
the workspace default. The [architecture](../architecture.md#backend-selection)
gives the reasons.

## Selecting a backend

```sh
cargo test --locked --workspace                                                    # default
cargo test --locked --workspace --no-default-features --features kyotocabinet
cargo test --locked --workspace --no-default-features --features bdb
```

A peer requires `--no-default-features`: the default set already
selects one backend, and the store refuses two backends at once.
The store build script finds tkrzw only through `pkg-config tkrzw`.
To select a self-built tkrzw (`just tkrzw-build DIR`, which CI uses), set
`PKG_CONFIG_PATH` and the runtime library path
([discovery and overrides](#discovery-and-overrides)).

## System libraries

| backend | Debian/Ubuntu packages | macOS (Homebrew) |
| --- | --- | --- |
| tkrzw | `libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev pkg-config g++` (the cxx bridge is C++17; no libclang) | `tkrzw`, and `export LIBRARY_PATH="$(brew --prefix)/lib"` (see [setup](setup.md#macos): `cargo test` links lz4/zstd from there; `cargo check`/`clippy` never link and are not evidence) |
| kyotocabinet | `libkyotocabinet-dev pkg-config g++` (the cxx bridge is C++) | `kyoto-cabinet`; the keg ships `kyotocabinet.pc` and pkg-config resolves it. The backend links at link time, so bare-name dylib lookup does not apply |
| bdb | `libdb-dev` (resolves to `libdb5.3-dev`) `libclang-dev pkg-config`; Fedora: `libdb-devel` | `berkeley-db@5`: 5.3.28 under the Sleepycat license, the surveyed version. The default `berkeley-db` formula is 18.1 AGPL-3.0-only and stays unusable. The keg is keg-only and ships no `.pc`, so point the [overrides](#discovery-and-overrides) at it (the rpath flag of the build lets the test binaries find the dylib) |

Every backend links a **system** library: the library that the distribution
ships and patches. oxpinyin vendors none of them.

- Berkeley DB is bound through its C header. No copy of it is compiled into any oxpinyin
  artifact.
- tkrzw is bound through a cxx bridge over its C++ classes
  (`crates/oxpinyin-store/src/tkrzw/bridge.rs`, with the adapter
  `adapter.cc`). The bridge is compiled against the installed tkrzw headers and linked
  to the system libtkrzw.
- Kyoto Cabinet is used as libpinyin uses it. A cxx bridge
  (`crates/oxpinyin-store/src/kyotocabinet/bridge.rs`, with the
  adapter `adapter.cc`) constructs its database classes. These classes are header code, so the build compiles them
  from the installed Kyoto Cabinet headers into the facade, as the libpinyin
  build does. The system libkyotocabinet supplies the rest.

If the development package is missing, the build fails at `build.rs` with a message
that names the package. It never falls back to downloading or compiling its
own copy.

### Packaging: runtime versus build dependencies

For a downstream Linux package built with a native-library backend
(tkrzw, Kyoto Cabinet, Berkeley DB), these tools are build-time only:

- `pkg-config`: `system-deps` uses it to locate the library;
- `libclang` (Berkeley DB): `bindgen` uses it to read the C headers of Berkeley DB;
- a C++ compiler (tkrzw, Kyoto Cabinet): `cxx-build` uses it to compile the tkrzw and Kyoto
  Cabinet bridges against their C++ headers.

None of them is linked, and none of them appears in the
runtime dependency set. Each backend splits into its own
`lib*-dev` / `lib*` pair (`libdb5.3` / `libdb5.3-dev` on Debian,
`libdb` / `libdb-devel` on Fedora). The Kyoto Cabinet build also needs a
C++ compiler at build time. Its facades then list `libstdc++.so.6` and
`libm.so.6` in `DT_NEEDED`, as the Kyoto Cabinet build of libpinyin does. Both
are already dependencies of libkyotocabinet.

## Discovery and overrides

`system-deps` finds every backend from `[package.metadata.system-deps]` in
`crates/oxpinyin-store/Cargo.toml`, as the `configure.ac` of libpinyin does.

- tkrzw and Kyoto Cabinet are found through `tkrzw.pc` and `kyotocabinet.pc`.
  A missing `.pc` file is a hard error.
- Berkeley DB is never looked up
  through pkg-config. With no override, the build reads `db.h` from the default include path of the compiler
  and links `-ldb`.
- A self-built tkrzw is
  selected with `PKG_CONFIG_PATH` and the runtime library path.

To reach an installation outside the defaults, use these
`system-deps` variables. `<NAME>` is `KYOTOCABINET` or `BDB`. The first
three replace what the `.pc` file reports, and `build.rs` tracks each of them:

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
`sanitize` recipe sets these four variables from `brew --prefix berkeley-db@5`:
`SYSTEM_DEPS_BDB_NO_PKG_CONFIG=1`, `SYSTEM_DEPS_BDB_LIB=db`,
`SYSTEM_DEPS_BDB_SEARCH_NATIVE=<prefix>/lib` and
`SYSTEM_DEPS_BDB_INCLUDE=<prefix>/include`. To run
`cargo` or `just check bdb` by hand, export the same four. The build script never calls `brew`.

`BINDGEN_EXTRA_CLANG_ARGS` reaches bindgen as usual. A change to any of these variables
regenerates the declarations, so no stale set stays behind.
The generated bindings are deliberately not committed. They cross the
ABI by layout, not as opaque handles. Generating them from
the installed header keeps the declarations and the linked `.so` in
step by construction. `crates/oxpinyin-store/build.rs` gives the
full reasons.

`OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION` is for CI hosts that have no
surveyed libdb.

- Unset or blank: the
  Berkeley DB backend refuses at open any linked libdb other than 5.3
  (`check_runtime_version` in `crates/oxpinyin-store/src/bdb/ffi.rs`).
- Set to a non-empty value: that comparison alone warns once, and the open continues.
  The warning line names the linked version and the surveyed 5.3.
  The variable does not skip any other check.

No workflow or developer recipe sets it:
Linux `libdb-dev` and Homebrew `berkeley-db@5` are the surveyed 5.3.

The two C-ABI crates also need `libglib2.0-dev` and `g++`. They
are Linux-first.

## Fixtures per backend

`fixtures/w3/<db|kct|tkt>/` is the committed mini data set. It has one
directory per backend, with the native file names of each backend.
It was written from a Python subset of the model20
text, then the pin's own program sequence from `oxpinyin-utils`, built
for each backend (`fixtures/w3/README.md`). The commands that regenerated and
checked it are retired (#1764). Recorded evidence: every regenerated directory
compared equal to the committed tree, chunk files byte for
byte and DBM files record by record ([goldens and pins](goldens-and-pins.md)).

## Switching is a format transition

A backend does not open a user directory that another backend wrote:

- The `database format` line of `user.conf` names the backend family.
- A profile that does not conform is wiped on open, exactly as the `check_format` of libpinyin does.
- The DBM files carry the libpinyin names on Kyoto Cabinet, tkrzw and
  Berkeley DB.

The same thing happens when a distribution switches the
backend of its libpinyin. A change of the default backend is the same
transition for from-source builds: a user store that a build on
the earlier default wrote is wiped on open under the same rule (see the
[compatibility policy](../findings/compatibility-policy.md)).
