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

### `pinyin_get_right_pinyin_offset` asserts at a parsed-length cursor

- **Upstream source cite:** `src/pinyin.cpp:2162-2176` (`_check_offset`) and
  `src/pinyin.cpp:3061-3092` (`pinyin_get_right_pinyin_offset`).
- **Mechanism:** `_check_offset` asserts `zero_key != key` when the requested
  offset is a column containing only the zero `'` key.  The bisection
  harness's `cursor == parsed_len` probe for `nihao` reaches that column
  through `pinyin_get_right_pinyin_offset` and aborts.
- **What oxpinyin does instead:** cursor helpers are provisional pure
  arithmetic over the raw input and never assert; the same probe returns
  `min(offset.saturating_add(1), raw_len)`.  The differential harness
  therefore keeps its oracle run on cursor positions the oracle accepts, and
  the capi/valgrind gate exercises the full sequence.
- **Externally observable:** yes — the oracle aborts on that cursor probe;
  oxpinyin returns `true` with a boundary value.
