# Design — Drop-in: the libpinyin binary

## Overview

Two merged parts establish the default binary: identity (SONAME, header, pkg-config; #206, #192) and native storage on libpinyin's three backends.
Runtime opens system data as `pinyin_init` does (P6, 2026-09-02).
It keeps native user files (task 9, 2026-09-09).
Task 10 added Berkeley DB on 2026-09-12.
The [compatibility policy](../../findings/compatibility-policy.md) governs every difference.

## Architecture

| Component | Responsibility |
|---|---|
| capi `build.rs` | Linux SONAME and baked pkg-config values; validates cargo-c metadata against `oracle-pin.txt`. |
| capi `Cargo.toml` | Library name `pinyin`, version `15.0.0`, `versioning = true`; header subdirectory `libpinyin-2.11.92`. |
| capi `build_pin_version.rs` | Reads `libpinyin_tag` from `tools/oracle/oracle-pin.txt`. |
| capi `libpinyin.pc.in` | Pin template with prefix, exec_prefix, libdir, includedir, VERSION and DATABASE_FORMAT substitutions. |
| capi `pinyin.h`, `libpinyin.ver` | Declarations and the 79-symbol export list. |
| data `system_files.rs` | Native system filenames, chunks and addon library table. |
| data `chunk_format.rs`, `phrase_library.rs` | Mapped, checksummed MemoryChunk reader. |
| data `row_format.rs` | Four DBM row layouts shared with datagen. |
| data `chewing_table.rs`, `phrase_table.rs`, `bigram_table.rs`, `punct.rs`, `table_conf.rs` | Lazy readers with a handle and point read each. |
| data `user_files.rs` | user.conf codec, user DBM names and .dbin records. |
| user `persistence.rs` | Native user directory for each backend. |
| runtime `lib.rs` | `Runtime::open(system_dir, user_dir)`, following `pinyin_init`. |
| `tools/packaging/install.sh` | Fills prefix, exec_prefix, libdir and includedir in pkg-config as autoconf does. |

## Components and Interfaces

### Binary identity (`oxpinyin-capi`)

Set Linux SONAME with `cargo:rustc-cdylib-link-arg=-Wl,-soname,libpinyin.so.15`.
Cargo-c library metadata uses name `pinyin`, version `15.0.0` and `versioning = true`.
This is libtool -version-info 15:0, confirmed against Ubuntu's `libpinyin15`.
Install `pinyin.h` under `libpinyin-2.11.92`.
The pin's `libpinyin_tag` determines that directory.
`build.rs` rejects metadata that disagrees with the pin (version ruling 2026-09-26 UTC, #592).

Use the pin's `libpinyin.pc.in` template line for line, including `pkgdatadir`, `database_format` and `exec_prefix`.
`build.rs` bakes `@VERSION@` and `@DATABASE_FORMAT@`.
The install script fills `@prefix@`, `@exec_prefix@`, `@libdir@` and `@includedir@` as the pin's configure does.
`tools/packaging/check-pc-metadata.sh` compares the result with a pin install.
Export the full 79-symbol live ABI, regardless of consumer usage.
See `docs/findings/abi-reference.md` and `tools/abi/check-exports.sh`.

### Storage (`oxpinyin-store`, `oxpinyin-data`, `oxpinyin-runtime`)

Select one cargo backend feature: `tkrzw` by default, `bdb` or `kyotocabinet`.
This mirrors libpinyin's `--with-dbm`.
Store rejects zero or multiple backends.
Open the system directory through `Runtime::open(system_dir, user_dir)` as `pinyin_init` does.
Required DBMs are `pinyin_index`, `phrase_index` and `bigram`, alongside per-library chunks.
Optional inputs are `table.conf` for λ, punctuation and the addon DBM pair.
The caller supplies `StoragePaths`.
Do not auto-detect a distribution layout.

Use native names and layouts, including `pinyin_index.bin` and `bigram.db`.
Libpinyin on that backend and the pin's own `gen_binary_files` →
`import_interpolation` → `gen_unigram` sequence (the `oxpinyin-utils`
binaries) use the same set.
Do not convert or scan data at open.
Use a handle and point read per file, with mapped and checksummed chunks.

### MemoryChunk (`oxpinyin-data/src/chunk_format.rs`, `phrase_library.rs`)

Parse the 8-byte header by byte offset, without pointer casts.
It contains a u32 LE data length followed by a u32 XOR data checksum.
The checksum mirrors `memory_chunk.h::get_check_sum`.
The payload follows.
Verify the checksum before use.

### Backend file shapes

- Content tables, including `gb_char.bin` and `merged.bin`, are backend-independent MemoryChunk images containing SubPhraseIndex structures.
- Phrase, pinyin and punctuation indexes use tree databases with point lookups.
  These are Kyoto Cabinet TreeDB, tkrzw TreeDBM or Berkeley DB B-tree.
- Bigram uses a hash database keyed by raw u32 tokens, with SingleGram chunk values.

### User data

Use the native `user.conf`, `user_bigram`, `user_pinyin_index`, `user_phrase_index`, `user.bin` and `*.dbin` files.
Load through `check_format`.
Save whole files to `.tmp` siblings, then rename them as the pin does.
See `docs/findings/user-store.md` §11.
Same-backend libpinyin and oxpinyin builds use the same user directory.
A different backend starts fresh because it cannot read those files.
Task 9 measured both directions with `tools/oracle/user-dir-round-trip.sh` against pin-built oracles.

The [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) documents model formats, internals and multiple dictionaries.
Use the pinned source (`074a2219`) when it differs.
