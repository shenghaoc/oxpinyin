# What `dlopen("libpinyin.so.15")` actually requires

Date: 2026-08-28 · Status: **investigation finding** (measured against
Ubuntu's shipped `libpinyin15`) · Branch:
`claude/pr3-soname-consumer-union`.

The drop-in goal is: rename the built object to `libpinyin.so.15`, put it
on the library path, and unmodified consumers work. Three properties of
the real library decide whether that works, and only one of them is
obvious.

## 1. SONAME — `libpinyin.so.15`

Derived, not guessed: the pin's `configure.ac` carries
`libpinyin_abi_current = 15` and `libpinyin_abi_revision = 0`, giving
libtool `-version-info 15:0`, and libtool's SONAME is
`lib<name>.so.(current - age)` with `age = 0`. Confirmed on the shipped
binary:

```
$ readelf -d /usr/lib/x86_64-linux-gnu/libpinyin.so.15 | grep SONAME
 0x000000000000000e (SONAME)  Library soname: [libpinyin.so.15]
```

A consumer records that string in `DT_NEEDED`, so the replacement has to
answer to it. Set from `build.rs` as a `cdylib` link arg, and by
`[package.metadata.capi.library]` `name = "pinyin"` / `version = "15.0.0"`
for the installed file.

## 2. Symbol versioning — the part that bites

The real library defines its symbols **versioned**:

```
$ nm -D /usr/lib/x86_64-linux-gnu/libpinyin.so.15 | grep pinyin_init
0000000000059c150 T pinyin_init@@LIBPINYIN
```

so a consumer linked against it records a versioned reference:

```
$ readelf -V ./consumer
  000000: Version: 1  File: libpinyin.so.15  Cnt: 1
  0x0010:   Name: LIBPINYIN  Flags: none  Version: 3
```

Three library shapes were built and run against a consumer carrying that
reference. The result is not two-valued:

| Library shape | Outcome |
| --- | --- |
| **No version definitions at all** | loads and runs; glibc warns `no version information available` once per start |
| **Symbols versioned under `LIBPINYIN`** | loads and runs clean — upstream's shape |
| **Version definitions present, symbols unversioned** | **hard failure**: `version 'LIBPINYIN' not found`, exit 1 |

The third row is the trap, because it is what a naive
`-Wl,--version-script` produces from a Rust cdylib. rustc always emits
its *own anonymous* version script for a cdylib
(`-Wl,--version-script=…/list`), and a second, named script cannot be
combined with it:

- **GNU ld** rejects the pair outright — `anonymous version tag cannot be
  combined with other version tags` — so the build fails.
- **rust-lld** accepts the named script's version *definitions* while
  refusing to reassign the symbols (`attempt to reassign symbol
  'pinyin_alloc_instance' of VER_NDX_GLOBAL to version 'LIBPINYIN'`),
  emitting a warning and producing exactly the shape that fails to load.

So adding a version script through the normal cdylib build turns a
working drop-in into a broken one. It is not done here; `build.rs`
carries the reasoning. **The current artifact is row 1**: it loads, and
every consumer prints one glibc warning per start.

Closing that last gap — reaching row 2 — needs the shared object linked
from the staticlib in a packaging step, where rustc's anonymous script is
not in the link line. That is a packaging change, not a code change, and
it is the remaining difference between "works with a warning" and "byte
-identical interface".

## 3. Symbol scope — the consumer union, enforced in source

With a version script unavailable, the enforcement mechanism for the
compatibility policy's exception (d) is `#[cfg]` on the exports outside
the union, behind a `shipped` feature.

That feature is subtractive, which cargo features usually should not be.
The additive spelling was tried first and does not work: **cargo-c does
not forward `--no-default-features` to cargo**, so a default-on `harness`
feature switched off at install time still produced an artifact carrying
the extra symbols (56, not 53). `--features` *is* forwarded, so the
restriction has to ride on it.

Measured on the installed artifact:

```
$ cargo capi install --features shipped …
$ readelf -d libpinyin.so.15.0.0 | grep SONAME     → libpinyin.so.15
$ nm -D --defined-only … | grep -c 'pinyin_'       → 53
  outside the consumer union                       → none
  union symbols not yet implemented                → 5 (see below)
```

## 4. The five that are not there yet

The consumer union is 58 symbols; 53 are implemented. The gap is one
family, and it has one root cause:

```
pinyin_get_pinyin_key
pinyin_get_pinyin_key_rest_length
pinyin_get_pinyin_string
pinyin_get_pinyin_strings
pinyin_get_zhuyin_string
```

plus two symbols that are exported but are **stubs returning `false`** —
`pinyin_get_pinyin_key_rest` and `pinyin_get_pinyin_key_rest_positions`
(`cursor.rs`, "Provisional: always returns false").

Together these are fcitx-libpinyin's entire preedit renderer
(`eim.cpp:419-520`): it walks offsets calling `pinyin_get_pinyin_key`,
uses a `false` return as the **loop terminator**, then renders each key
through the string functions. Against the current build the loop
terminates immediately and the composition renders empty.

The root cause is single: there is no populated `ChewingKey` /
`ChewingKeyRest` for the **default plain-full-pinyin** parse mode. The
other modes are already reachable — `cursor.rs::span_source` builds
`(start, end)` per key from the zhuyin / double / index parses — but
plain full pinyin falls back to the session, and `Session::composition_keys()`
returns `Vec<SyllableKey>` having **discarded the spans**: `fewest_keys`
yields `Edge`s carrying `from` / `to` / `syllable_start` / `key` /
`tone`, and the mapping keeps only `edge.key()`. `Session::build_graph_at`
is private, so the capi cannot rebuild them itself, and doing so through
`oxpinyin-core` directly would re-derive a parse rather than read the
session's.

One additive engine accessor — the existing `composition_keys()` keeping
its spans — unblocks all seven. That is an engine interface change, so
it is a STOP under the decision's terms rather than something to
improvise.
