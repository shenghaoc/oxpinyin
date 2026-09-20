# Backends — building under each store backend

`oxpinyin-store` compiles exactly one of four peer backends per binary;
the store refuses zero or more than one with a `compile_error!`. tkrzw is
the workspace default. The crate map (`.kiro/steering/structure.md`) and
`docs/findings/backend-selection-audit.md` carry the why.

## Selecting a backend

```sh
cargo test --locked --workspace                                                    # tkrzw (default)
cargo test --locked --workspace --no-default-features --features kyotocabinet
cargo test --locked --workspace --no-default-features --features redb              # pure Rust, macOS/Windows
cargo test --locked --workspace --no-default-features --features bdb                # Berkeley DB, libpinyin's original DBM
```

`--no-default-features` is required for a peer: the default set already
selects tkrzw, and two backends at once is refused.

## System libraries

| backend | Debian/Ubuntu packages | macOS (Homebrew) |
| --- | --- | --- |
| tkrzw | `libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libclang-dev pkg-config` | `tkrzw`, and `export LIBRARY_PATH="$(brew --prefix)/lib"` (see README: `cargo test` links lz4/zstd from there, `cargo check`/`clippy` never link and are not evidence) |
| kyotocabinet | `libkyotocabinet-dev libclang-dev pkg-config` | `kyoto-cabinet`; the keg ships `kyotocabinet.pc`, pkg-config resolves it, and the store suite passes (verified on 1.2.80, 2026-09-13). The old "the KC dylib does not dlopen on macOS" note does not reproduce on today's bottled formula — the dylib dlopens by absolute path, and the backend links at link time, so bare-name lookup never enters the picture |
| bdb | `libdb-dev` (resolves to `libdb5.3-dev`) `libclang-dev pkg-config`; Fedora: `libdb-devel` | `berkeley-db@5` — 5.3.28 under the Sleepycat license, the surveyed version; the default `berkeley-db` formula is 18.1 AGPL-3.0-only and stays unusable. The keg ships no `.pc`, so point the overrides at it: `OXPINYIN_BDB_INCLUDE_DIR="$(brew --prefix)/opt/berkeley-db@5/include"` and `OXPINYIN_BDB_LIB_DIR="$(brew --prefix)/opt/berkeley-db@5/lib"` (the build's rpath flag lets the test binaries find the dylib). Clippy and the full suite pass with the same counts as Linux — 36/0/4 (verified 2026-09-13) |
| redb | none | none |

Three of the four backends bind a **system** C library through its own
header, and only redb is pure Rust. oxpinyin vendors none of them: there
is no copy of Kyoto Cabinet, of tkrzw or of Berkeley DB
compiled into any oxpinyin artifact, so each library is the one the
distribution ships and patches. A build with the development package missing fails at
`build.rs` with a message naming the package; it never falls back to
downloading or compiling its own copy.

### Packaging: runtime versus build dependencies

For a downstream Linux package built with a C-library backend (tkrzw,
Kyoto Cabinet, Berkeley DB), `libclang` and `pkg-config` are build-time
only: `pkg-config` locates the library and `bindgen` reads its header to
generate the declarations. Neither is linked, and neither appears in the
runtime dependency set — each backend splits into its own
`lib*-dev` / `lib*` pair (`libdb5.3` / `libdb5.3-dev` on Debian,
`libdb` / `libdb-devel` on Fedora).

An installation `pkg-config` cannot find — a custom prefix, or a build
that ships no `.pc` file — is reachable without it:
`OXPINYIN_KC_INCLUDE_DIR` prepends a header directory and
`OXPINYIN_KC_LIB_DIR` adds a link-search path and an rpath, mirroring
the `OXPINYIN_BDB_*` pair. `BINDGEN_EXTRA_CLANG_ARGS`
reaches bindgen as usual. All are tracked by `build.rs`, so changing one
regenerates the declarations instead of leaving a stale set behind.
The generated bindings are not committed, deliberately — they cross the
ABI by layout rather than as opaque handles, and generating them from
the installed header keeps the declarations and the linked `.so` in
lockstep by construction. `crates/oxpinyin-store/build.rs` carries the
full reasoning.

The two C-ABI crates additionally need `libglib2.0-dev` and `g++`; they
are Linux-first. `oxpinyin-dictool` depends on `oxpinyin-capi` and so
inherits that.

## The exactly-one-backend gate

```sh
tools/store/backend-matrix.sh
```

Runs `cargo check --locked -p oxpinyin-store` for the default selection
and each of the four explicit ones, and proves
every multi-backend and zero-backend combination is refused. CI runs it
on every store-affecting change (`store-backends.yml`, `backend-matrix`).

## Fixtures per backend

`fixtures/w3/<kct|tkt|db|redb>/` is the committed mini data set, one
directory per backend, with libpinyin's own file names on KC, tkrzw and
Berkeley DB.
Regenerate with `oxpinyin-datagen compile --mini` from the model20 cache
(`goldens-and-pins.md`).

## Switching is a format transition

A user dir written by one backend is not opened by another: `user.conf`'s
`database format` line names the backend family, and a profile that does
not conform is wiped on open exactly as libpinyin's `check_format` does;
the DBM files carry libpinyin's names on Kyoto Cabinet, tkrzw and
Berkeley DB, and `<stem>.<ext>` on redb. This
matches what distributions do for libpinyin's own backend switches
(`ROADMAP.md`, "tkrzw is the default selected backend").
