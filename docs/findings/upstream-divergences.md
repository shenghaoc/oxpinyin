# Upstream divergences

Purpose: a register of behaviours that oxpinyin cannot or deliberately does
not reproduce because of a Rust language mechanism. Source policy permits
reading and copying upstream; this file is for the residue. Once the rewrite
is complete, these notes are collected to report back to libpinyin.

## Entry template

```markdown
### <short name>

- **Upstream source cite:** `path:lines` in the pinned libpinyin source.
- **Mechanism:** what the C++ does.
- **What oxpinyin does instead:** the Rust behaviour.
- **Externally observable:** yes/no and how a caller would see it.
```

## Register

### Bigram export iterator's pinyin buffer

- **Upstream source cite:** `src/pinyin.cpp:842-872`
  (`pinyin_bigram_iterator_has_next_phrase` builds the `m_pinyins` join
  buffer).
- **Mechanism:** the export iterator keeps C pointers into reused
  pronunciation/join buffers. Repeating an export cycle inside one context
  reuses stale storage and the pinned oracle segfaults.
- **What oxpinyin does instead:** `CapiContext::export_bigram_rows` renders
  the complete row snapshot up front into owned Rust strings before the
  iterator handle is created, so repeated iterator cycles cannot alias stale
  C storage. The per-round train differential runs one export per fresh
  context for the oracle and compares those rows to oxpinyin
  (`tools/bisection/run-train-diff.sh`).
- **Externally observable:** yes — upstream aborts on the repeated-export
  sequence; oxpinyin returns the same rows on every cycle. Also cross-indexed
  in `reference/memory-safety-bugs.md` (use-after-free class).

### Public bigram export is a rendering surface

- **Upstream source cite:** `src/pinyin.cpp:775-918`.
- **Mechanism:** the public bigram iterators render the store: sentence-start
  predecessors are dropped, counts are doubled, below-threshold rows are
  hidden, pronunciations are expanded as a Cartesian product, and
  per-predecessor totals are unreachable.
- **What oxpinyin does instead:** the C ABI reproduces that rendering for
  compatibility; the one-time migration tool does not use the lossy iterator
  and instead links the pinned `libstorage.a` through a dump shim to read the
  raw user-store values (`docs/findings/legacy-migration.md` §3).
- **Externally observable:** yes — the C ABI surface matches; the migration
  tool is an internal tool and keeps the full value surface.

### HANYU full pinyin ignores tone digits under USE_TONE — CLOSED

- **Upstream source cite:** `FullPinyinParser2::parse_one_key`
  (`src/storage/pinyin_parser2.cpp:164-214`): under `USE_TONE` a
  trailing digit 1–5 is the tone and is consumed with the match
  (`zai4` consumes 4; aux renders `zai4` through
  `ChewingKey::get_pinyin_string`, `chewing_key.cpp:47-58`).
- **Mechanism:** the scan reads only the span's last byte; the
  digit-stripped core goes through the ordinary option-gated index
  lookup, so an initial-only key carries a tone like a complete one,
  and the DP window is `max_full_pinyin_length = 7` "include tone"
  (`pinyin_parser2.cpp:82`). `0` and `6`–`9` are not tones: they stay
  in the core, fail the lookup, and the shorter toneless parse wins.
- **Status:** closed by the HANYU `USE_TONE` port. The graph's
  `emit_edges` strips a trailing `1..=5` only under the bit (window 7
  then, 6 otherwise), `Edge` carries the tone, the capi aux renders
  canonical + digit, fuzzy alternates inherit the tone, and the
  resplit/divided tables never match a toned key (`ChewingKey`
  `operator==` includes `m_tone`, `chewing_key.h:81-91`). Measured:
  `SCHEME_DIFF_TONE=1 SCHEME_DIFF_PARSE_AUX_ONLY=1
  run-scheme-diff.sh full 1` → PARSE_AUX_IDENTICAL, with the tone-less
  full-1 sweep staying PARSE_AUX_IDENTICAL over its unchanged corpus.
- **Back-reference:** distinct from #130's aux over-read (a buffer
  split in the aux renderer, not parser consumption) — carrying the
  digit here is exactly what keeps that over-read closed on HANYU.

### Tone digit on an initial-only key aborts the pin's phrase search

- **Upstream source cite:** `contains_incomplete_pinyin`
  (`src/storage/pinyin_phrase3.h:146-156`) asserts
  `CHEWING_ZERO_TONE == key.m_tone` for any zero-middle/zero-final
  key; every `chewing_large_table2` search path dispatches through it.
- **Mechanism:** the tone scan's only precondition is the option-gated
  index hit, so the *parser* produces an initial-only key with a tone
  (`n4` under `PINYIN_INCOMPLETE | USE_TONE`) — and the first phrase
  search containing that key trips the assert. The parser permits
  exactly what the search asserts against.
- **What oxpinyin does instead:** parses the toned initial-only key
  and searches without aborting — the toned incomplete edge flows the
  scan matrix like any other (constitution 4: nothing panics).
- **Externally observable:** yes — the pinned oracle SIGABRTs on `n4`
  under `USE_TONE | PINYIN_INCOMPLETE` as soon as candidates are
  guessed; oxpinyin returns candidates. No oracle differential is
  possible for this class (the pin-built `.so` aborts), the same
  situation as the scheme-setter rows above; the fullpin-diff tone
  sweep documents the exclusion in the driver. Report-back candidate
  for libpinyin.

### Scheme setters abort or half-mutate on the no-op slots

The #109 contract-lock (all rows verified at `0c5e80e1`; oxpinyin
answers `false` and keeps the previous scheme in every case, pinned by
`contract_tests.rs`):

- **double CUSTOMIZED (30)** — upstream aborts mid-call inside
  `DoublePinyinParser2::set_scheme` (`pinyin_parser2.cpp:611-612`)
  after the unconditional fallback clear already ran. The API wrapper
  `pinyin_set_double_pinyin_scheme` (`pinyin.cpp:1154-1159`) never
  returns.
- **double out-of-enum (0, 7–29, 31+)** — the parser clears
  `m_fallback_table` first (`pinyin_parser2.cpp:582`), returns `false`;
  the wrapper ignores the result and answers **`true`**. A live
  fallback-bearing scheme (ZRM/PYJJ/XHE) silently loses its fallback
  while the caller is told the call succeeded: a half-mutation.
  oxpinyin rejects with `false` and the fallback keeps working.
- **zhuyin STANDARD_DVORAK (7)** — the API routes 7 into
  `ZhuyinSimpleParser2::set_scheme`, whose dvorak arm assigns both
  tables and falls through into `default: abort()`
  (`zhuyin_parser2.cpp:291-295`). **Still present at libpinyin tip
  `95e3af7`** (report-back candidate; the keyboard is dormant until
  upstream fixes the fallthrough — then it becomes a table-addition
  port, not a contract slot). The API wrapper also `delete`s the old
  parser before the switch (`pinyin.cpp:1163-1164`), so the context
  would be broken even if the abort were caught.
- **zhuyin out-of-enum** — aborts at the API layer's `default:`
  (`pinyin.cpp:1188`); **full-pinyin out-of-enum** aborts inside
  `FullPinyinParser2::set_scheme` (`pinyin_parser2.cpp:398`) while the
  wrapper (`pinyin.cpp:1148-1153`) answers `true` unconditionally.

**Externally observable:** only through crash or lied-about state —
oxpinyin's `false` + unchanged is the non-aborting contract the
constitution requires; no oracle differential is possible for these
inputs (the pin-built `.so` SIGABRTs).
