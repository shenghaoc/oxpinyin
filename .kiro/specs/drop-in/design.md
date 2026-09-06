# Design — Drop-in replacement

## Overview

Two merged pieces carry the surface: the binary identity (SONAME,
header, pkg-config — #206, #192) and the output-compatibility policy
that governs every divergence (`docs/findings/compatibility-policy.md`).

## Architecture

```text
crates/oxpinyin-capi/
  build.rs                 — Linux: -Wl,-soname,libpinyin.so.15; bakes the .pc
  Cargo.toml               — [package.metadata.capi] library { name = "pinyin",
                             version = "15.0.0", versioning = true };
                             header { subdirectory = "libpinyin-2.11.91" }
  libpinyin.pc.in          — @prefix@/@libdir@/@DATABASE_FORMAT@ placeholders
  pinyin.h                 — the declarations
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

### Backend file shapes

- Content tables (`gb_char.bin`, `merged.bin`, …): backend-independent
  `MemoryChunk` images holding the `SubPhraseIndex` structures.
- `phrase_index.bin` / `pinyin_index.bin` / `punct.bin`: the build DBM's
  tree databases (TreeDB for Kyoto Cabinet, TreeDBM for tkrzw — despite
  the extension).
- `bigram.db`: the build DBM's hash database keyed by the raw `u32`
  token, valued by a `SingleGram` chunk.

## Out of scope / shelved

- The `MemoryChunk` write path for user data (learned bigrams written
  back in libpinyin's format) — pending.
- The BerkeleyDB compat path — SHELVED
  (`docs/findings/berkeleydb-compat-phase1.md`); the incomplete
  implementation lives on `feat/bdb-backend`.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) — `MemoryChunk` format, DBM layout, and the data directory structure are documented there.
