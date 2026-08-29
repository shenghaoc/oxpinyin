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

### validate_constraint's drop test is the span-search shape — CLOSED (academic: equivalent on model20)

- **Upstream source cite:**
  `src/lookup/phonetic_lookup.cpp:142-168` (`validate_constraint`);
  `src/storage/phonetic_key_matrix.cpp:534-601`
  (`compute_pronunciation_possibility`); `src/storage/phrase_index.h:136-164`
  (`get_pronunciation_possibility`); `src/storage/pinyin_phrase3.h:68-144`
  (the loose compare).
- **Mechanism:** a forcing is dropped when
  `compute_pronunciation_possibility` of the forced token over its span
  falls below `FLT_EPSILON` (2^-23) under the current matrix. The
  quantity is a sum over every matrix path that spells the phrase: each
  complete path contributes the item's matched/total pronunciation
  share under the loose compare — initials exact, initial-only keys
  match any middle/final, zero tone matches any tone, fuzzy not
  handled.
- **What oxpinyin does instead:** drops when no span entry spelling the
  forced token carries a kept pronunciation possibility — `None` (no
  counts, read as possibility 1) or `Some` with a nonzero matched
  count; only `Some((0, _))` rejects (`span_finds_token`;
  the `Some((0, _))` zero-guard landed with the §3 matched/total work)
  — the possibility arithmetic itself is the already-recorded §3
  divergence (first path per token, matched/total as a step-cost term),
  so the below-ε threshold has no bit-faithful port. The cells also
  carry the chosen phrase's display text where upstream re-fetches by
  token from the phrase index, so the selection record rebuilds from
  the store alone.
- **Status:** closed as **academic — equivalent on model20**. The
  arithmetic differs (upstream sums f32 per-path loose-compare terms;
  oxpinyin guards on integer zero before any division), but the drop
  boundary is unreachable on the pinned data, so the two tests produce
  identical observable behavior. Proof: the threshold implies a
  per-token pronunciation total above 2^23 = 8,388,608 for any
  nonzero-but-below-ε share. Scanning the model20 tables: the max
  per-token total is 2,945,481 (的 — 2,224,855 across its pinyin
  records plus 720,626 across its punctuated-variant records; the
  per-library max alone is 2,224,855) and every record frequency is
  ≥ 1, so every nonzero sum is ≥ 1/2,945,481 ≈ 3.4e-7 > FLT_EPSILON ≈
  1.19e-7 — a 2.8× margin. Below-ε-but-nonzero cannot occur; the test
  is equivalently a zero test on both sides. The zero case agrees too:
  every stored pronunciation in model20 carries zero tone (no tone
  digits in any table), so the compare's tone rule never blocks; the
  loose compare's initial-only lenience is mirrored by the span
  search's partial-key expansion (both initial-exact); and fuzzy
  alternates are explicit matrix-column keys on both sides, so the
  all-exact path always matches. No model20 input exists where the pin
  drops a forcing and oxpinyin keeps it — the E2E I/O rule is
  satisfied vacuously, and no non-vacuity case is constructible.
- **Re-opening condition:** two corpus changes reopen this entry, with
  different fixes. A per-token pronunciation total large enough to push
  a forced token's nonzero share below FLT_EPSILON (a matched count of
  1 over a total above 2^23 = 8,388,608) makes the threshold — not
  zero — the drop boundary while a span entry still lists the token:
  this entry becomes a revert target, and the fix is the threshold port
  over the already plumbed matched/total pairs in `span_finds_token`.
  Separate, and not addressed by that port: a corpus storing nonzero
  pronunciation tones breaks matching parity — the pin's loose compare
  turns tone-sensitive where the record lookup behind the span entries
  is tone-blind — so the pin computes a zero sum where oxpinyin's
  entries still carry matched > 0; addressing it needs tone-aware
  matching parity, not the ε comparison.
- **Externally observable:** was only ever reachable on edits that
  leave a span marginally spellable — the same inputs where the §3
  possibility divergence is already observable; on model20, not
  reachable at all (see Status).

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

### The n-best row-choose cursor answers the whole parse end — CLOSED

- **Upstream source cite:** `src/pinyin.cpp:2511-2519`
  (`pinyin_choose_candidate`'s NBEST branch returns
  `matrix.size() - 1` unconditionally).
- **Mechanism:** choosing any n-best row answers the whole input's parse
  length as the new cursor, whatever span the row's own path covered.
- **What oxpinyin did instead:** the row candidate's absolute end —
  the composition offset it actually advanced to. The two agreed whenever
  the row's path reached the parse bound (every real-table surface,
  including the live-typing differential); a degenerate row that stops
  early answered its own shorter end.
- **Status:** closed by answering `parsed_len` for every
  `NBEST_MATCH_CANDIDATE` selection — upstream's own value of
  `matrix.size() - 1`, since `fill_matrix` sizes the matrix to
  `parsed_len + 1` and no split/fuzzy step resizes it, carried here in
  the active parse mode's own coordinates (`m_parsed_len`,
  `pinyin_get_parsed_input_length`) — whatever span the row's path
  covers (`crates/oxpinyin-capi/src/candidates.rs`). Only the answered
  cursor changed; the engine's composition state is untouched. Measured:
  `union-diff.c` grew an NBEST-row section that chooses the fixture's
  degenerate single-phrase row (imported user phrase 测测 for "cece",
  pristine train state — the train section's unigram deltas enrich the
  decode past it, so the section runs first), prints the cursor, and
  follows the corrected post-NBEST flow — `pinyin_guess_sentence`, then
  `pinyin_train`, never `pinyin_guess_candidates` at the cursor (the
  tail slot starts no span on either engine; the old draft's
  guess-at-the-cursor chain only passed because the old row-own-end
  answer happened to equal the chosen phrase's extent there). Old answer
  `nbest-cursor: 4` vs the pin's 9 → `run-union-diff.sh` DIVERGENCE;
  new answer 9 = 9 → IDENTICAL end to end. live-typing differential
  IDENTICAL; the frozen candidate and sentence-surface pins
  bit-identical.
- **Note:** upstream has no `m_last_index` member (grep over the pinned
  2.11.91 tree: none) — the returned cursor is the only cursor channel,
  and the engine session's composition offset keeps its existing
  internal semantics.
- **Externally observable:** was — only through a row whose path ends
  before the parsed input does; the union fixture's imported-phrase
  composition drives exactly one such single-phrase row on the mini
  tables, and none of the frozen pin surfaces constructs one on real
  tables.

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

### The cursor helpers' `_check_offset` aborts answer `false` — not the pin's abort, not post-`95e3af7` upstream's discarded-`false` true

- **Upstream source cite:** `src/pinyin.cpp:2163-2180` (`_check_offset`,
  the assert at `:2175`) called on the COMPUTED result of the word moves
  — `pinyin_get_left_pinyin_offset`'s second check (`pinyin.cpp:3055`)
  and `pinyin_get_right_pinyin_offset`'s (`pinyin.cpp:3090`) — and on
  the normalized cursor offset of `pinyin_get_pinyin_offset`
  (`pinyin.cpp:3023`), at the pin.
- **Mechanism:** `_check_offset` asserts that the column before the
  examined offset is not a lone zero key. The word moves run it twice —
  on the caller offset and on their own computed result — and the second
  call is what fires on tail cursors: for `nihaoshijie` under the parity
  word `0x18a`, `get_right_pinyin_offset(11)` passes the first check
  (column 10 holds the lone non-zero `e`), reads the trailing zero key
  at column 11 (the pin's reserved extra slot), and the second check at
  the zero's raw end 12 sees column 11's lone zero key and aborts.
  Measured first-hand on the rebuilt pin with a fork-per-probe driver:
  the ONLY abort of 48 probes over `nihaoshijie` is `get_right(11)`;
  `get_left(11)` genuinely answers 10 (its walk halts at column 10).
  The same shape fires at every offset one past a separator zero —
  `get_left(3)`/`get_right(3)` on `ni'hao` — and at every offset past
  the parsed end of an early-stopping parse (`ni2hao` offsets 2..6).
- **What oxpinyin does instead:** answers `false` — the engine's
  `EngineError::ZeroKeyOffsetCheck` rendered as the C ABI's `false` by
  the three cursor helpers, extending the no-abort policy already
  applied at the guess seam (`LookupOffsetPastSeparator`) and the
  scheme setters. `get_right` also keeps the pin's one graceful false
  (no key starts at the position, `pinyin.cpp:3085-3086`).
- **The two upstream arms:** at the pin (`0c5e80e`) the call SIGABRTs —
  no oracle differential is possible there. Peng Wu's post-pin
  [`Fix _check_offset function`](https://github.com/libpinyin/libpinyin/commit/95e3af71cca3ce6a974e55ab68db1424da79c286)
  replaces the assert with `if (zero_key != key) return false;` — an
  INVERTED condition whose return value every call site discards, so
  the fixed upstream completes the call and returns `true` with the
  computed value (`right=12` on a matrix whose last usable column is
  11). oxpinyin's `false` diverges from BOTH arms, deliberately: `12`
  is a value no caller can use — upstream is propagating a broken
  result rather than reporting failure, and `false` is the only answer
  a frontend can act on. Report-back candidate for libpinyin (the
  inverted condition also inverts the intended validation).
- **Externally observable:** yes — upstream aborts at the pin /
  returns the broken value post-`95e3af7`; oxpinyin returns `false`.
  Frontends driving Ctrl+Left/Right at a tail cursor see the
  difference; no pinned differential is possible at the abort points.
  The fifth distinct finding in the `_check_offset` family: the three
  sightings consolidated in `oracle-bisect-differential-abort.md`
  (the W11 bisect abort, the ibus-libpinyin#570 guess-seam pattern,
  and the shared root cause), the guess-seam leading-run answered as
  `LookupOffsetPastSeparator`, and this cursor-helper seam.

### Apostrophe-only input: the pin consumes every byte, the engine consumes none

- **Upstream source cite:** `src/pinyin.cpp` parse path over
  `FullPinyinParser2` (`src/storage/pinyin_parser2.cpp`): the pin emits a
  zero `ChewingKey` per `'` separator and counts it in `m_parsed_len` —
  measured on the pin: `'` → parse_return 1, `''` → 2, `'''` → 3
  (the table in `oracle-apostrophe-abort.md`, F-E-14).
- **Mechanism:** the pin's DP walks a separator-only input by emitting
  zero keys, so an all-apostrophe composition has a non-empty matrix
  (lone zero keys at every position) and a consumed length equal to the
  input length.
- **What oxpinyin does instead:** `SegmentGraph` consumes a leading
  apostrophe run only as propagation TOWARD a following key — with no
  key following, no edge is emitted and the consumed length is 0. The
  cursor laws on top then answer `Ok(0)` where the pin's `_check_offset`
  aborts over those zero columns (`EngineError::ZeroKeyOffsetCheck`,
  the entry above); the parse surface itself reports 0 where the pin
  reports the byte count.
- **Externally observable:** yes — the `pinyin_parse_more_full_pinyins`
  return and `pinyin_get_parsed_input_length` differ on apostrophe-only
  input (pin 1/2/3, oxpinyin 0), and the cursor helpers diverge at the
  abort shapes: `pinyin_get_pinyin_offset` answers `true, 0` (the
  clamped zero-fill), while `pinyin_get_left_pinyin_offset` and
  `pinyin_get_right_pinyin_offset` return `false` where the pin
  aborts (`ZeroKeyOffsetCheck`) — the left helper also answers
  `true, 0` at offset 0 only. This is the parser-stop-consumption surface —
  class B2 of `uncovered-surface-differentials.md` ("where does the
  parser stop consuming"), recorded here so B2's closing work INHERITS
  it instead of rediscovering it; the sibling abort on the same input
  (`pinyin_get_pinyin_key`) remains F-E-14 in
  `oracle-apostrophe-abort.md`.

### FORCE_TONE is honoured on the full-pinyin seam only — the double/zhuyin shapes are a different, unported law

- **Upstream source cite:** `src/storage/pinyin_parser2.cpp:412` and
  `:448` (`DoublePinyinParser2::parse_one_key`: `if (options & FORCE_TONE
  && 3 != len) return false;` — NOT nested under `USE_TONE`, and a
  length-3 requirement the full-pinyin parser does not have — plus the
  zero-tone rejection at `:448`); `PinyinDirectParser2::parse_one_key`
  carries the full-pinyin-shaped check (`:645`).
- **Mechanism:** the pin gives each scheme parser its own FORCE_TONE
  semantics; the double-pinyin one is a genuinely different law (a
  two-key-plus-tone length gate).
- **What oxpinyin does instead:** implements the measured surface — the
  full-pinyin law, nested inside `USE_TONE` exactly like the pin
  (`pinyin_parser2.cpp:176-190` ported to `graph.rs::tone_split`) — and
  leaves the double/zhuyin parsers untouched. The measured C1 surface of
  the uncovered-surface differential is full-pinyin only; porting the
  scheme-parser shapes unmeasured is exactly what would perturb the
  frozen double/zhuyin scheme sweeps.
- **Externally observable:** yes — a frontend setting FORCE_TONE on a
  double-pinyin scheme gets the full-pinyin behaviour (no effect without
  `USE_TONE` on that seam) rather than the pin's length-3 gate. Recorded
  as a scope boundary, not an oversight: the port lands with a measured
  double/zhuyin FORCE_TONE differential. The full-pinyin seam itself
  matches the pin (capi e2e `parse_termination` module, harness phase-C
  0x60 probes closed).

## Sanitizer scope on the tkrzw shim CI (2026-08-27)

- **Where:** `.github/workflows/store-backends.yml`, `tkrzw-sanitizers` job.
- **libpinyin behaviour:** its make-check CI (and any `-fsanitize=address,undefined`
  build of a C/C++ tree) instruments every translation unit, Rust has no
  equivalent because there is no Rust in the pin.
- **oxpinyin behaviour:** the ASan arm runs FULL `-Zsanitizer=address`
  instrumentation over the target graph (Rust units plus the GCC-instrumented
  C++ shim), made possible by passing cargo an explicit `--target` so
  RUSTFLAGS never reaches host build scripts or proc macros — the mechanism
  that previously made sanitized proc-macro dylibs unloadable (E0463 on
  `cxxbridge_macro`). The UBSan arm instruments the shim translation units
  and injects libubsan at the final link, because rustc's `-Zsanitizer` list
  has never included an `undefined` value. In both arms the prebuilt standard
  library is uninstrumented (no `-Zbuild-std`).
- **Externally observable:** none for shipped behavior; this widens or narrows
  no differential. The residual gaps are toolchain-bound: no
  `-Zsanitizer=undefined` exists, and std is prebuilt. Revisit when either
  changes; per source policy, recorded and not chased.
