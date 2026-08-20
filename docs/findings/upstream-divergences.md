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

### HANYU full pinyin ignores tone digits under USE_TONE

- **Upstream source cite:** `FullPinyinParser2::parse_one_key`
  (`src/storage/pinyin_parser2.cpp:155-205`): under `USE_TONE` a
  trailing digit 1–5 is the tone and is consumed with the match
  (`zai4` consumes 4; aux renders `zai4`).
- **What oxpinyin does instead:** the HANYU full-pinyin surface
  (`pinyin_parse_more_full_pinyins` → `Session::type_pinyin`) treats
  the digit as junk — `zai4` consumes 3. The frozen full-pinyin corpus
  is tone-less, so every earlier differential ran a profile without
  `USE_TONE` and the gap was unmeasured until the W15 full-scheme
  driver swept scheme 1 with tones.
- **Scope:** HANYU only. LUOMA and SECONDARY_ZHUYIN carry the pinned
  tone-digit behavior (`full_pinyin_index.rs`), and the bopomofo
  keyboards always did.
- **Externally observable:** yes, only with `USE_TONE` set on the
  context — the fork never sets it for full pinyin (ibus-libpinyin
  1.16.5 calls no tone-bearing full-pinyin path), so the borrowed
  frontend cannot reach the divergence.
- **Status:** recorded, not chased — closing it means teaching the
  frozen HANYU parser (or its capi seam) the tone-digit rule, which is
  HANYU-surface work outside the #109 scheme stack.
