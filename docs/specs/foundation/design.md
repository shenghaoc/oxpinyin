# Design — Foundation

Investigations precede and gate later phases, although they produce no shipping software.
See `docs/findings/spec-derivation.md` for details and fixture families.

**Investigation A: provenance.** Inspect archive licenses and record Branch A, A′ or B.
Stage 1's pinned-archive route defers redistribution questions.

**Investigation B: schema.** Capture the pinned frontend schema verbatim.
Document unclear keys from observed behaviour.

**Source-built oracle (`tools/oracle/`).** Verify source and data archive checksums.
Build pinned libpinyin and check the pinned ibus-libpinyin build.
The distribution-independent recipe emits Lane-L's exact shared-object path.

**Capture harness (`tools/capture/`).** Use a small C program against pinned `pinyin.h`.
Emit JSON with pin-reference stamps for F-A–F-F under the method document.
Disable learning and use fresh state.

**Parser.** Use greedy longest-match over a static syllable table with backtracking.
Return alternatives, such as `xian` producing [xian] and [xi,an].
Return `ParseResult { syllables, remainder }` without errors for well-formed UTF-8.
Define Dictionary, UserModel, LanguageModel and InputParser as signatures only.
The earlier unsealed/defaulted-growth rule was retired 2026-10-18 UTC.
See the [interface ruling](../../constitution.md#external-interfaces).

**Data path (P6, `crates/oxpinyin-data`, `crates/oxpinyin-runtime`).**
Open native system directories through `Runtime::open`, as `pinyin_init` does.
Use native DBM names/layouts, mapped and checksummed MemoryChunk files, and `table.conf` for λ.
The caller supplies the directory.
Do not detect layouts.
P6 replaced #228's reader; see `docs/findings/runtime-direct-libpinyin-data-2026-09-02.md`.

**Storage model.** Select bdb, kyotocabinet or tkrzw through the `DefaultStore` `#[cfg]` chain.
tkrzw became default on 2026-10-30 UTC, the maintainer decision that tkrzw is maintained upstream and Berkeley DB and Kyoto Cabinet are not.
Berkeley DB was default from 2026-09-20 to 2026-10-30, matching bare `./configure`.
That was Q1, recorded 2026-09-27 UTC, #593 item 1, in the compatibility policy's "Amendment — rulings recorded".
Tkrzw was default from 2026-09-05 to 2026-09-20; redb was removed 2026-09-20.
Compile exactly one backend per binary.
Use `compile_error!` to reject zero or multiple selections.
No fallback precedence remains.
This mirrors libpinyin's `--with-dbm`.

Build native runtime tables from pinned model20 for every backend with the pin's own data utilities (`gen_binary_files`, `import_interpolation`, `gen_unigram`, now in `oxpinyin-utils`).
Keep parity verification local.
The model20 archive is non-redistributable and never enters CI.
Dictionary loading, language models, decoding and IBus are outside foundation scope.
The [libpinyin wiki](https://github.com/libpinyin/libpinyin/wiki) describes architecture, parser internals and formats as the project catches up.
