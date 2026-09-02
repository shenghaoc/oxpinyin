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

### Constraints survive every re-parse — CLOSED

- **Upstream source cite:** `src/pinyin.cpp:1497-1517`
  (`pinyin_parse_more_full_pinyins` never touches `m_constraints`);
  `src/pinyin.cpp:2693-2704` (`pinyin_reset` clears them,
  `m_constraints->clear()` at :2699).
- **Mechanism:** upstream's constraints are instance state that survives
  every re-parse — extension, backspace, edit, and a re-parse after the
  composition completed — with `validate_constraint` dropping whatever
  no longer spells at the next guess. There is no engine-visible
  "completed" notion: the cursor is the frontend's own state.
- **What oxpinyin did (pre-revert — historical, superseded by the
  Closed bullet below):** the parse continued an OPEN composition's
  re-parse when the buffer evolved from the stored one — extension,
  shrink, or re-send kept the store, the selection record, and the
  clamped cursor; validate dropped what stops spelling, and the record
  followed. Two shapes started fresh: a composition a SELECTION
  consumed (an engine-level emulation of the frontend's
  reset-on-commit contract the #141 cursor flows pinned), and a
  divergent buffer. Only the divergent-buffer half survives today.
- **Audit note (2026-08-29):** an early work order framed this entry as
  a `pinyin_reset` scope/order question. It never was: the pin's
  `pinyin_reset` and oxpinyin's `pinyin_reset` (`reset_parse_state` +
  `Session::reset`) produce identical post-state field for field — both
  clear the constraint store, and upstream's parse path leaves it
  untouched. The divergence lived only in `parse_continues`'s
  selection-committed rule.
- **Closed** (`fix/revert-r5-constraint-reset`): a selection-committed
  composition whose buffer evolved from the stored one now CONTINUES —
  `Session::committed_parse_continues` joins `parse_continues`, and
  `begin_parse` takes only the composition reset (`reset_composition`),
  keeping the store and the selection record into the next guess where
  validate drops what stops spelling. `pinyin_reset` alone clears the
  store now, exactly upstream. A DIVERGENT buffer still starts fresh:
  a different string is a different composition, and a stale
  selection-derived cursor must not mis-anchor the new composition's
  window before validate could drop the mismatched forcings — a
  deliberate boundary upstream has no analogue for (its cursor is the
  frontend's own), not a recorded divergence.
- **Evidence:** the live-typing differential gained a committed-reparse
  phase (choose the whole input's phrase — the commit branch — then
  re-parse an extension with no reset): `pinyin_clear_constraint(0)`
  answers 1 on both sides post-revert, 0 on the engine pre-revert
  (DIVERGENT), so the probe flips to IDENTICAL with the change
  (`live-typing.md`). The frozen pins held bit-identical (candidates
  10,190/10,190/absent 0/order-only 0/prefix-10 98,930; sentence
  488/385/379), the scheme sweep is byte-identical to the pre-change
  baseline (its standing §5 tie class), and the backspace ladder is
  unchanged (`live-typing.md` §"Backspace-after-choose"). The three
  #141 cursor flows that pinned the fresh start were re-based onto the
  continued-store contract (the committed-reparse store test, the
  post-separator choose law, the Luoma offset law); the
  training-through-the-ABI flow now takes the explicit `pinyin_reset`
  a frontend performs.

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

### Mid-syllable candidate-lookup offset: closed — the pin's empty-column law, not the suffix re-parse

- **Upstream source cite:** `src/pinyin.cpp:2224-2262`
  (`pinyin_guess_candidates` anchors `start = offset` and runs
  `search_matrix(matrix, start, end, ...)` over the whole-composition
  `PhoneticKeyMatrix`); `src/pinyin.cpp:2163-2180` (`_check_offset` asserts
  only on a lone zero-key column — one past an apostrophe run — never on an
  ordinary mid-syllable offset); `src/pinyin.cpp:3006-3027`
  (`pinyin_get_pinyin_offset` walks the cursor back to the nearest non-empty
  column before any caller reaches the guess); `src/storage/phonetic_key_matrix.cpp`
  (`fill_matrix` puts each chosen key at its raw begin, and `resplit_step` /
  `inner_split_step` append split keys at interior positions, so a divided
  syllable's boundary is a live column too); `src/storage/special_table.h`
  (the frozen divided/resplit pair lists).
- **Mechanism:** the pin's matrix has one column per input byte. A column
  carries a key where the chosen parse's syllable begins, plus the split-key
  halves the two table steps add (`jie` in `nihaoshijie` also carries
  `ji` + `e`, so byte 10 answers the `e` window — measured fresh `n=190`,
  阿 first — while mid-chunk bytes 3/4/6 of the same input stay empty), and a
  zero key at every apostrophe, which `search_matrix` steps over (measured:
  `ni'hao@2` answers the full `hao` window, `n=93`). `search_matrix` from an
  empty column matches nothing, so the window there is only the prepended
  n-best sentence rows over the raw-suffix fallback — measured on `nihao`
  (parity word `0x18a`): fresh offsets 1/3/4/5 → `true` with `n=0`; after
  `pinyin_guess_sentence` → `true` with `n=1` (你好 alone); `nihaoshijie`
  after the sentence lookup → `true` with `n=3` (你好世界/你好时节/你好是届).
  The pin never *aborts* at a mid-syllable offset; only one past a lone
  apostrophe column trips the `_check_offset` assert (`ni'hao@3` — the
  recorded no-pin-behaviour landmine, not a comparable surface). In normal
  use the pin is never handed a mid-syllable offset — the frontend routes
  every cursor through `pinyin_get_pinyin_offset`, which snaps it to the
  syllable start.
- **What oxpinyin did instead (pre-fix):** the candidate window was rebuilt
  from the raw byte suffix `&raw[offset..]` (`Session::candidates_at` →
  `Session::scan_window`), so a mid-syllable offset re-parsed the tail as a
  fresh composition and returned that tail's phrases — measured on `nihao`:
  offset 3 → `n=105` phrase rows (`奥/澳/凹/…`, the `ao` re-parse), offset 4 →
  `n=5` (the `o` re-parse) — where the pin shows the empty-column window.
  Offsets whose suffix cannot begin a parse (`i`-initial tails: `nihao@1`,
  `nihaoshijie@1/7/9`) already agreed, because the re-parse found nothing and
  the C ABI skips the raw-fallback row.
- **The fix:** `Session::candidates_at` now classifies the anchor before
  scanning. An anchor a scan-matrix key's syllable starts on — or an
  apostrophe byte the parse reached — keeps the offset-anchored scan; any
  other byte (a mid-syllable position, or an apostrophe past a stop byte,
  outside the matrix) is the pin's empty column, answered as the raw-suffix
  fallback under the prepended
  n-best rows with no phrase scan. The classification reuses
  `build_scan_matrix` — the same matrix model the window scan itself reads
  (`docs/findings/matrix-split-tables.md`) — so the boundary law and the
  candidate construction cannot disagree about what the matrix holds.
  Measured byte-identical against the pin over every byte offset of `nihao`
  and `nihaoshijie`, fresh and post-sentence, on the compared surface — the
  guess bool, the window count, and each window's first four rows (the
  driver's
  phase E: `tools/bisection/uncovered-surface-diff.c`, labels `raw:`) — and
  over the exotic classes: `ni'hao@2` (transparent apostrophe, `n=93`/`94`
  both sides), `nihaozh@6` and `nihaozhu@6/7` (incomplete `zh`/`zhu` stay one
  matrix key — empty columns both sides), `shon` under
  `PINYIN_CORRECT_ON_ONG` (the parse is `s|hong`; bytes 2/3 empty both
  sides), and `nang`/`shuo` under `PINYIN_AMB_AN_ANG` (single-key parses;
  every interior byte empty both sides). The prior suffix-re-parse windows
  (`nihao@3` → `n=105`) are gone. At every syllable-aligned offset — the only
  kind a correct caller produces — the two engines keep agreeing bit-for-bit.
  Where the pin's `_check_offset` aborts (one past an apostrophe run), oxpinyin
  still normalizes or refuses via
  `CapiInstance::validate_lookup_offset`
  (`Session::normalized_lookup_offset` → `EngineError::LookupOffsetPastSeparator`)
  — the no-abort policy already recorded in `oracle-bisect-differential-abort.md`.
- **Externally observable:** not in practice — no known frontend passes a
  mid-syllable offset to `pinyin_guess_candidates`; fcitx5-oxpinyin and
  ibus-libpinyin both snap the cursor with `pinyin_get_pinyin_offset` first,
  and at a snapped (syllable-aligned) offset the windows are identical. A
  caller that bypasses the snap now sees the pin's empty-column window on
  both engines. This closes the residue the 2026-08-27 amendment of
  `uncovered-surface-differentials.md` recorded as not chased.

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

### The single-key surface aborts the pin where oxpinyin answers `false`

- **Upstream source cite:** `FullPinyinParser2::parse_one_key`
  (`src/storage/pinyin_parser2.cpp:168-170`, the
  `assert(NULL == strchr(input, '\''))` on apostrophes);
  `pinyin_unload_addon_phrase_library` (`src/pinyin.cpp:497-499`, the
  `assert(index < PHRASE_INDEX_LIBRARY_COUNT)`); the empty-input reads
  under `USE_TONE` (`input[parsed_len - 1]` at
  `pinyin_parser2.cpp:180` on a zero-length string;
  `ZhuyinSimpleParser2::parse_one_key`'s `str[len - 1]` at
  `zhuyin_parser2.cpp:171`).
- **Mechanism:** the Tier-A single-key ABI surface (`pinyin_parse_full_pinyin`,
  `pinyin_parse_double_pinyin`, `pinyin_parse_chewing`,
  `pinyin_unload_addon_phrase_library`) takes arbitrary caller input with
  no guards; several shapes run the caller straight into an `assert` (or
  an out-of-bounds read) and the pinned oracle dies — measured first-hand
  while building `tools/bisection/key-surface-diff.c`: the apostrophe
  probe SIGABRTs at `pinyin_parser2.cpp:170`, the `index = 16` unload
  probe SIGABRTs at `pinyin.cpp:499`.
- **What oxpinyin does instead:** the no-abort policy — apostrophes
  refuse (`false`, zero key for the full-pinyin entry, which zeroes
  `*onekey` before its probe exactly like the pin), an out-of-range
  addon index answers `false`, empty input refuses. All pinned by the
  Rust ABI suite (`tests/abi/keys.rs`); the differential excludes these
  shapes with the exclusion documented in the driver.
- **Externally observable:** yes — upstream SIGABRTs on the same calls
  oxpinyin answers. Report-back batch: file with the scheme-setter and
  `_check_offset` assert families.

### FORCE_TONE — scheme-specific: full-pinyin batch and all one-key seams honour scheme law; zhuyin batch closed (1671954); double-pinyin batch seam remains

- **Upstream source cite:** `src/storage/pinyin_parser2.cpp:412` and
  `:448` (`DoublePinyinParser2::parse_one_key`: `if (options & FORCE_TONE
  && 3 != len) return false;` — NOT nested under `USE_TONE`, and a
  length-3 requirement the full-pinyin parser does not have — plus an
  inner check at `:448` that is unreachable, the digit parse above it
  already refuses every non-tone byte); `PinyinDirectParser2::parse_one_key`
  carries the full-pinyin-shaped check (`:645`).
- **Mechanism:** the pin gives each scheme parser its own FORCE_TONE
  semantics; the double-pinyin one is a genuinely different law (a
  two-key-plus-tone length gate).
- **What oxpinyin does instead:** implements the measured surface — the
  full-pinyin law, nested inside `USE_TONE` exactly like the pin
  (`pinyin_parser2.cpp:176-190` ported to `graph.rs::tone_split`) — and
  originally left the BATCH double/zhuyin parsers untouched. The measured C1 surface of
  the uncovered-surface differential is full-pinyin only; porting the
  scheme-parser shapes unmeasured is exactly what would perturb the
  frozen double/zhuyin scheme sweeps.
- **Tier-A amendment (2026-08-29, the one-key seams).** The ABI
  single-key entries DO carry their scheme laws now:
  `pinyin_parse_double_pinyin` implements `DoublePinyinParser2::
  parse_one_key`'s law — the length-3 `FORCE_TONE` gate at
  `pinyin_parser2.cpp:412` (whose inner zero-tone check at `:448` is
  dead: the digit parse above it already refuses every non-tone byte) —
  and `pinyin_parse_chewing` implements the Simple/Discrete/CP26
  `FORCE_TONE` placements (`zhuyin_parser2.cpp:178` nested under
  `USE_TONE`; `:373`+`:387` unconditional for Discrete; `:602` nested).
  Measured: `tools/bisection/run-key-surface-diff.sh` is IDENTICAL
  against the pin (2,131 probe lines) across double schemes 1–6 and
  chewing keyboards 1–6, 8, 9. The FORCE_TONE law parity itself is
  the `0x1ea` (`USE_TONE|FORCE_TONE`) and `0x1ca` (FORCE_TONE alone)
  profiles; `0x1aa` (`USE_TONE` alone) and the `0x18a` baseline run for
  regression coverage of the neighbouring seams, not FORCE_TONE. The
  batch double-pinyin `parse` surface keeps the original scope boundary
  above (its builder is the frozen scheme sweep); the batch zhuyin surface
  was subsequently closed — see Zhuyin batch amendment below.
- **Zhuyin batch amendment (1671954, 2026-08-31).** The batch zhuyin
  `parse` surface (`zhuyin_parse_more_chewings` via `oxpinyin-zhuyin-capi`)
  now honours `FORCE_TONE` per keyboard family: nested under `USE_TONE`
  for Simple/CP26, unconditional for Discrete — matching
  `zhuyin_parser2.cpp:176-180, :373, :387, :602`. Measured: the
  `tools/bisection/zhuyin-diff.c` differential converges on the batch parse.
  This closes the zhuyin batch seam. The double-pinyin batch seam
  (`pinyin_parse_more_double_pinyins`) remains open: the
  `pinyin_parser2.cpp:412` length-3 gate is not yet implemented on that path
  and belongs with the eventual double-pinyin SPEC freeze.
- **Externally observable:** on the one-key seams and the zhuyin batch
  seam, no longer — all answer identically to the pin under every
  FORCE_TONE profile (one-key seams: D3 gate; zhuyin batch: 1671954).
  On the double-pinyin batch seam, yes — `pinyin_parse_more_double_pinyins`
  with FORCE_TONE set produces the full-pinyin behaviour (effective only
  inside `USE_TONE`) rather than the pin's length-3 gate
  (`pinyin_parser2.cpp:412`). The full-pinyin seam itself matches the pin
  (capi e2e `parse_termination` module, harness phase-C 0x60 probes closed).

### Empty-string phrase lookup SIGFPEs the pin

- **Upstream source cite:** `pinyin_phrase_segment` →
  `PhraseLookup::get_best_match` with `sentence_length = 0`
  (`src/lookup/phrase_lookup.cpp:121-157`), reaching
  `m_phrase_table->search(0, ...)`; measured SIGFPE (gdb: divide in the
  search path) on the pin-built oracle.
- **Mechanism:** a zero-length sentence reaches the span search, which
  divides by the (zero) span length; upstream never guards the entry
  point's UTF-8-validated but possibly-empty input.
- **What oxpinyin does instead:** the span DP over zero characters has
  one step (the virtual start), the last step is empty, `final_step`
  answers `false` with a zero-length result — the same shape every
  failed match takes.
- **Externally observable:** yes — the pin SIGFPEs on
  `pinyin_phrase_segment(instance, "")`; oxpinyin answers `false`.
  Same theirs-bug family as the apostrophe abort (F-E-14) and the
  SECONDARY_ZHUYIN over-read; report-back candidate for libpinyin.
  Found while building the Tier-C dict-surface differential
  (`tools/bisection/dict-surface-diff.c`, which excludes the shape).

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

## Native data-file naming under the compile-time backend (2026-08-29)

- **Where:** `oxpinyin-store` (`DefaultStore`/`DEFAULT_STORE_EXT`),
  `oxpinyin-runtime`'s system/user file names, `oxpinyin-datagen`
  `Backend::extension`.
- **libpinyin behaviour:** the DBM backend is chosen at configure time
  (`--with-dbm`, `if BERKELEYDB/KYOTOCABINET/TKRZW` in
  `src/storage/Makefile.am`), and the data filenames are backend-INDEPENDENT
  compile-time constants (`SYSTEM_BIGRAM "bigram.db"`,
  `SYSTEM_PINYIN_INDEX "pinyin_index.bin"`, … `src/pinyin_internal.h`), so a
  Kyoto-Cabinet-built libpinyin still writes `bigram.db`.
- **oxpinyin behaviour:** the same one-backend-per-binary compile-time
  selection (the `DefaultStore` cfg chain, precedence
  kyotocabinet > tkrzw > lmdb > redb), but native tables carry the backend's
  own extension (`pinyin_index.kct`/`.tkt`/`.lmdb`/`.redb`), so a directory
  self-describes which backend wrote it and mixed deployments cannot
  misread a file through the wrong engine.
- **Externally observable:** only in oxpinyin's NATIVE data directories.
  The libpinyin drop-in/compat path reads libpinyin's own fixed names
  (`bigram.db`, `*.bin`) unchanged, so no libpinyin consumer sees the
  difference; recorded because the naming intentionally diverges from the
  pin's constants rather than mirroring them.

## R1 measured on the drop-in compat paths — order-only, sets identical (2026-08-30)

> **SUPERSEDED (see architecture correction).** The libpinyin drop-in /
> compat loader described in this section has been removed. oxpinyin
> reads only its own peer-backend tables (KC, redb, LMDB, tkrzw); it does
> not detect or read libpinyin's on-disk DBM files. The measurements
> below are preserved as a historical record of what the (removed)
> compat path did.

- **Where:** the removed `oxpinyin-data/src/compat` module (libpinyin
  drop-in loader) and its removed
  `tools/bisection/run-pred-order-dropin.sh` /
  `run-dropin-fedora-kc.sh` / `run-dropin-debian-tkrzw.sh` container
  harnesses.
- **The measurement:** dual-dlopen differential — the distro's own
  libpinyin (the oracle) and oxpinyin's `libpinyin_capi.so` (the subject)
  each run the eight predicted-prefix probes over the SAME installed
  `libpinyin-data` directory; PREDICTED_PREFIX rows are compared in order
  (absolute indices stripped: the subject's `_with_punctuations` API
  prepends punctuation rows of a different type; the driver falls back to
  plain `pinyin_guess_predicted_candidates` on libpinyin < 2.11, whose
  enum is a prefix of 2.11.91's — `PREDICTED_PREFIX_CANDIDATE` sits at the
  same ordinal). The driver also dumps the PREDICTED_PUNCTUATION rows
  (type 8, from the install's `punct.bin`, present from 2.11 on) as
  `punct-*` lines, which makes the differential the compat punct reader's
  gate: those rows appear on the subject only when it reads that file.
- **Kyoto Cabinet path** (Fedora rawhide container: libpinyin 2.11.91,
  kyotocabinet 1.2.80): oracle 1,571 rows, subject 1,571 rows; sorted row
  SETS byte-identical; 2,562 diff lines of pure reordering. **DIVERGE,
  order-only** — exactly the registered "predicted-candidate tie order"
  divergence below: the pin walks its DBM's physical bucket order, oxpinyin
  emits the defined text-ascending order. Zero content divergence.
- **tkrzw path** (Debian testing container: libpinyin 2.11.91-1, the
  tkrzw build Debian switched to in 2.11.91-1): the same shape — 1,571 =
  1,571 rows, sets identical, order-only. First measurement on this
  backend; no prior target existed.
- **Kyoto Cabinet path, NixOS packaging** (nixos/nix container:
  nixpkgs-unstable libpinyin 2.11.91, kyotocabinet 1.2.80, oracle at
  `/nix/store/…-libpinyin-2.11.91/lib/libpinyin.so.15`): identical to the
  Fedora measurement — 1,571 = 1,571 rows, sets identical, order-only,
  the same 2,562 reordered lines. Confirms the compat path is layout-
  portable (profile-symlinked `/nix/store` paths, no `/usr/lib`), not
  just RPM-shaped. Built with nixpkgs' rustc (1.95) under
  `--ignore-rust-version` — rustup toolchains cannot run on a pure Nix
  image — with the differential gating the artifact.
- **Punct rows, after the compat `punct.bin` reader** (2026-08-30, same
  differential with the driver's `punct-*` dump): every total rises
  1,571 → 1,588 on BOTH sides — 17 PREDICTED_PUNCTUATION rows per run
  (好 ，。; 是 “，：; 了 。，“！; …) — and the punct rows are identical
  between oracle and subject, order included, on the NixOS KC path
  (nixpkgs's 405 KB `punct.bin`, a TreeDB) and on the Debian tkrzw path
  (its own punct.bin, a TreeDBM). The reorder residual stays 2,562
  pred-row lines; the punct rows add zero divergence. Before the reader,
  the subject's punct table was always empty on these paths, so these
  rows could not have appeared at all.
- **libpinyin behaviour:** predicted candidates come back in the DBM's
  iteration order — backend-dependent, semantically arbitrary, different
  between the KC and tkrzw oracles themselves.
- **oxpinyin behaviour:** the `(length desc, amplified-freq desc)`
  ranking governs the emitted list; the text-ascending collection order
  (token-ascending within one text) resolves only the ties that ranking
  leaves — build-stable on every backend and on the compat paths.
- **Externally observable:** candidate POSITIONS in the predicted list
  differ; the candidate set does not. Ruled intentional by the
  "Predicted-candidate tie order" entry; these measurements close R1's
  open question by attributing the whole drop-in divergence to that one
  rule, on both real-data backends.

## zhuyin batch `FORCE_TONE` law — CLOSED (implemented in oxpinyin-core)

Initially reported as "parse restrictiveness" by the libzhuyin differential:
`zhuyin_parse_more_chewings` reported a non-zero `consumed` for toneless
syllables (`ta`=1, `li`=2, `ju`=2) where the pin reported 0. Root cause was
**not** the syllable-validation gate — it was the **batch parser ignoring
`FORCE_TONE`**.

- **Upstream source cite:** `src/zhuyin.cpp:1061` (batch chews pass
  `context->m_options`), `src/zhuyin.cpp:273` (`zhuyin_init` seeds
  `USE_TONE | FORCE_TONE`), `src/storage/zhuyin_parser2.cpp:176-180` (Simple
  `parse_one_key` rejects a toneless syllable under `FORCE_TONE`, nested
  under `USE_TONE`; `:373,:387` for Discrete; `:602` for CP26).
- **Mechanism:** with `FORCE_TONE` (part of the pin's zhuyin default), the
  batch parse rejects a syllable that carries no tone.
- **What oxpinyin now does:** `oxpinyin_core::ZhuyinParser::parse_with_options`
  was added (additive — the existing three-argument `parse`, used by the
  pinyin facade's `pinyin_parse_more_chewings`, is unchanged) to model the
  pin's option-word batch law. `oxpinyin-zhuyin-capi`'s
  `zhuyin_parse_more_chewings` passes the caller's full option word, so
  `FORCE_TONE` is honoured. `KeyProbe` now carries `force_tone` and rejects a
  toneless match per keyboard family (nested under `USE_TONE` for Simple and
  CP26, unconditional for Discrete).
- **Externally observable:** the differential now converges on the batch
  parse: with `USE_TONE | FORCE_TONE` both sides report `ta`=0, `li`=0,
  `ju`=0, `su3`=3, `ke3`=3 for the STANDARD keyboard.

## zhuyin candidate-tag grouping + `after(consumed)` terminal offset — CLOSED (display-law collapse + builder terminal mapping)

- **Upstream source cite:** `src/zhuyin.cpp:1272-1291`
  (`_prepend_sentence_candidates` prepends `m_nbest_results.size()`
  `BEST_MATCH_CANDIDATE` rows), `src/zhuyin.cpp:1460-1540`
  (`zhuyin_guess_candidates_after_cursor` returns `true` for a valid lookup
  into a non-empty matrix even with no candidate spanning the offset),
  `src/zhuyin.cpp:1542` (`zhuyin_guess_candidates_before_cursor`, same rule).
- **Mechanism:** (1) the pin prepends exactly `m_nbest_results.size()`
  sentence rows as `BEST_MATCH_CANDIDATE`, so on `su3` it tags one row
  `BEST_MATCH` and the rest `AFTER`; the engine's `candidates_at` emits a
  `Sentence` row per n-best sentence (two for `su3`), so the facade tags the
  second row `BEST_MATCH` too — candidate set and count identical (125), one
  label differs (ORDER-ONLY-like). (2) for an offset equal to the consumed
  length (`after(consumed)`), the pin returns `true` with 0 candidates (the
  matrix is non-empty); the facade returns `false` because the normalized
  offset is in original zhuyin-input coordinates while `candidates_at`
  expects session raw-buffer coordinates.
- **What oxpinyin does instead:** the facade's 4-value-enum tagging is
  faithful to the pin's prepend law; the row-count difference is the engine's
  n-best construction. The `after(consumed)` terminal-offset `false` is class
  (c) — the pin returns `true`, oxpinyin `false` — and stems from the same
  candidate-construction gap (coordinate mismatch between the original
  zhuyin input offset and the session's `'`-joined raw buffer).
- **Externally observable:** yes — `zhuyin_get_candidate_type` differs on the
  row after the top match, and `zhuyin_guess_candidates_after_cursor`
  answers `false` at `offset == consumed` where the pin answers `true`/0.
- **Classification:** engine workstream (n-best row count) + class (c)
  (terminal-offset availability). Neither is a facade defect: the candidate
  set is identical, and the terminal-offset case is not exercised by the
  pinned differential driver. The `after(consumed)` coordinate gap and the
  multi-syllable before-cursor and candidate-construction gaps below share the
  same root cause: `session.candidates()` is forward-anchored and the facade
  cannot UNION multiple `candidates_at` windows. One implementation direction
  covers all three: the backward-anchored window builder; see the next entry.

  (Amended 2026-08-31: the tag-grouping half is CLOSED — the row-count
  divergence was the **string-fill law**, not the n-best constants. Upstream
  zhuyin fills every `BEST_MATCH_CANDIDATE` row through `zhuyin_get_sentence`,
  which always reads `get_result(0)` (`zhuyin.cpp:1327-1330`, `:990-995`),
  unlike the pinyin surface's per-index `pinyin_get_sentence`
  (`pinyin.cpp:2004-2007`); identical strings collide in
  `_remove_duplicated_items_by_phrase_string`, which physically removes the
  duplicates (`zhuyin.cpp:1425-1438`), so exactly one sentence row is
  observable regardless of the n-best count. oxpinyin was applying the pinyin
  per-row law on the zhuyin surface. Fixed scheme-locally: the zhuyin facade
  sets `Session::set_collapse_sentence_rows_to_best(true)` and the prepend
  rides only the 1-best row — leaving `NSTORE`/`NBEST_ROWS` (and the shared
  trellis) untouched. Measured on the full-row differential
  (`tools/bisection/zhuyin-diff.c` now dumps every row): before the fix 253
  oracle rows vs 254 oxpinyin rows with `su3` candidate[1] `AFTER`/尼 vs
  `BEST_MATCH`/尼 and `su3u3` `after(0)` 128 vs 129 with candidate[1]
  `AFTER`/拟议 vs `BEST_MATCH`/你以 — after the fix the driver is byte-identical
  (revert-and-check: reverting the two source edits reproduces the 259-line
  diff). The `after(consumed)` terminal-offset half stays open for the
  backward-anchored window builder.)

  (Amended 2026-08-31, second half: the `after(consumed)` terminal offset is
  CLOSED by the same builder change — the original-offset mapping is now
  direction- and terminal-aware (`zhuyin_lookup_session_offset`): the
  terminal lookup answers the session buffer's one-past-end (the pin's
  reserved slot, where the span walk yields nothing and only the prepended
  sentence rows answer — `true` with the BEST_MATCH row, measured identical
  on the extended differential at after(consumed) for every corpus input),
  closing the class (c) coordinate gap. The tag-grouping and terminal-offset
  halves of this entry are both closed; see the before-cursor entry for the
  builder's full measured numbers.)

## zhuyin before-cursor candidate window — CLOSED (backward-anchored window builder)

The facade's `zhuyin_guess_candidates_before_cursor` originally reused the
composition-anchored cached candidate window, so `before(0)` wrongly returned
125 word candidates where the pin returns 0 (nothing precedes the first key).
That facade bug is fixed, but the fix is **correct only for a single-syllable
composition**; multi-syllable before-cursor is a genuine engine gap.

- **What oxpinyin now does (single-syllable):** the before-cursor path takes
  the composition window and filters to candidates whose consumed span ENDS
  at the requested original-offset (`snapshot_candidates`'s `before_end`). At
  offset 0 no span ends there (empty window, matching the pin); at the
  terminal offset the syllable's candidates are returned. `before(0)`=0 and
  `before(consumed)` match the pin on the single-syllable differential corpus.
- **Multi-syllable (measured on the two-syllable `su3u3`):** `before(3)`
  (first key boundary) matches the pin (125 on both), but `before(consumed)=5`
  does NOT — the pin returns 600 (the last key's 597 candidates plus the
  whole-composition sentence rows), oxpinyin returns 3. Root cause: the
  engine's forward-anchored `session.candidates()` does not enumerate the
  trailing keys' candidates, and the facade cannot UNION multiple
  `candidates_at` windows into one `CandidateList` (the engine does not
  expose `Candidate`/`CandidateList` construction). Fixing it requires an
  engine change: a backward-anchored window builder (the pin's
  `search_matrix` walk over spans ending at the offset).
- **Externally observable:** yes — `zhuyin_get_n_candidate` differs for
  `before(consumed)` on a multi-syllable composition. Registered as engine
  workstream, not a facade defect (the single-syllable ABI surface is
  correct).

  (Amended 2026-08-31: CLOSED — the engine gained the backward-anchored
  window builder `Session::candidates_ending_at(offset)` (additive, the
  `parse_with_options` seam pattern): the prefix graph's scan matrix walked
  per start `0..offset` ascending — the pin's longest-span-first `len` loop
  (`zhuyin.cpp:1575-1631`) — each span's slice ranked by the three-key order
  with its own previous-token gram, groups concatenated, the sentence rows
  prepended, one text dedup. The facade maps the original offset
  directionally (`zhuyin_lookup_session_offset`): the after family takes the
  right-key start, the before family the LEFT-KEY END — a mid-composition
  boundary is the apostrophe byte in the `'`-joined buffer, and upstream's
  walk answers the left syllable's candidates there — and the terminal
  offset answers the buffer's one-past-end. Sentence rows are exempt from
  the facade's before-end filter (the prepend law has no offset condition).
  Decomposition first, per review — measured on the instrumented oracle at
  0c5e80e1: `before(5)` on su3u3 = span (0,5) 3 phrase candidates + span
  (3,5) 597 + mid-syllable starts (1,2,4) no match (empty columns,
  `SEARCH_NONE`) = 600 phrases, +1 sentence row, −1 string-duplicate → 600;
  the register's earlier "597 + sentence rows" arithmetic was a
  simplification, as suspected. Measured differential (the driver now dumps
  n/TEXT/TYPE in full at after(0), after(consumed), before(0), before(3),
  before(consumed), over 11 inputs including three-syllable `su3u3u3`):
  before the fix `before(consumed)` on su3u3 is 3 (oxpinyin) vs 600 (pin)
  and `before(3)` 1 vs 126; after the fix the extended driver is
  byte-identical (revert-and-check: reverting the builder and facade edits
  reproduces a 1728-line diff).

  (Amended 2026-09-01, **STOP record and protocol split** — supersedes the
  "surface shift" framing the 2026-08-31 amendment closed with. **A STOP
  fired and was overridden.** The work order's STOP read: *"`before(3)` on
  `su3u3` ceasing to be 125 on both sides."* The Phase-2 baseline measured
  **1 (oxpinyin) vs 126 (pin)** on exactly that query, and the work
  continued on a reinterpretation ("agreement preserved") instead of
  stopping and reporting. Neither number in that baseline was the
  register's datapoint, but that is a finding, not an excuse: oxpinyin's 1
  came from a mid-implementation builder state (the exact-segment graph
  bug, since fixed) — not from the pre-change path — and the pin's 126 came
  from the driver's new guess-first sequence, where
  `zhuyin_guess_sentence` populates `m_nbest_results` and the prepended
  BEST_MATCH row rides every before-cursor answer (the prepend law has no
  offset condition). Re-measured on BOTH protocols, both sides, with the
  register-era binary rebuilt at its own base (`1451211`; the driver gained
  a `noguess` 4th argument so the protocol is a flag, not an accident):

  | su3u3 `before(3)` | pin | oxpinyin @ 1451211 | oxpinyin @ this branch |
  |---|---|---|---|
  | parse only (no `guess_sentence`) | 125 | **125** | 125 |
  | `guess_sentence` first | 126 | **125** | 126 |

  What this settles. The register's "125 on both" was a **real measurement
  under the parse-only protocol**, and the boundary agreement it recorded
  was real: at the FIRST key boundary every span ending at the offset also
  starts at the composition anchor, which is precisely the degenerate case
  the composition-anchored filter handles — the entry's single-syllable
  scoping was drawn from a datapoint that existed. But it was
  protocol-bound: under guess-first the register-era path never matched
  (125 vs 126 — its filter applied the span test to the sentence row and
  dropped it, where the pin prepends regardless of offset), so
  `before(3)` agreement at that boundary was always an artefact of the
  unguessed sequence. The final state matches the pin under BOTH protocols
  (125/125 parse-only, 126/126 guess-first), so the boundary now agrees
  for the right reason — the builder — rather than by the filter's
  coincidence.

  **Single-syllable closure re-examined — and one half of its recorded
  reason corrected.** The entry's closure sentence — "`before(0)`=0 and
  `before(consumed)` match the pin on the single-syllable differential
  corpus" — names no protocol, and under the guess-first protocol one half
  of it was false at the base: `before(0)` answered 0 against the pin's 1
  (the prepended row the old filter dropped). The same protocol-bound
  pattern as `before(3)`, a second instance, not a free-standing wrinkle.
  Re-measured on su3, both protocols, both sides:

  | su3 surface | protocol | pin | oxpinyin @ 1451211 | oxpinyin @ this branch |
  |---|---|---|---|---|
  | `before(0)` | parse only | 0 | 0 | 0 |
  | `before(0)` | guess first | 1 | **0** | 1 |
  | `before(consumed)` | parse only | 125 | 125 | 125 |
  | `before(consumed)` | guess first | 125 | 125 | 125 |

  So the closure sentence held in full only under the parse-only protocol;
  under guess-first only its `before(consumed)` half matched (the
  terminal-offset row survived the old filter because the sentence row's
  span maps to the whole parse, which equals the terminal offset).
  The closure itself stands at the final state under BOTH protocols and
  now rests on the general mechanism (the builder) rather than the
  filter's coincidence — but its recorded reason is now protocol-accurate.

  (Amended 2026-09-01, **baseline column re-measured** — the correction
  the before(3) STOP record forced, applied to the whole A2 baseline.
  The A2 commit message's baseline cells came from a mid-implementation
  run and are superseded by this table, measured at the base itself
  (1451211 rebuilt) under the declared guess-first protocol:

  | su3u3 surface | pin | oxpinyin @ 1451211 | oxpinyin @ this branch |
  |---|---|---|---|
  | `after(0)` | true/128 | true/129 | true/128 |
  | `after(consumed)` | true/1 | true/**2** | true/1 |
  | `before(0)` | true/1 | true/**0** | true/1 |
  | `before(3)` | true/126 | true/**125** | true/126 |
  | `before(consumed)` | true/600 | true/**4** | true/600 |

  Corrections this forces on the record:
  - The A2 baseline's "`after(consumed)` false vs true" is wrong twice
    over. At 1451211 the facade answers **true** — with 2 rows, both
    uncollapsed sentence rows riding the empty-column prepend after the
    coordinate mismatch landed the lookup on a mid-syllable column of the
    `'`-joined buffer — not false; and the pin answers true/**1** under
    this protocol, not true/0 (true/0 is the parse-only shape, which is
    what the entry's original "pin returns true with 0 candidates"
    observable measured). The coordinate-mismatch mechanism stands; the
    recorded observed value was true/2 vs true/1, the tag-grouping
    divergence itself.
  - The "3 vs 600" `before(consumed)` cell matches the parse-only shape
    (re-measured: 3 vs 600 there) and agrees with this entry's original
    registration, so that cell stands as recorded — but at the declared
    protocol the base answers **4** (the two uncollapsed sentence rows
    survive the old filter at the terminal offset; dedup then drops one
    against a same-string phrase).
  - The "`before(3)` 1 vs 126" cell was the contaminated one that fired
    the STOP; the base's measured value at the declared protocol is
    125, per the table above.)

## zhuyin multi-syllable candidate construction — CLOSED (the zhuyin display law, not the construction model)

The two-syllable differential input `su3u3` (ㄋㄧˇ ㄧˇ, consumed 5) exposes a
broader divergence than the `after(consumed)`/tag-gap above: the multi-syllable
candidate construction itself differs on the count, the phrase set, and the
tags.

- **Measured (oracle vs oxpinyin on `su3u3`):**
  - `n_candidates` at `after(0)`: **128 (pin) vs 129 (oxpinyin)** — the first
    candidate count differs (oxpinyin emits one extra row).
  - `candidate[1]` TEXT: pin `拟议`/`逆夷` at 1-2, oxpinyin `你以` at 1 — the
    multi-syllable phrase set diverges.
  - `candidate[1]` TYPE: pin `AFTER`, oxpinyin `BEST_MATCH` — oxpinyin carries
    a second `BEST_MATCH` row (`你以`) where the pin has exactly one sentence
    row (`你一` at 0).
  - `before(consumed=5)`: **600 (pin) vs 3 (oxpinyin)** — the before-cursor
    window diverges.
  - `before(3)` (first key boundary): 125 on both — matches.
- **Root cause:** the engine's forward-anchored `session.candidates()` builds
  the composition's candidates from the start; it does not enumerate the
  trailing keys' candidates (per-key at each position the way the pin's
  `search_matrix` walk does), and the facade cannot UNION multiple
  `candidates_at` windows into one `CandidateList` (the engine does not expose
  `Candidate`/`CandidateList` construction). The facade's tagging LAW is
  faithful to the pin's prepend rule, but the engine feeds it two sentence
  rows where the pin's `m_nbest_results` holds one, so the observed TYPE
  values differ as well (`candidate[1]`: `BEST_MATCH`/你以 vs the pin's
  `AFTER`/拟议).
- **Classification:** engine workstream (the candidate-construction model).
  Not a facade defect — the single-syllable facade surface is correct, and the
  facade faithfully translates what the engine provides. The differential
  corpus documents this as the known multi-syllable gap. This entry, the
  before-cursor multi-syllable gap above, and the `after(consumed)` / tag
  grouping entry all share one underlying cause and one implementation
  direction: the backward-anchored window builder mirroring the pin's
  `search_matrix` walk over spans ending at the offset.

  (Amended 2026-08-31: CLOSED — the divergence was the pinyin string-fill law
  riding the zhuyin surface, not the candidate-construction model: every
  symptom (count +1, `candidate[1]` TEXT `你以` vs `拟议`, TYPE
  `BEST_MATCH` vs `AFTER`) traces to the second n-best sentence row
  surviving where upstream zhuyin's display law collapses all sentence rows
  onto the 1-best string. See the candidate-tag grouping entry above for the
  law, the fix shape (`Session::set_collapse_sentence_rows_to_best`, set by
  the zhuyin facade only), and the measured before/after numbers. The full-row
  differential is byte-identical after the fix, revert-and-check proven.
  The `before(consumed)` half of the measured list stays open in the
  before-cursor entry above — that is the backward-anchored window builder's,
  not this entry's.)

## zhuyin n-best trellis constants: `PhoneticLookup<1, 1>` vs the engine's `<2, 3>` port — registered, not yet fixed

- **Upstream source cite:** `src/zhuyin.cpp:50` (`PhoneticLookup<1, 1> *
  m_pinyin_lookup` — `nstore = 1`, `nbest = 1` for libzhuyin) vs
  `src/pinyin.cpp:55` (`PhoneticLookup<2, 3>` for libpinyin); the beam/tail
  selection is `src/lookup/phonetic_lookup.h:330-341` (`get_tails` caps the
  results with `get_top_results<nstore>(nbest, …)`).
- **Mechanism:** the two upstream surfaces instantiate the same beam search
  with different constants. libzhuyin keeps ONE value per
  `(position, token)` trellis node and extracts at most ONE sentence tail;
  libpinyin keeps two and up to three. The candidate-list half of the
  observable difference is masked by the zhuyin display law (see the
  candidate-tag grouping entry — every sentence row displays the 1-best
  string and the dedup collapses them), but the trellis depth itself (which
  `(position, token)` values survive pruning, and with them the constraint
  and training walks' available rows) genuinely differs between the
  surfaces.
- **What oxpinyin does instead:** `crates/oxpinyin-engine/src/nbest.rs`
  hardcodes `NSTORE = 2` / `NBEST_ROWS = 3` — the pinyin instantiation — for
  both surfaces. Surfaced by the Phase-1 work on the candidate-window
  builder (2026-08-31); an earlier reading took the constants for the cause
  of the row-count divergence, which the string-fill law turned out to be.
- **Externally observable:** not through today's libzhuyin candidate
  surface (`zhuyin.h` exposes no per-index sentence getter, and the display
  law collapses the list), so no differential row moves today. It becomes
  observable through any future per-row sentence access on the zhuyin
  surface, and the pruning depth is measurable against the pin's
  constraint/train behaviour. Fix shape when taken: per-surface constants
  through const generics (the `parse_with_options` additive pattern), NOT a
  global constant edit — the full-pinyin corpus pins freeze the `<2, 3>`
  behaviour.

## zhuyin `FORCE_TONE` / `ZHUYIN_INCOMPLETE` default

- **Upstream source cite:** `src/zhuyin.cpp:273` (`context->m_options =
  USE_TONE | FORCE_TONE`; no `ZHUYIN_INCOMPLETE`).
- **Mechanism:** `zhuyin_init` seeds `USE_TONE | FORCE_TONE` and nothing
  else. `FORCE_TONE` is honoured by the chewing parser nested inside
  `USE_TONE` for the Simple / CP26 keyboards and unconditionally for
  Discrete (`zhuyin_parser2.cpp:178,373,387,602`); `ZHUYIN_INCOMPLETE` is
  OFF by default.
- **What oxpinyin does instead:** `CapiContext::open` seeds the same
  `USE_TONE | FORCE_TONE` word and defaults `incomplete` to `false`
  (matching the pin). The FORCE_TONE law is delegated to
  `oxpinyin_core::ZhuyinParser::parse_with_options`, which honours it in the
  Simple/CP26 (nested) and Discrete (unconditional) shapes — the
  implementation matches the pin's three shapes.
- **Externally observable:** no, with the corrected default — the differential
  was run with the pin's default word (`USE_TONE | FORCE_TONE`, no
  `ZHUYIN_INCOMPLETE`) and the parse-length gap above is the only residual.
  Entry kept so a future consumer that sets the bit finds the law already
  analysed.
