# Design — Drop-in replacement

## Overview

Three merged pieces carry the surface: the binary identity (SONAME,
header, pkg-config — #206, #192), the direct data path — the runtime
opens libpinyin's own data directory in place through `oxpinyin-data`'s
readers (P6, 2026-09-02, which replaced the #228 compatibility layer),
with learned user data read and written in libpinyin's own user-file set
(task 9, 2026-09-09) — and the output-compatibility policy that governs
every divergence (`docs/findings/compatibility-policy.md`).

## Architecture

```text
crates/oxpinyin-capi/
  build.rs                 — Linux: -Wl,-soname,libpinyin.so.15; bakes the .pc
  Cargo.toml               — [package.metadata.capi] library { name = "pinyin",
                             version = "15.0.0", versioning = true };
                             header { subdirectory = "libpinyin-2.11.91" }
  libpinyin.pc.in          — @prefix@/@libdir@/@DATABASE_FORMAT@ placeholders
  pinyin.h, libpinyin.ver  — the declarations and the 79-symbol export list
crates/oxpinyin-data/src/
  system_files.rs          — libpinyin's file inventory: DBM names per backend
                             family, the chunk files, the addon library table
  chunk_format.rs, phrase_library.rs — the MemoryChunk reader (mmap, checksum)
  row_format.rs            — the four DBM row layouts, shared with datagen
  chewing_table.rs, phrase_table.rs, bigram_table.rs, punct.rs, table_conf.rs
                           — lazy readers: a handle plus a point read each
  user_files.rs            — user.conf codec, the user DBM names, .dbin records
crates/oxpinyin-user/src/persistence.rs — the user dir in libpinyin's own shapes
crates/oxpinyin-runtime/src/lib.rs      — Runtime::open(system_dir, user_dir),
                                          what pinyin_init does
tools/packaging/install.sh              — fills @prefix@/@libdir@ into the .pc
```

## Components and Interfaces

### Binary identity (`oxpinyin-capi`)

- SONAME: `cargo:rustc-cdylib-link-arg=-Wl,-soname,libpinyin.so.15`
  (Linux); cargo-c `[package.metadata.capi.library]` name `pinyin`,
  version `15.0.0`, `versioning = true` (libtool -version-info 15:0,
  confirmed against Ubuntu's shipped `libpinyin15`).
- Header: `subdirectory = "libpinyin-2.11.91"`; asset install of
  `pinyin.h` into the same subdirectory.
- pkg-config: `libpinyin.pc.in` carries `pkgdatadir`, `database_format`
  and `exec_prefix`; `build.rs` bakes `@VERSION@`/`@DATABASE_FORMAT@`,
  and `tools/packaging/install.sh` fills `@prefix@`/`@libdir@`.
- Exported surface: the full live upstream ABI, 79 `pinyin_*` symbols
  (`docs/findings/abi-subset.md` §6, `tools/abi/check-exports.sh`); the
  58-symbol consumer union is the subset the two reference consumers
  call.

### Direct data path (`oxpinyin-runtime`, `oxpinyin-data`)

- `Runtime::open(system_dir, user_dir)` opens a directory the way
  `pinyin_init` does: the three required DBMs (`pinyin_index`,
  `phrase_index`, `bigram`), the per-library chunk files, and optionally
  `table.conf` (λ), `punct` and the addon DBM pair. The caller supplies
  the directory (`StoragePaths`); no distro layout is auto-detected.
- The DBM file names follow the compiled-in backend family: libpinyin's
  own (`pinyin_index.bin`, `bigram.db`, …) on Kyoto Cabinet, tkrzw and
  Berkeley DB, so an unmodified install's `data/` opens as is;
  `<stem>.<ext>` on redb and LMDB, which no libpinyin build writes. On every backend the
  directory `oxpinyin-datagen compile` writes opens the same way.
- Nothing is converted or scanned at open — a handle plus a point read
  per file, chunk files mmapped and checksummed. There is no
  compatibility layer: the readers that open an install are the readers
  that open oxpinyin's own output
  (`docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`).

### MemoryChunk (`oxpinyin-data/src/chunk_format.rs`, `phrase_library.rs`)

- 8-byte header: `u32` LE length of the data section, then `u32` XOR
  checksum over it (mirrored from `memory_chunk.h::get_check_sum`); the
  payload follows. Read by byte offset from the mapped file — no
  pointer casts; the checksum is verified before use.

### Backend file shapes

- Content tables (`gb_char.bin`, `merged.bin`, …): backend-independent
  `MemoryChunk` images holding the `SubPhraseIndex` structures.
- `phrase_index` / `pinyin_index` / `punct`: the build DBM's tree
  databases (TreeDB for Kyoto Cabinet, TreeDBM for tkrzw — despite the
  `.bin` extension), read in place through point lookups.
- `bigram`: the build DBM's hash database keyed by the raw `u32` token,
  valued by a `SingleGram` chunk.
- User dir: `user.conf`, `user_bigram` (a Kyoto Cabinet StashDB snapshot
  or a tkrzw HashDBM — its own container, not the system bigram's),
  `user_pinyin_index`, `user_phrase_index`, `user.bin` and the `*.dbin`
  diff logs — loaded through `check_format`, saved whole to `.tmp`
  siblings then renamed (`docs/findings/user-store.md` §11).

### User data (task 9, landed 2026-09-09)

An oxpinyin built on Kyoto Cabinet and a libpinyin built on Kyoto
Cabinet — likewise the tkrzw pair — read and write the same user dir,
and a swap in either direction carries the learned data with it;
measured by `tools/oracle/user-dir-round-trip.sh` against pin-built
oracles, 10/10 export rows byte-identical each way on both backends
(`docs/findings/compatibility-policy.md`, goal amendment 2026-09-09).
Fresh-start applies only across a genuine KV-backend change.

## Out of scope / shelved

- ~~The BerkeleyDB backend — SHELVED~~ Landed 2026-09-12 as the fifth
  store peer (`docs/findings/berkeleydb-backend.md`); the shelved
  first cut on `feat/bdb-backend` supplied its FFI shape and
  byte-layout evidence.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) — the n-gram model format, internals and multiple-dictionary layout are documented there; the pinned source (`074a2219`) is the authority where the two differ.
