# build-flags-audit.md — the configure flags the pin exposes vs oxpinyin

Status: 2026-08-31 · Scope: the pin `0c5e80e1` (tag 2.11.91) `configure.ac`
flags and their oxpinyin equivalent.

## Summary

The pin exposes **one** build flag that changes the shipped library surface:
`--enable-libzhuyin`. All other `--enable-*` / `--with-*` options tune the
build (DBM backend, tests, utilities) but do not alter the exported ABI of
`libpinyin.so.15`.

## The flags

| Flag | Pin behaviour | oxpinyin equivalent | Status |
|---|---|---|---|
| `--enable-libzhuyin` | builds a **separate** `libzhuyin.so.15` (own SONAME, own `libzhuyin.ver`, own installed `zhuyin.h` / `zhuyin_custom2.h` / `libzhuyin.pc`) from `$(pinyin_SOURCES) zhuyin.cpp` | new crate `oxpinyin-zhuyin-capi` building `libzhuyin.so.15` | **BEHAVIORAL/IMPLEMENTED** |
| `--with-dbm=` | selects the storage backend for the phrase/pinyin indexes | `oxpinyin-store` backend feature (`kyotocabinet` / `redb` / `lmdb` / `tkrzw`) | equivalent |
| `--enable-*` (tests, utils, …) | build-only toggles | n/a (the Rust workspace builds tests inline) | n/a |

## `--enable-libzhuyin` — BEHAVIORAL / IMPLEMENTED

**The flag builds a second shared object, not extra symbols in
`libpinyin.so.15`.** Upstream `src/Makefile.am:108-125`:

- `lib_LTLIBRARIES += libzhuyin.la`
- `libzhuyin_la_SOURCES = $(pinyin_SOURCES) zhuyin.cpp`
- `libzhuyin_la_LDFLAGS = ... --version-script=$(srcdir)/libzhuyin.ver -version-info @LT_VERSION_INFO@`

`@LT_VERSION_INFO@` is the same libtool `current:revision` as libpinyin
(`:97`), whose ABI current is 15 — so the SONAME is **`libzhuyin.so.15`**,
and the export surface is governed by **`src/libzhuyin.ver`**.

**oxpinyin equivalent:** the new workspace member **`oxpinyin-zhuyin-capi`**
produces `libzhuyin.so.15` (metadata `[package.metadata.capi.library] name =
"zhuyin", version = "15.0.0", versioning = true`). It is an **independent
build target**; `oxpinyin-capi` (which builds `libpinyin.so.15`) is
**untouched**. The two facades are the Rust mirror of upstream's one-engine-
two-facades cut (`docs/findings/chewing-crate-seam.md`).

**The exported symbol set is `src/libzhuyin.ver` verbatim: 52 symbols.**
`zhuyin_get_raw_user_input` appears in `zhuyin.h` only inside `#if 0` and is
**not** in the `.ver` — it is not exported. Verified: the built
`libzhuyin_capi.so` exports exactly the 52 `.ver` symbols (diff of the two
sorted lists is empty).

**Install surface:** `libzhuyin.so.15`, `zhuyin.h`, `zhuyin_custom2.h`,
`libzhuyin.pc` (the `libzhuyin.pc.in` template is copied verbatim from the
pin; `build.rs` bakes `@VERSION@` / `@LIBPINYIN_BINARY_VERSION@` /
`@DATABASE_FORMAT@`, and the packaging wrapper fills `@prefix@` /
`@libdir@`).

**Export-control note.** A Rust `cdylib` cannot apply a named version script
at link time: rustc merges its own anonymous script and GNU ld rejects the
pair (the same constraint `oxpinyin-capi`'s `build.rs:26-41` records). So
the boundary is enforced by **source construction** — only the 52
`#[unsafe(no_mangle)]` `zhuyin_*` symbols are exported — and `libzhuyin.ver`
ships verbatim as the record and for the packaging step. The built artifact's
export list is verified equal to the `.ver` list at build time.

**`libpinyin.so.15` ABI unchanged.** `oxpinyin-capi` is not modified in this
change; its built artifact continues to export the same symbol set (the
79-symbol `pinyin_*` parity target measured against the pin oracle).

**Divergences discovered by the zhuyin differential** are recorded in
`docs/findings/upstream-divergences.md` (zhuyin parse restrictiveness,
candidate-type tagging, `FORCE_TONE` / `ZHUYIN_INCOMPLETE` default) — see
that register.
