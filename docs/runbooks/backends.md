# Backends — building under each store backend

`oxpinyin-store` compiles exactly one of four peer backends per binary;
the store refuses zero or more than one with a `compile_error!`. tkrzw is
the workspace default. The crate map (`.kiro/steering/structure.md`) and
`docs/findings/backend-selection-audit.md` carry the why.

## Selecting a backend

```sh
cargo test --locked --workspace                                                    # tkrzw (default)
cargo test --locked --workspace --no-default-features --features kyotocabinet
cargo test --locked --workspace --no-default-features --features lmdb
cargo test --locked --workspace --no-default-features --features redb              # pure Rust, macOS/Windows
```

`--no-default-features` is required for a peer: the default set already
selects tkrzw, and two backends at once is refused.

## System libraries

| backend | Debian/Ubuntu packages | macOS (Homebrew) |
| --- | --- | --- |
| tkrzw | `libtkrzw-dev liblzma-dev liblz4-dev libzstd-dev zlib1g-dev libclang-dev pkg-config` | `tkrzw`, and `export LIBRARY_PATH="$(brew --prefix)/lib"` (see README: `cargo test` links lz4/zstd from there, `cargo check`/`clippy` never link and are not evidence) |
| kyotocabinet | `libkyotocabinet-dev libclang-dev pkg-config` | not supported (the KC dylib does not dlopen on macOS; use a Linux container) |
| lmdb | `liblmdb-dev libclang-dev pkg-config` | `lmdb` |
| redb | none | none |

Three of the four backends bind a **system** C library through its own
header, and only redb is pure Rust. oxpinyin vendors none of them: there
is no copy of `mdb.c`, of Kyoto Cabinet or of tkrzw compiled into any
oxpinyin artifact, so each library is the one the distribution ships and
patches. A build with the development package missing fails at
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
split applies to the tkrzw and Kyoto Cabinet backends against their own
`lib*-dev` / `lib*` pairs.

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

Runs `cargo check --workspace` for the four valid selections and proves
every multi-backend and zero-backend combination is refused. CI runs it
on every store-affecting change (`store-backends.yml`, `backend-matrix`).

## Fixtures per backend

`fixtures/w3/<kct|tkt|lmdb|redb>/` is the committed mini data set, one
directory per backend, with libpinyin's own file names on KC and tkrzw.
Regenerate with `oxpinyin-datagen compile --mini` from the model20 cache
(`goldens-and-pins.md`).

Opening a committed `fixtures/w3/lmdb/*` DBM rewrites its `-lock`
sidecar — LMDB does that on every open, read-only included; the data
files never change. The sidecars are gitignored
(`/fixtures/**/*.lmdb-lock`) and untracked since 2026-09-06. If one
ever shows up in `git status`, the ignore pattern regressed: fix the
pattern, do not commit the file (AGENTS.md points here).

## Switching is a format transition

A user store written by one backend is not opened by another; the file
extension names the backend (`user_store.<kct|tkt|lmdb|redb>`). This
matches what distributions do for libpinyin's own backend switches
(`ROADMAP.md`, "tkrzw is the default selected backend").
