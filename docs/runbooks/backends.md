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

## Fuzzing the file ingress

The `store-open` fuzz target treats its input as a DBM file and opens it
through both container classes the runtime uses
(`ReadStore::open_read_only` for the tree tables,
`RawReadStore::open_hash_read_only` for `bigram.db`), then drives every
read the tier exposes. The backend under test is the one compiled in, so
the peer selection is a `cargo fuzz` argument:

```sh
cargo +nightly fuzz run store-open                                            # tkrzw (default)
cargo +nightly fuzz run store-open --no-default-features --features redb
cargo +nightly fuzz run store-open --no-default-features --features lmdb
cargo +nightly fuzz run store-open --no-default-features --features kyotocabinet
```

Random bytes are refused by every backend's header check, so an unseeded
run only exercises open-time rejection. To reach the record decoders
behind the header, seed the corpus with the committed store files for
the backend you are running:

```sh
tools/store/seed-store-fuzz-corpus.sh lmdb
```

### How each backend gets instrumented

`cargo fuzz` applies ASan and libFuzzer's coverage instrumentation
through `RUSTFLAGS`, which reach Rust code only. What that means per
peer:

| backend | container code | instrumented by `cargo fuzz`? |
| --- | --- | --- |
| redb | pure Rust | yes — ASan and coverage feedback end to end |
| lmdb | `mdb.c`, compiled in this build by `lmdb-master-sys` via `cc` | yes, when `CFLAGS` carries the flags (below) |
| tkrzw | system `libtkrzw.so` | no — partial, via ASan's allocator and libc interceptors |
| kyotocabinet | system `libkyotocabinet.so` | no — same as tkrzw |

LMDB is the one C backend whose source is compiled in-tree, so it is the
one whose C can be put under the same sanitizer and the same coverage
feedback as the Rust:

```sh
CC=clang CFLAGS="-fsanitize=address -fsanitize-coverage=inline-8bit-counters,pc-table,trace-cmp -fno-omit-frame-pointer -g" cargo +nightly fuzz run store-open --no-default-features --features lmdb
```

`CFLAGS` and not `CXXFLAGS`: libfuzzer-sys compiles libFuzzer itself
through `cc::Build::cpp(true)`, and libFuzzer must not carry its own
coverage instrumentation. On tkrzw and Kyoto Cabinet the container is a
distro shared object that nothing here can rebuild; the target still
earns its place there because ASan replaces the process allocator and
intercepts the libc memory routines process-wide, and because the code
this repository owns — the `table || 0x00 || key` framing, the `i32`
length conversions, the borrowed-record callbacks — is fully
instrumented and is where a hostile file's influence lands.

`docs/findings/store-file-ingress-fuzzing.md` records what each backend
actually does under a seeded corpus, and why no lane gates on the seeded
pass yet.

## Switching is a format transition

A user store written by one backend is not opened by another; the file
extension names the backend (`user_store.<kct|tkt|lmdb|redb>`). This
matches what distributions do for libpinyin's own backend switches
(`ROADMAP.md`, "tkrzw is the default selected backend").
