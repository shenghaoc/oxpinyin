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

Out of scope: dictionary loading, LM, decoding, IBus.
