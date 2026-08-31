# Design — Drop-in replacement

## Overview

Three merged pieces carry the surface: the binary identity (SONAME,
header, pkg-config — #206, #192), the compat read path (layout detection,
the `MemoryChunk` reader, the KC/tkrzw file shapes — #228), and the
output-compatibility policy that governs every divergence
(`docs/findings/compatibility-policy.md`).

## Architecture

```
crates/oxpinyin-capi/
  build.rs                 — Linux: -Wl,-soname,libpinyin.so.15; bakes the .pc
  Cargo.toml               — [package.metadata.capi] library { name = "pinyin",
                             version = "15.0.0", versioning = true };
                             header { subdirectory = "libpinyin-2.11.91" }
  libpinyin.pc.in          — @prefix@/@libdir@/@DATABASE_FORMAT@ placeholders
  pinyin.h                 — the consumer-union declarations
crates/oxpinyin-data/src/
  compat/{mod.rs, chewing_table.rs} — CompatLayout::detect + the compat load
  memory_chunk.rs                   — the MemoryChunk container reader
crates/oxpinyin-runtime/src/lib.rs — the open_compat route from pinyin_init
tools/packaging/install.sh         — fills @prefix@/@libdir@ into the .pc
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
- Consumer union: 58 symbols, declared in `pinyin.h`.

### Layout detection (`oxpinyin-data/src/compat/mod.rs`)

- `CompatLayout::detect(system_dir)` accepts a directory with a parsable
  `table.conf` (the only file whose absence fails `pinyin_init`; it
  declares the DBM, λ and the default phrase libraries) and a `bigram.db`
  whose file magic names the DBM that wrote it. Recognised layouts include
  Fedora `/usr/lib64/libpinyin/data`, Debian
  `/usr/lib/<arch>/libpinyin/data`, and NixOS profile-symlinked
  `/nix/store` paths.
- `load(dir, &layout)` converts the directory at load time into the same
  in-memory model the native tables produce, so the decode path above is
  byte-for-byte the code that runs on oxpinyin's own data.

### MemoryChunk (`oxpinyin-data/src/memory_chunk.rs`)

- 8-byte header: `u32` LE length of the data section, then `u32` XOR
  checksum over it (mirrored from `memory_chunk.h::get_check_sum`); the
  payload follows. Parsed by byte-offset `from_le_bytes` reads — no
  pointer casts; the checksum is verified before use.

### Backend file shapes

- Content tables (`gb_char.bin`, `merged.bin`, …): backend-independent
  `MemoryChunk` images holding the `SubPhraseIndex` structures.
- `phrase_index.bin` / `pinyin_index.bin` / `punct.bin`: the build DBM's
  tree databases (TreeDB for Kyoto Cabinet, TreeDBM for tkrzw — despite
  the extension). The in-memory model derives every lookup structure from
  the content tables, so these serve detection and punct reading.
- `bigram.db`: the build DBM's hash database keyed by the raw `u32`
  token, valued by a `SingleGram` chunk.

## Out of scope / shelved

- The `MemoryChunk` write path for user data (learned bigrams written
  back in libpinyin's format) — pending.
- The BerkeleyDB compat path — SHELVED
  (`docs/findings/berkeleydb-compat-phase1.md`); the incomplete
  implementation lives on `feat/bdb-backend`.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) — `MemoryChunk` format, DBM layout, and the data directory structure are documented there.
