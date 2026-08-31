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

**Drop-in compat path (`oxpinyin-data/src/compat/`, `src/memory_chunk.rs`):**
`CompatLayout::detect` recognises a real libpinyin data directory —
`table.conf` (the only file whose absence fails `pinyin_init`; it declares
the DBM, λ and the default phrase libraries) plus a `bigram.db` whose file
magic names the DBM that wrote it — and the runtime routes `pinyin_init`
through `open_compat` on detection. Content tables are backend-independent
`MemoryChunk` images (8-byte header: u32 LE length, u32 XOR checksum
mirrored from `memory_chunk.h::get_check_sum`) holding the
`SubPhraseIndex` structures; `phrase_index.bin`/`pinyin_index.bin` and
`punct.bin` are the build DBM's tree databases (TreeDB/TreeDBM, despite
the extension) and serve detection and punct reading; `bigram.db` is the
build DBM's hash database keyed by the raw `u32` token and valued by a
`SingleGram` chunk. Measured on Fedora rawhide (Kyoto Cabinet), Debian
testing (tkrzw) and NixOS (Kyoto Cabinet): 1,571/1,571 rows, sorted sets
byte-identical, order-only.

**Storage model:** four backends, compile-time selected through the
`DefaultStore` `#[cfg]` chain (kyotocabinet > tkrzw > lmdb > redb; KC
default), one per binary, mirroring libpinyin's own `--with-dbm`. Runtime
tables are compiled natively from the pinned model20 archive for every
backend (`oxpinyin-datagen`); parity verification stays local-only — the
model20 archive is non-redistributable and never enters CI.

**Python binding seam:** `oxpinyin-python` consumes the same
`oxpinyin-runtime` assembly as the C ABI over PyO3 — the rlib route, no
`extern "C"` crossing, no dlopen — with free-threaded CPython (`abi3-py310`
+ `abi3t-py315` declared; CI validates the source build on 3.14t) and the
GIL released around session work.

Out of scope: dictionary loading, LM, decoding, IBus.

**Reference:** [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) — architecture, parser internals, and data formats; the authoritative upstream source while the project catches up.
