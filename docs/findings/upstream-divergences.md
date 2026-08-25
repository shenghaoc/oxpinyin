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

### Constraint-aware train without the consistency assert

- **Upstream source cite:** `src/lookup/phonetic_lookup.h:841-935`
  (`train_result3`), `src/pinyin.cpp:2669-2689` (`pinyin_train`).
- **Mechanism:** the train walk asserts the result's token at every
  `CONSTRAINT_ONESTEP` position equals the forced token — a stale result
  walked against a fresh store aborts. With an empty store it trains
  nothing at all, results or not.
- **What oxpinyin does instead:** no assert (the no-abort policy): the
  last lookup's 1-best result is walked as it is. When the result carries
  no forcings — a row-0 choose constrains nothing, exactly upstream — the
  engine falls back to the selection-history walk, because its row chooses
  record tokens where upstream keeps the `MatchResult` on the instance;
  without the fallback the union driver's row-0-intercepted choose would
  train nothing where the oracle's normal-choose flow trains both
  phrases (`run-union-diff.sh`, kept green).
- **Externally observable:** yes — a choose-then-train without an
  intermediate re-guess trains the recorded selection on oxpinyin and
  aborts upstream; the frontend contract (re-guess between choose and
  train) makes the two agree on every driven surface.

### validate_constraint's drop test is the span-search shape

- **Upstream source cite:**
  `src/lookup/phonetic_lookup.cpp:142-168` (`validate_constraint`).
- **Mechanism:** a forcing is dropped when
  `compute_pronunciation_possibility` of the forced token over its span
  falls below `FLT_EPSILON` under the current matrix.
- **What oxpinyin does instead:** drops when the span search over the
  span no longer yields the forced token (`span_finds_token`) — the
  possibility arithmetic itself is the already-recorded §3 divergence
  (first path per token, matched/total as a step-cost term), so the
  below-ε threshold has no bit-faithful port. The cells also carry the
  chosen phrase's display text where upstream re-fetches by token from
  the phrase index, so the selection record rebuilds from the store
  alone.
- **Externally observable:** only on edits that leave a span
  marginally spellable — the same inputs where the §3 possibility
  divergence is already observable.

### Constraints survive every re-parse except the selection-committed one

- **Upstream source cite:** `src/pinyin.cpp:1497-1533`
  (`pinyin_parse_more_full_pinyins` never touches `m_constraints`);
  `src/pinyin.cpp:2697` (`pinyin_reset` clears them).
- **Mechanism:** upstream's constraints are instance state that survives
  every re-parse — extension, backspace, edit, and a re-parse after the
  composition completed — with `validate_constraint` dropping whatever
  no longer spells at the next guess. There is no engine-visible
  "completed" notion: the cursor is the frontend's own state.
- **What oxpinyin does instead:** the parse continues an OPEN
  composition's re-parse when the buffer evolved from the stored one —
  extension, shrink, or re-send keep the store, the selection record,
  and the clamped cursor; validate drops what stops spelling, and the
  record follows. Two shapes start fresh: a composition a SELECTION
  consumed (the frontend's reset-between-compositions contract, which
  the #141 cursor flows' pinned tests require and which upstream's
  frontends perform themselves via `pinyin_reset` on commit), and a
  divergent buffer — a different string is a different composition,
  and a stale selection-derived cursor must not mis-anchor the new
  composition's window before validate could drop the mismatched
  forcings.
- **Externally observable:** yes — on a re-parse after a
  selection-consumed composition without an intervening reset, or on a
  divergent re-parse of an open composition; neither is a shape
  upstream's frontends drive (they reset on commit, and mid-composition
  they re-send the same buffer). The backspace ladder itself is
  measured identical (`live-typing.md` §"Backspace-after-choose").

### The n-best row-choose cursor is the row's own end

- **Upstream source cite:** `src/pinyin.cpp:2511-2519`
  (`pinyin_choose_candidate`'s NBEST branch returns
  `matrix.size() - 1` unconditionally).
- **Mechanism:** choosing any n-best row answers the whole input's parse
  length as the new cursor, whatever span the row's own path covered.
- **What oxpinyin does instead:** the row candidate's absolute end —
  the composition offset it actually advances to. The two agree whenever
  the row's path reaches the parse bound (every real-table surface,
  including the live-typing differential); a degenerate row that stops
  early (the mini fixture's single-phrase row) answers its own shorter
  end.
- **Externally observable:** yes, but only through a row whose path ends
  before the parsed input does — none of the pinned surfaces constructs
  one on real tables.

### pinyin_get_sentence asserts a non-empty past-the-rows index

- **Upstream source cite:** `src/pinyin.cpp:1463-1482`
  (`pinyin_get_sentence`).
- **Mechanism:** the API is inconsistent with itself on the same caller
  error — an empty result set answers `false` (defined), but a
  non-empty one asserts `index < results.size()`: asking one row past
  the set SIGABRTs. Two behaviors for one misuse, one of them a crash.
- **What oxpinyin does instead:** `false` past the row count, including
  on a non-empty set (the W14 decoded-or-false gate) — the empty-set
  branch's own behavior, applied uniformly.
- **Externally observable:** yes — upstream SIGABRTs on the call;
  oxpinyin returns false. Found while teaching the live-typing driver
  the caller contract: the frontend renders exactly the NBEST rows the
  candidate list carries, so a proved-index-bound question never trips
  it on either engine. Report-back batch: file with the aux over-read
  as an internal-inconsistency pair, not a bare assert.

### N-best trellis accumulates gfloat log costs — not reproducible in fixed point

- **Upstream source cite:** `src/lookup/phonetic_lookup.h:663, 692`
  (`m_poss += log(...)` per step, a `gfloat` accumulator rounded at
  every node); comparator `trellis_value_less_than` (`:66-91`), used by
  the node store, the beam-32, and the tail-3 selections; final tail
  sort `trellis_value_compare` (`:174-178`), which truncates the float
  poss difference to `gint` so two tails within 1.0 nat tie and keep
  heap-pop order.
- **Mechanism:** the sentence n-best selection is a pure function of the
  accumulated `gfloat` log-probabilities. Each step adds a natural `log`
  (computed in `double`, stored back to `f32`), and near-ties among the
  top-3 survivors — which 1st/2nd/3rd hypotheses live, and their rank
  order — are decided by those exact float values, down to the ULP.
- **What oxpinyin does instead:** the core integer fixed-point surprisal
  scale (negative log₂ × 1000; `crates/oxpinyin-core/src/cost.rs`),
  accumulated exactly, with an insertion-order tiebreak. Reproducing the
  `gfloat` values would require a floating-point natural `log` per step;
  `f64::ln` delegates to the platform libm with no cross-platform
  bit-exactness guarantee, the build forbids `-march=native` for exactly
  this reason, and constitution item 6 requires output to be a pure
  function of (input, user state, config) on every OS. The float
  dependency also reaches the tiebreak (heap-pop order is seeded by the
  exact-float comparator), so it cannot be recovered in fixed point
  either. Contrast the candidate frequency `amplified_frequency`
  (`(1−λ)·unigram/total·2²⁴` — IEEE-754 basic ops only, no
  transcendental), which *is* bit-reproducible and ported to 100%.
- **Externally observable:** yes, on the sentence surface only. Against
  the pinned oracle over a 496-input W2 sample
  (`fixtures/w4/oracle-sentence-surface.txt`): 1-best 488/496, n-best
  distinct-set 385/496, n-best ordered / first-6 rows 379/496; the 117
  ordered misses are all trellis-side (0 candidate-surface leaks). The
  candidate surface, which does not share this arithmetic, is
  bit-identical. Recorded as the measured Stage-1 sentence residual in
  `sentence-surface.md` §12 (recommended as a permanent divergence; the
  freeze is the maintainer's call); enumerate with the read-only
  `pinyin-oracle` `sentence-tail` binary.

### Predicted-candidate tie order is the Tkrzw HashDBM bucket walk

- **Upstream source cite:** `src/storage/phrase_large_table3_tkrzwdb.cpp:155-190`
  (`PhraseLargeTable3::search_suggestion`: `MakeIterator`/`Jump(prefix)`/
  `Next` over `phrase_index.bin`, a `TkrzwHDB` file); consumed verbatim by
  `_compute_predicted_prefix_candidates` (`src/pinyin.cpp:2380-2405`) and
  left in place by `g_array_sort_with_data` — measured on a 178-element
  array with grouped ties, glib's sort preserves within-tie insertion order
  (0 inversions).
- **Mechanism:** the system suggestion phrases are baked with uniform
  phrase-index counts (measured on model20: 好 177×100+1×200, 的
  281×100+2×99, 一 587×100+2×99+2×200, 我 167×100+1×200), so the
  `(length desc, amplified-freq desc)` comparator ties across the whole
  list and the row order a caller sees is exactly the store's iteration
  order — the Tkrzw hash bucket walk, one physical file holding all
  libraries' tokens (27 library switches observed in one prefix's list).
  Deterministic for a given file and tkrzw version; not expressible as a
  sort key over (text, token, library).
- **What oxpinyin does instead:** the prediction pipeline has three
  stages. (1) Collection — `SystemDictionary::suggest_after`
  (`crates/oxpinyin-data/src/dict.rs:196-217`) walks a
  `BTreeMap<String, Vec<u32>>` in text order, collecting every phrase that
  starts with the prefix, then sorts that collection by token ascending.
  (2) Ranking — `guess_predicted` (`crates/oxpinyin-capi/src/predict.rs`)
  applies a stable sort whose primary comparator is **phrase length,
  descending**, tie-broken by **amplified frequency, descending**, so the
  final order is determined by that comparator (and the stable collection
  order within a full tie), NOT directly by the `BTreeMap` walk. (3)
  Deduplication drops repeats by phrase text. The hash bucket order of a
  foreign DBM layout is not derivable from any key, so matching the pin's
  order exactly would mean replicating the Tkrzw hash layout or freezing
  per-prefix orders as fixture data.
- **Externally observable:** yes — the row order of `PREDICTED_PREFIX`
  candidates from `pinyin_guess_predicted_candidates[_with_punctuations]`.
  The sets are identical after the prefix slice (closed by the B1 fix —
  the slice lands in `predict.rs`), and every response is divergent on
  position only. Position mismatches vs the pin, matched model20 tables:
  **177/178 on 好, 1557/1571 across the eight measurement prefixes**
  (the text-ascending order; the pre-switch token-ascending order was
  174/178 and 1541/1571). The gate is `tools/bisection/pred-order-diff.c`
  on the measurement branch.

**Decision (maintainer, 2026-08-25): a defined order, not fixture-frozen
parity.** The pin's order is a compile-time artifact of its DBM choice
with no semantic content, a frozen fixture would re-freeze whenever the
pin's storage changes, and it would hand frontends an order that
carries no meaning. The defined order is **text-ascending** — stable
across builds, what the `BTreeMap` walk already yields, reproducible by
anyone — joining the trellis-float entry as "upstream deterministic but
not reproducibly so." Two consequences, stated explicitly:

1. oxpinyin **permanently diverges from the pin on list positions**
   for predicted candidates. The parity number (177/178 on 好,
   1557/1571 across the eight measurement prefixes — measured after
   the text-ascending switch) is a recorded constant, not a target
   of zero.
2. The pred-order gate therefore **changes meaning**: from a parity
   assertion (drive to zero) to a **defined-order assertion** — the
   emitted list equals its own defined text-ascending order
   (within the comparator's `(char count, amplified frequency)`
   tie groups). Implemented in-tree: the capi e2e test
   `predicted_tie_groups_are_text_ascending_including_user_rows`;
   the runner comparison against the pin stays as the
   recorded-divergence constant.

**Completed (fix/predicted-text-order):** the token pre-sorts are gone —
three sites, not two: `SystemDictionary::suggest_after`
(`dict.rs:215`), `append_predicted_prefix` (`predict.rs`), and the user
seam `UserLookup::suggest_after` (`oxpinyin-user/src/lookup.rs`). The
`BTreeMap` text-ascending walk (token-ascending within one text) now
survives the stable sort's tie groups, on the system and user seams
alike — measured new drift constants 177/178 (好) and 1557/1571
eight prefixes. The defined-order predicate is asserted by the capi
e2e test `predicted_tie_groups_are_text_ascending_including_user_rows`
(grouping by the comparator's exact `(char count, amplified frequency)`
key, populated user store included); the oracle comparison remains the
recorded drift constant, *not* a target of zero.

**Homograph nuance (frontend-invisible):** within one text the
per-text token vector stays token-ascending (`build_text_tokens`), so
a homograph row keeps the same surviving token under the defined order;
the one case that can differ is a system-vs-user text duplicate — the
system row now always precedes the user row, so with a populated user
store the **token recorded on the surviving dedup row** can differ.
Text, candidate type and counts cannot.

### Mid-syllable candidate-lookup offset: empty matrix column vs suffix re-parse

- **Upstream source cite:** `src/pinyin.cpp:2224-2262`
  (`pinyin_guess_candidates` anchors `start = offset` and runs
  `search_matrix(matrix, start, end, ...)` over the whole-composition
  `PhoneticKeyMatrix`); `src/pinyin.cpp:2163-2180` (`_check_offset` asserts
  only on a lone zero-key column — one past an apostrophe run — never on an
  ordinary mid-syllable offset); `src/pinyin.cpp:3006-3027`
  (`pinyin_get_pinyin_offset` walks the cursor back to the nearest non-empty
  column before any caller reaches the guess).
- **Mechanism:** the pin's matrix has one column per input byte, but a column
  carries a key only where a syllable *begins*; mid-syllable byte positions are
  empty columns. `search_matrix` from an empty `start` column matches nothing,
  so `pinyin_guess_candidates` at a mid-syllable offset returns only the
  prepended n-best sentence rows and no phrase rows — measured on `nihao`
  (parity word `0x18a`, after `pinyin_guess_sentence`): offset 3 → `n=1` (`你好`
  alone), offsets 1 and 4 likewise `n=1`. The pin never *aborts* there; only
  one past a lone apostrophe column trips the `_check_offset` assert. In normal
  use the pin is never handed a mid-syllable offset — the frontend routes every
  cursor through `pinyin_get_pinyin_offset`, which snaps it to the syllable
  start.
- **What oxpinyin does instead:** the candidate window is rebuilt from the raw
  byte suffix `&raw[offset..]` (`Session::candidates_at` → `Session::scan_window`,
  the same construction `refresh` runs at the composition offset), so a
  mid-syllable offset re-parses the tail as a fresh composition and returns that
  tail's phrases — measured on `nihao`: offset 3 → `n=106` (`奥/澳/凹/傲/…`, the
  `ao` re-parse), offset 4 → `n=6` (`哦/噢/…`, the `o` re-parse). At every
  syllable-aligned offset — the only kind a correct caller produces — the two
  agree bit-for-bit (`nihao` at offsets 0/2/5 identical, `n=126`/`94`/`1`).
  Where the pin's `_check_offset` aborts (one past an apostrophe run), oxpinyin
  returns `false` from `pinyin_guess_candidates` via
  `CapiInstance::validate_lookup_offset`
  (`Session::normalized_lookup_offset` → `EngineError::LookupOffsetPastSeparator`)
  — the no-abort policy already recorded in `oracle-bisect-differential-abort.md`.
- **Externally observable:** not in practice — no known frontend passes a
  mid-syllable offset to `pinyin_guess_candidates`; fcitx5-oxpinyin and
  ibus-libpinyin both snap the cursor with `pinyin_get_pinyin_offset` first, and
  at a snapped (syllable-aligned) offset the windows are identical. A caller
  that bypasses the snap and hands a raw mid-syllable offset sees oxpinyin's
  suffix-re-parse window where the pin shows only the sentence row. The
  divergence is a language-mechanism residue: oxpinyin builds the window from a
  re-parsed byte suffix rather than indexing a persisted whole-composition
  matrix, so it does not model that matrix's empty mid-syllable columns. Per
  source policy, recorded and not chased.
