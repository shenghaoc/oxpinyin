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
| lmdb | none (heed builds LMDB from source) | none |
| redb | none | none |

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
