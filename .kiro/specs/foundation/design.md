# Design — Foundation

Investigations gate later phases; they come first though they produce no
shipping software. Details and fixture-family definitions live in
`docs/findings/spec-derivation.md`.

**Investigation A — provenance:** archive/licence inspection → finding with
Branch A / A′ / B declaration; note that Stage 1's pinned-archive data route
defers redistribution questions.

**Investigation B — schema capture:** pinned frontend schema extracted
verbatim; unclear keys documented from observed behaviour.

**Source-built oracle (`tools/oracle/`):** checksum-verified source and data
archives → pinned libpinyin shared object plus a pinned ibus-libpinyin build
check. The build recipe is distribution-independent and emits the exact shared
object path consumed by Lane-L.

**Capture harness (`tools/capture/`):** small C program against the pinned
`pinyin.h`, JSON out, pin-ref-stamped; fixture families F-A–F-F per the method
doc; learning off, fresh state.

**Parser:** greedy longest-match over a static syllable table with
backtracking; returns alternatives (`xian` → [xian] and [xi,an]);
`ParseResult { syllables, remainder }`; never an error for well-formed UTF-8.
Traits defined signatures-only: Dictionary, UserModel, LanguageModel,
InputParser — unsealed, defaulted growth.

**Drop-in data path (P6, `crates/oxpinyin-data`, `crates/oxpinyin-runtime`):**
`Runtime::open` opens a libpinyin data directory in place the way
`pinyin_init` does — the DBMs (libpinyin's own file names on Kyoto
Cabinet and tkrzw, `<stem>.<ext>` on redb and LMDB), the `MemoryChunk`
files mmapped and checksummed, `table.conf` for λ — through the same
readers it uses for oxpinyin's own output. There is no compatibility
layer and no layout detection; the caller supplies the directory. The
#228 `CompatLayout` reader this replaced is recorded in
`docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`.

**Storage model:** four backends, compile-time selected through the
`DefaultStore` `#[cfg]` chain (kyotocabinet > tkrzw > lmdb > redb; KC
default), one per binary, mirroring libpinyin's own `--with-dbm`. Runtime
tables are compiled natively from the pinned model20 archive for every
backend (`oxpinyin-datagen`); parity verification stays local-only — the
model20 archive is non-redistributable and never enters CI.

**Python binding seam:** `oxpinyin-python` consumes the same
`oxpinyin-runtime` assembly as the C ABI over PyO3 — the rlib route, no
`extern "C"` crossing, no dlopen — with free-threaded CPython (`abi3-py310`
+ `abi3t-py315` declared; CI validates the source build on 3.14t — a
version-specific `cp314t` extension, not an `abi3t-py315` stable-ABI
wheel) and the GIL released around session work.

Out of scope: dictionary loading, LM, decoding, IBus.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) — architecture, parser internals, and data formats; the authoritative upstream source while the project catches up.
