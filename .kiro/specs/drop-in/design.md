# Design — Drop-in: the libpinyin binary

## Overview

Two merged pieces make an oxpinyin build a libpinyin binary: the binary
identity (SONAME, header, pkg-config — #206, #192) and native storage on
libpinyin's three backends. The runtime opens its backend's system data
directory the way `pinyin_init` does (P6, 2026-09-02) and keeps the user
dir as that backend's native user files (task 9, 2026-09-09; task 10
added Berkeley DB, 2026-09-12). Every difference from the pin is governed
by `docs/findings/compatibility-policy.md`.

## Architecture

```text
crates/oxpinyin-capi/
  build.rs                 — Linux: -Wl,-soname,libpinyin.so.15; bakes the .pc
  Cargo.toml               — [package.metadata.capi] library { name = "pinyin",
                             version = "15.0.0", versioning = true };
                             header { subdirectory = "libpinyin-2.11.92" }
                             (checked by build.rs against oracle-pin.txt)
  build_pin_version.rs     — reads libpinyin_tag from tools/oracle/oracle-pin.txt
  libpinyin.pc.in          — the pin's template: @prefix@/@exec_prefix@/
                             @libdir@/@includedir@/@VERSION@/@DATABASE_FORMAT@
  pinyin.h, libpinyin.ver  — the declarations and the 79-symbol export list
crates/oxpinyin-data/src/
  system_files.rs          — the backend's system file names, the chunk files,
                             the addon library table
  chunk_format.rs, phrase_library.rs — the MemoryChunk reader (mmap, checksum)
  row_format.rs            — the four DBM row layouts, shared with datagen
  chewing_table.rs, phrase_table.rs, bigram_table.rs, punct.rs, table_conf.rs
                           — lazy readers: a handle plus a point read each
  user_files.rs            — user.conf codec, the user DBM names, .dbin records
crates/oxpinyin-user/src/persistence.rs — the user dir, native per backend
crates/oxpinyin-runtime/src/lib.rs      — Runtime::open(system_dir, user_dir),
                                          what pinyin_init does
tools/packaging/install.sh              — fills @prefix@/@exec_prefix@/@libdir@/
                                          @includedir@ into the .pc as autoconf does
```

## Components and Interfaces

### Binary identity (`oxpinyin-capi`)

- SONAME: `cargo:rustc-cdylib-link-arg=-Wl,-soname,libpinyin.so.15`
  (Linux); cargo-c `[package.metadata.capi.library]` name `pinyin`,
  version `15.0.0`, `versioning = true` (libtool -version-info 15:0,
  confirmed against Ubuntu's shipped `libpinyin15`).
- Header: `subdirectory = "libpinyin-2.11.92"`; asset install of
  `pinyin.h` into the same subdirectory. The version is the pin's
  (`libpinyin_tag` in `tools/oracle/oracle-pin.txt`); `build.rs` reads
  the pin record and fails the build when the `[package.metadata.capi]`
  tables disagree with it (version ruling 2026-09-26 UTC, #592).
- pkg-config: `libpinyin.pc.in` is the pin's template line for line
  (`pkgdatadir`, `database_format`, `exec_prefix`); `build.rs` bakes
  `@VERSION@`/`@DATABASE_FORMAT@`, and `tools/packaging/install.sh` fills
  `@prefix@`/`@exec_prefix@`/`@libdir@`/`@includedir@` as the pin's
  configure does. `tools/packaging/check-pc-metadata.sh` diffs the
  result against a pin install.
- Exported surface: the full live upstream ABI, 79 `pinyin_*` symbols
  (`docs/findings/abi-reference.md`, `tools/abi/check-exports.sh`); no
  symbol is scoped by which consumer calls it.

### Storage (`oxpinyin-store`, `oxpinyin-data`, `oxpinyin-runtime`)

- One backend per build — `bdb` (default), `kyotocabinet` or `tkrzw` —
  selected by cargo feature as libpinyin selects with `--with-dbm`;
  `oxpinyin-store` refuses a build naming none or several.
- `Runtime::open(system_dir, user_dir)` opens the system directory the
  way `pinyin_init` does: the three required DBMs (`pinyin_index`,
  `phrase_index`, `bigram`), the per-library chunk files, and optionally
  `table.conf` (λ), `punct` and the addon DBM pair. The caller supplies
  the directory (`StoragePaths`); no distro layout is auto-detected.
- File names and record layouts are the backend's native ones
  (`pinyin_index.bin`, `bigram.db`, …): the set a libpinyin build on that
  backend uses, and the set `oxpinyin-datagen compile` writes.
- Nothing is converted or scanned at open — a handle plus a point read
  per file, chunk files mmapped and checksummed.

### MemoryChunk (`oxpinyin-data/src/chunk_format.rs`, `phrase_library.rs`)

- 8-byte header: `u32` LE length of the data section, then `u32` XOR
  checksum over it (mirrored from `memory_chunk.h::get_check_sum`); the
  payload follows. Read by byte offset from the mapped file — no
  pointer casts; the checksum is verified before use.

### Backend file shapes

- Content tables (`gb_char.bin`, `merged.bin`, …): backend-independent
  `MemoryChunk` images holding the `SubPhraseIndex` structures.
- `phrase_index` / `pinyin_index` / `punct`: the backend's tree databases
  (TreeDB on Kyoto Cabinet, TreeDBM on tkrzw, a B-tree on Berkeley DB),
  read through point lookups.
- `bigram`: the backend's hash database keyed by the raw `u32` token,
  valued by a `SingleGram` chunk.

### User data

The user dir is the backend's native user file set — `user.conf`,
`user_bigram`, `user_pinyin_index`, `user_phrase_index`, `user.bin` and
the `*.dbin` diff logs — loaded through `check_format` and saved whole to
`.tmp` siblings then renamed, as the pin does (`docs/findings/user-store.md`
§11). Builds on the same backend, libpinyin or oxpinyin, use the same
user dir; a build on another backend cannot read it and starts fresh, as
libpinyin's own builds do. Measured by `tools/oracle/user-dir-round-trip.sh`
against pin-built oracles (task 9).

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) — the n-gram model format, internals and multiple-dictionary layout are documented there; the pinned source (`074a2219`) is the authority where the two differ.
