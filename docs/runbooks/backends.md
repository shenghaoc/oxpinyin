# Backends — building under each store backend

`oxpinyin-store` compiles exactly one of five peer backends per binary;
the store refuses zero or more than one with a `compile_error!`. tkrzw is
the workspace default. The crate map (`.kiro/steering/structure.md`) and
`docs/findings/backend-selection-audit.md` carry the why.

## Selecting a backend

```sh
cargo test --locked --workspace                                                    # tkrzw (default)
cargo test --locked --workspace --no-default-features --features kyotocabinet
cargo test --locked --workspace --no-default-features --features lmdb
cargo test --locked --workspace --no-default-features --features redb              # pure Rust, macOS/Windows
cargo test --locked --workspace --no-default-features --features bdb                # Berkeley DB, libpinyin's original DBM
```

`--no-default-features` is required for a peer: the default set already
selects tkrzw, and two backends at once is refused.

## System libraries

| backend | Debian/Ubuntu packages | macOS (Homebrew) |
| --- | --- | --- |
| tkrzw | `libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libclang-dev pkg-config` | `tkrzw`, and `export LIBRARY_PATH="$(brew --prefix)/lib"` (see README: `cargo test` links lz4/zstd from there, `cargo check`/`clippy` never link and are not evidence) |
| kyotocabinet | `libkyotocabinet-dev libclang-dev pkg-config` | not supported (the KC dylib does not dlopen on macOS; use a Linux container) |
| lmdb | `liblmdb-dev libclang-dev pkg-config` | `lmdb pkgconf`; libclang ships with the Xcode Command Line Tools. If `pkg-config --cflags lmdb` comes back empty, add Homebrew's metadata directory: `export PKG_CONFIG_PATH="$(brew --prefix)/lib/pkgconfig"` |
| bdb | `libdb-dev` (resolves to `libdb5.3-dev`) `libclang-dev pkg-config`; Fedora: `libdb-devel` | not supported — the Homebrew formula ships the AGPL-licensed 18.1, and the backend surveys 5.3 only; use a Linux container |
| redb | none | none |

Four of the five backends bind a **system** C library through its own
header, and only redb is pure Rust. oxpinyin vendors none of them: there
is no copy of `mdb.c`, of Kyoto Cabinet, of tkrzw or of Berkeley DB
compiled into any oxpinyin artifact, so each library is the one the
distribution ships and patches. A build with the development package missing fails at
`build.rs` with a message naming the package; it never falls back to
downloading or compiling its own copy.

### Packaging: runtime versus build dependencies

For a downstream Linux package built with `--features lmdb`:

| | Debian/Ubuntu | Fedora | Arch |
| --- | --- | --- | --- |
| **runtime** (`Depends`) | `liblmdb0` | `lmdb-libs` | `lmdb` |
| **build** (`Build-Depends`) | `liblmdb-dev`, `libclang-dev`, `pkg-config` | `lmdb-devel`, `clang-devel`, `pkgconf` | `lmdb`, `clang`, `pkgconf` |

`libclang` and `pkg-config` are build-time only: `pkg-config` locates the
library and `bindgen` reads `lmdb.h` to generate the declarations. Neither
is linked, and neither appears in the runtime dependency set. The same
split applies to the tkrzw, Kyoto Cabinet and Berkeley DB backends
against their own `lib*-dev` / `lib*` pairs (`libdb5.3` /
`libdb5.3-dev` on Debian, `libdb` / `libdb-devel` on Fedora).

An installation `pkg-config` cannot find — a custom prefix, or a build
that ships no `.pc` file — is reachable without it:
`OXPINYIN_LMDB_INCLUDE_DIR` prepends a header directory and
`OXPINYIN_LMDB_LIB_DIR` adds a link-search path and an rpath, mirroring
the `OXPINYIN_KC_*` and `OXPINYIN_BDB_*` pairs. `BINDGEN_EXTRA_CLANG_ARGS`
reaches bindgen as usual. All are tracked by `build.rs`, so changing one
regenerates the declarations instead of leaving a stale
`lmdb_bindings.rs` behind.

The generated bindings are not committed, deliberately: `MDB_val` and
`MDB_stat` cross the ABI by layout rather than as opaque handles, and the
`MDB_NOTFOUND` / `MDB_MAP_FULL` / `MDB_DBS_FULL` codes the backend
branches on are `#define`s. Generating them from the installed header
keeps the declarations and the linked `.so` in lockstep by construction.
`crates/oxpinyin-store/build.rs` carries the full reasoning.

The two C-ABI crates additionally need `libglib2.0-dev` and `g++`; they
are Linux-first. `oxpinyin-dictool` depends on `oxpinyin-capi` and so
inherits that.

## The exactly-one-backend gate

```sh
tools/store/backend-matrix.sh
```

Runs `cargo check --locked -p oxpinyin-store` for the default selection
and each of the five explicit ones, and proves
every multi-backend and zero-backend combination is refused. CI runs it
on every store-affecting change (`store-backends.yml`, `backend-matrix`).

## Fixtures per backend

`fixtures/w3/<kct|tkt|db|lmdb|redb>/` is the committed mini data set, one
directory per backend, with libpinyin's own file names on KC, tkrzw and
Berkeley DB.
Regenerate with `oxpinyin-datagen compile --mini` from the model20 cache
(`goldens-and-pins.md`).

Opening a committed `fixtures/w3/lmdb/*` DBM rewrites its `-lock`
sidecar — LMDB does that on every open, read-only included; the data
files never change. The sidecars are gitignored
(`/fixtures/**/*.lmdb-lock`) and untracked since 2026-09-06. If one
ever shows up in `git status`, the ignore pattern regressed: fix the
pattern, do not commit the file (AGENTS.md points here).

## Switching is a format transition

A user dir written by one backend is not opened by another: `user.conf`'s
`database format` line names the backend family, and a profile that does
not conform is wiped on open exactly as libpinyin's `check_format` does;
the DBM files carry libpinyin's names on Kyoto Cabinet, tkrzw and
Berkeley DB, and `<stem>.<ext>` on redb and LMDB. This
matches what distributions do for libpinyin's own backend switches
(`ROADMAP.md`, "tkrzw is the default selected backend").
