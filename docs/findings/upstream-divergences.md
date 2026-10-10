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

- **Restated 2026-09-27 UTC (#530, audit D-20; policy row 1).** The
  original record below called this a stale-buffer reuse that crashes on
  a *repeated* export cycle. Re-read at `074a2219`, the defect is an
  unterminated string vector, and it crashes the *first* export cycle
  after a train.
- **Upstream source cite:** `src/pinyin.cpp:842-872`
  (`pinyin_bigram_iterator_has_next_phrase`): `:844-850` and `:857-863`
  build each pronunciation with `g_ptr_array_new`, free the array with
  `g_ptr_array_free(…, FALSE)` and pass the result to `g_strjoinv` and
  `g_strfreev`; nothing adds the terminating NULL. The unigram export
  does add it (`:726-734`, `g_ptr_array_add(array, NULL)` at `:730`).
- **Mechanism:** `g_strjoinv`/`g_strfreev` walk a `gchar **` until a
  NULL; the array has none, so both read past its end into whatever
  the heap holds there — an out-of-bounds read whose outcome depends on
  the allocation's neighbourhood. Any export that reaches a row with a
  pronunciation takes the path.
- **What oxpinyin does instead:** `CapiContext::export_bigram_rows`
  renders every row into owned Rust strings when the iterator is created
  (`crates/oxpinyin-capi/src/iterators.rs:336`), so no join over a C
  array exists to over-read, on the first cycle or any later one.
- **Externally observable:** yes — the audit's execution
  (`bigram-before-save.c`): after one train, the first export cycle
  SIGSEGVs in `pinyin_bigram_iterator_has_next_phrase` on tkrzw and kc
  whether or not the user dir was saved, and on bdb unsaved (4 of 4
  runs); oxpinyin completes the cycle. Class (b): the pin's behaviour is
  undefined, and no safe construction reproduces a read past an
  allocation. Also cross-indexed in `reference/memory-safety-bugs.md`.
- **Original record (2026-08, superseded in mechanism and scope by the
  restatement above):** the export iterator keeps C pointers into
  reused pronunciation/join buffers; repeating an export cycle inside
  one context reuses stale storage and the pinned oracle segfaults.
  The per-round train differential runs one export per fresh context
  for the oracle and compares those rows to oxpinyin
  (`tools/bisection/run-train-diff.sh`).

### Public bigram export is a rendering surface

- **Upstream source cite:** `src/pinyin.cpp:775-918`.
- **Mechanism:** the public bigram iterators render the store: sentence-start
  predecessors are dropped, counts are doubled, below-threshold rows are
  hidden, pronunciations are expanded as a Cartesian product, and
  per-predecessor totals are unreachable.
- **What oxpinyin does instead:** the C ABI reproduces that rendering for
  compatibility. The full value surface is reachable only below the public
  iterators; the one-time migration tool that would have read it (linking
  the pinned `libstorage.a` through a dump shim,
  `docs/findings/legacy-migration.md` §3) was cancelled — that document is
  SHELVED and the implementation stays on the parked branch
  `feat/w7-t2-legacy-migrate`.
- **Externally observable:** yes — the C ABI surface matches the pin's
  rendering; nothing in-tree reads the raw store.

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

- **Status (2026-10-04, lane C, PR 12e):** class (c), **both halves met**: the pin's sites are `storage/pinyin_phrase3.h:152` and `pinyin.cpp:2769`, both **`assert`**; oxpinyin answers `false` with exactly one `libpinyin` warning from `pinyin_get_pinyin_is_incomplete`, `pinyin_guess_candidates`, `pinyin_guess_sentence` and `pinyin_guess_sentence_with_prefix` (`compatibility-policy.md` row 4). It returned candidates (and `true`) before.

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
  as the pin does, then refuses every lookup over a matrix that holds
  one — `false` and one warning, nothing searched (constitution 4:
  nothing panics).
- **Externally observable:** yes — the pinned oracle SIGABRTs on `n4`
  under `USE_TONE | PINYIN_INCOMPLETE` as soon as candidates are
  guessed; oxpinyin answers `false` with the warning. The abort is held
  by the `abort-*-toned-initial*` cases of `contract-diff.py`; the
  fullpin-diff tone sweep keeps its exclusion. Report-back candidate
  for libpinyin.

### Scheme setters abort or half-mutate on the no-op slots

- **Status (2026-09-27 UTC, #549):** class (c), **pending logging (lane C, #525)** — for the abort slots: double 30 `storage/pinyin_parser2.cpp:611`, zhuyin 7 `storage/zhuyin_parser2.cpp:295`, zhuyin out-of-enum `pinyin.cpp:1189` (not `:1188`) with libzhuyin's `zhuyin.cpp:736`, full-pinyin out-of-enum `storage/pinyin_parser2.cpp:398` — all **`abort()`**, so they abort in an `-DNDEBUG` build too. oxpinyin's `false` holds; the log line is owed (rows 5a, 5c, 5d; 5b stays closed).

The #109 contract-lock (all rows verified at `0c5e80e1`; row 5b
reverted 2026-09-15; remaining rows pinned by
`crates/oxpinyin-capi/tests/abi/contract.rs`):

- **double CUSTOMIZED (30)** — upstream aborts mid-call inside
  `DoublePinyinParser2::set_scheme` (`pinyin_parser2.cpp:611-612`)
  after the unconditional fallback clear already ran. The API wrapper
  `pinyin_set_double_pinyin_scheme` (`pinyin.cpp:1154-1159`) never
  returns.
- **double out-of-enum (negatives, 0, 7–29, 31+)** — the parser clears
  `m_fallback_table` first (`pinyin_parser2.cpp:580`), returns `false`;
  the wrapper ignores the result and answers **`true`**
  (`pinyin.cpp:1155–1159`). A live fallback-bearing scheme
  (ZRM/PYJJ/XHE) silently loses its fallback while the caller is told
  the call succeeded: a half-mutation. **Reproduced 2026-09-15:** the
  CAPI returns `true` and clears the fallback, matching the pin; the
  shengmu/yunmu tables stay intact.
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
for the remaining abort rows (double 30, zhuyin 7 and out-of-enum,
full-pinyin out-of-enum) oxpinyin's `false` + unchanged is the
non-aborting contract the constitution requires; the double
out-of-enum half-mutation (row 5b) now reproduces the pin's lied
`true` + cleared fallback. No oracle differential is possible for
the abort rows (the pin-built `.so` SIGABRTs); the half-mutation is
observable through a following parse that would have used the
fallback (the contract test's `aa` probe after the out-of-enum call).

(Amended 2026-09-16: the `run-scheme-diff.sh` oracle differential with
the out-of-enum values (99, −1 under ZRM, in `scheme-diff.c`) ran in a
debian:testing container, both sides on tkrzw, the capi on a P6-native
data directory: the probe rows are byte-identical on both sides — the
setter answers `true`, baseline `aa` consumed=2/n=8, after each
out-of-enum value consumed=0/guess false/n=0, restored ZRM
consumed=2/n=8 — and the whole-log diff is IDENTICAL, exit 0, no SKIP
line.)

### Constraint-aware train without the consistency assert

- **Status (2026-09-27 UTC, #549):** class (c), **pending logging (lane C, #525)** — and **needs guard, not just a log**: `lookup/phonetic_lookup.h:868` is an **`assert`**; oxpinyin trains and answers the gate result, so neither half holds (row 6).

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

- **Status (2026-09-27 UTC, #549):** class (c), **pending logging (lane C, #525)** — `pinyin.cpp:1474` is an **`assert`**; oxpinyin's `false` holds (`crates/oxpinyin-capi/src/sentence.rs:132-154`); the log line is owed (row 10).

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

### One bigram-prediction row differs on the pin's own data (trellis residual)

- **Closed 2026-10-03 with row 33 (#614, merge commit `33e2e54d`):** `union-diff` is
  pin-identical on `main` (`IDENTICAL (11 log lines)` on tkrzw, bdb and kc);
  the extra line described here is gone.
- **Reattributed 2026-09-27 UTC (#550, audit D-05; policy row 20 → row
  33).** The line is not a trellis residual. The union driver's
  whole-row NBEST choose installs no `CONSTRAINT_ONESTEP` on the pin
  (`src/pinyin.cpp:2515-2520`), so `train_result3` trains nothing
  (`src/lookup/phonetic_lookup.h:866`) and the pin writes no `测测 → 你`
  at all; oxpinyin's selection-history fallback
  (`crates/oxpinyin-engine/src/session/selection.rs:298-339`) writes it
  with count 138 and predicts 你. That is row 33's mechanism, and the
  audit measured it on every cell, not only on Kyoto Cabinet. The
  bullets below are the original record; their "straddle the
  `m_count ≥ 10` filter" explanation is superseded.

- **Where:** `tools/bisection/run-same-data-dir-diff.sh union-diff` on a
  `--with-dbm=KyotoCabinet` libpinyin install's own `data/` — one line,
  `pred: type=4 text=你` (a `PREDICTED_BIGRAM_CANDIDATE`), present on
  oxpinyin's C ABI and not the pin's, after the union differential's
  train-then-predict sequence for `测测`.
- **Mechanism:** downstream of the registered n-best trellis divergence
  (below). The union driver's step 3 chooses an n-best row for
  `cecenihao` and trains the constrained decode; the first decoded phrase
  after `测测` differs by the trellis's gfloat-vs-fixed-point residual, so
  the two engines write `测测 → 你` into the user bigram with counts that
  straddle `_compute_predicted_bigram_candidates`'s `m_count ≥ 10` filter
  (`pinyin.cpp:2340-2366`). At that one prefix oxpinyin's trained count
  clears 10 and the pin's does not, so oxpinyin emits the extra
  single-character bigram prediction.
- **Independent of the P6 runtime switch:** the user-store training code
  (`oxpinyin-user`) is unchanged by P6, and the divergence is stable
  across the scoring change; the standard `run-union-diff.sh` (oxpinyin on
  its own generated data vs the pin on its own) stays green, because there
  the two decodes settle the same way. It surfaces only when oxpinyin
  reads the pin's exact bytes and the trellis residual tips this filter.
- **Externally observable:** yes — one predicted-bigram row on this one
  trained-prediction edge. Not reducible without matching the pin's float
  trellis bit-for-bit, which the fixed-point decoder deliberately does not
  (see below). Every other `union-diff` line, and the whole
  `dict-surface` / `pred-order` / `live-typing` / `nbest-train` /
  `phrase-surface` / `predict` / `punct` / `addon` / `user-candidate` /
  `import` / `key-surface` differential, is identical on the pin's data.

### N-best trellis accumulates gfloat log costs — not reproducible in fixed point, FROZEN as a permanent Stage-1 divergence

**Current scope (2026-10-04 UTC, lane D):** selection is ported. Only bo, quguan, kaliantai, huilianpei, yunhoulvlvlunqianaonaoruoqubeiji and nou'y are class (a); the four separator residuals remain unattributed by name in sentence-surface.md §12's lane-D amendment. Current gate: 500/500/500 of 500 after the #639 resplit repair (495/495/495 of 496 at the port; corpus 14/10,465 before the repair). The historical record below is superseded for current scope and counts.

- **Scope shrinks, pending lane D (2026-09-27 UTC, #550, #535, audit
  D-13; policy row 11).** Two rules inside the frozen residual are
  selection logic, not arithmetic, and are not class (a): the node keep
  rule (the pin's `trellis_node::eval_item` heap front is the best
  value, replaced when a newcomer beats it —
  `src/lookup/phonetic_lookup_heap.h:25-29`, `:56-81`; oxpinyin replaces
  its worst, `crates/oxpinyin-engine/src/nbest.rs:241-267`) and the
  comparator's longer-by-one clause (dead at the pin, `:75-77` being
  subsumed by `:87-88` of `phonetic_lookup.h`; live in oxpinyin,
  `nbest.rs:194-197`). Selection-logic specimen D-26: ZiGuang
  `zhrgguor`, candidate[2] 宗人光卓然 on the pin, 总人光卓然 on
  oxpinyin, rows 0–1 identical (#535). Class (a) keeps the `gfloat`
  `log` accumulation only.

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
  (`fixtures/w4/oracle-sentence-surface.txt`): 1-best 491/496, n-best
  distinct-set 396/496, n-best ordered / first-6 rows 390/496; the 106
  ordered misses are all trellis-side (0 candidate-surface leaks). The
  candidate surface, which does not share this arithmetic, is
  bit-identical. Frozen as a permanent Stage-1 divergence by maintainer
  ruling 2026-09-02 at 488/385/379 and re-frozen 2026-09-04 at 491/396/390
  after P6 (`345af16d`) moved the trellis's P_unigram source —
  `sentence-surface.md` §12, asserted by
  `crates/pinyin-oracle/tests/sentence_surface_parity.rs`; enumerate with
  the read-only `pinyin-oracle` `sentence-tail` binary.

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
  stages (pre-P6 description, kept as the record; the P6 amendment below
  carries the current mechanism). (1) Collection —
  `SystemDictionary::suggest_after`
  (then `crates/oxpinyin-data/src/dict.rs:196-217`) walked a
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

1. **Superseded by P6 (2026-09-02).** Once the runtime reads the phrase
   DBM directly, the suggestion order is reproduced from the *same* file
   the pin walks: `SystemDictionary::suggest_after` +
   `resolve_suggestions` (`crates/oxpinyin-data/src/dict.rs`) group the
   tokens by library nibble ascending (the `reduce_tokens` concatenation)
   and, within a group, in the DBM's byte-lexical UCS-4 cursor order —
   exactly `PhraseLargeTable3::search_suggestion`'s walk. On the pin's own
   KC `data/`, `pred-order-diff` is now **IDENTICAL** (1588 log lines, 0
   position mismatches), not the text-ascending near-miss below. The
   text-ascending fallback remains the *defined* order for oxpinyin-native
   backends (redb/LMDB), whose container is not the pin's; on KC and tkrzw
   the pin's own order is reproduced because the file is the pin's own.
   The historical text-ascending decision is kept below for the record.
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
(`dict.rs`; since P6 `search_suggestion` + `resolve_suggestions`), `append_predicted_prefix` (`predict.rs`), and the user
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
per-text token vector stays token-ascending (`build_text_tokens` then;
`resolve_suggestions` in `crates/oxpinyin-data/src/dict.rs` since P6), so
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
  — the no-abort policy already recorded in `docs/testing/oracle-bisect-differential-abort.md`.
- **Externally observable:** not in practice — no known frontend passes a
  mid-syllable offset to `pinyin_guess_candidates`; fcitx5-oxpinyin and
  ibus-libpinyin both snap the cursor with `pinyin_get_pinyin_offset` first,
  and at a snapped (syllable-aligned) offset the windows are identical. A
  caller that bypasses the snap now sees the pin's empty-column window on
  both engines. This closes the residue the 2026-08-27 amendment of
  `uncovered-surface-differentials.md` recorded as not chased.

### The cursor helpers' `_check_offset` aborts answer `false` — not the pin's abort, not post-`95e3af7` upstream's discarded-`false` true

- **Status (2026-09-27 UTC, #549):** class (c), **pending logging (lane C, #525)** — at `074a2219` the aborts are the **`assert`**-wrapped calls `pinyin.cpp:3035`, `:3057`, `:3067`, `:3092` (`_check_offset` itself, `:2163-2182`, returns `false`); the `:2175` cited below is the `0c5e80e1` pin's line. oxpinyin's `false` holds; the log line is owed (row 14).

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
  sightings consolidated in `docs/testing/oracle-bisect-differential-abort.md`
  (the W11 bisect abort, the ibus-libpinyin#570 guess-seam pattern,
  and the shared root cause), the guess-seam leading-run answered as
  `LookupOffsetPastSeparator`, and this cursor-helper seam.

**Amendment — 2026-09-06 UTC, oracle pin 0c5e80e1 → 074a2219 (runtime
verification).** The pin bump re-measured this entry's shape with a
fork-per-probe C driver against the oracle at BOTH pins and the port
(evidence: `docs/findings/oracle-pin-074a221-evidence/`). Two
narrowings, no new divergences:

1. **The legal-boundary arm is CLOSED by upstream.** At an offset one
   past a lone zero-key column (e.g. `pinyin_guess_candidates("ni'", 3)`,
   the one-past-end boundary), the old pin SIGABRTs (`pinyin.cpp:2175`
   at 0c5e80e1); at 074a221 the same call completes and returns `true`
   with an empty candidate list — and the port answers exactly that
   (`true`, 0 candidates). Upstream moved to the port here; this entry's
   "diverges from BOTH arms" claim no longer holds at the legal
   boundary.
2. **The residual divergence is the illegal-offset arm only** (beyond
   one-past-end, e.g. offset 4 on the same input): the new oracle
   returns `true` with the computed value (the discarded check no longer
   rejects out-of-range offsets — the upstream construct defect that
   `assert(_check_offset(...))` compiles the call away under NDEBUG at
   six of ten sites), while the port keeps answering `false` — the
   already-registered abort-on-caller-input class answered by the
   no-abort policy. Not a new finding; see
   `docs/findings/oracle-pin-074a221-verification.md` §V3.

Also corrected by the same measurement: of 074a221's four bare
`_check_offset` call sites, `pinyin.cpp:3251` is dead code (`#if 0`),
`pinyin_get_pinyin_key`/`_pinyin_key_rest` (:2933/:2956) return `false`
at their range guard before reaching the check, and
`pinyin_get_character_offset`'s live check is the assert-wrapped
`:3204` (aborts at both pins). The single observable pin-to-pin change
in this family is `pinyin_guess_candidates` (:2226), covered above.
The port-side `get_character_offset` true-on-invalid-phrase behavior
seen in the same probe is a separate parity defect: issue #356.

### Apostrophe-only input: the pin consumes every byte, the engine consumes none — CLOSED

- **Status (amended 2026-09-06):** closed by 678f3259 (2026-08-26, the
  B2 parser-termination class): `SegmentGraph` propagates each
  apostrophe one byte, counted, so `'` → 1, `''` → 2, `'''` → 3 match
  the pin (pinned in `crates/oxpinyin-core/src/graph.rs`). The
  "what oxpinyin does instead" text below describes the pre-fix state
  and is kept as the record; the cursor helpers' class-(c) `false` at
  the `_check_offset` abort shapes is unchanged.

- **Upstream source cite:** `src/pinyin.cpp` parse path over
  `FullPinyinParser2` (`src/storage/pinyin_parser2.cpp`): the pin emits a
  zero `ChewingKey` per `'` separator and counts it in `m_parsed_len` —
  measured on the pin: `'` → parse_return 1, `''` → 2, `'''` → 3
  (the table in `docs/testing/oracle-apostrophe-abort.md`, F-E-14).
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
  `docs/testing/oracle-apostrophe-abort.md`.

### The single-key surface aborts the pin where oxpinyin answers `false`

- **Status (2026-09-27 UTC, #549):** class (c), **pending logging (lane C, #525)** — `storage/pinyin_parser2.cpp:170`, `pinyin.cpp:499` and `:466` are **`assert`**; the empty-input reads (`pinyin_parser2.cpp:178`, `zhuyin_parser2.cpp:171`) are over-reads, not aborts. oxpinyin's `false` holds; the log line is owed (row 21).

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
  Rust ABI suite (`crates/oxpinyin-capi/tests/abi/keys.rs`); the differential excludes these
  shapes with the exclusion documented in the driver.
- **Externally observable:** yes — upstream SIGABRTs on the same calls
  oxpinyin answers. Report-back batch: file with the scheme-setter and
  `_check_offset` assert families.

### `pinyin_get_character_offset`'s recursion asserts answer `false`

- **Status (2026-09-27 UTC, #549):** class (c), **pending logging (lane C, #525)** — the sites are `pinyin.cpp:3147`, `:3161`, `:3203`, `:3204` and `zhuyin.cpp:2110`, `:2158`, all **`assert`** (the `:3152`/`:3166` below are five lines stale; `:3172` is now `:3168`). oxpinyin's `false` holds; the log line is owed (row 19).

- **Upstream source cite:** `pinyin_get_character_offset`
  (`src/pinyin.cpp:3193-3241` at 074a221; `zhuyin.cpp:2148-2196` for the
  zhuyin twin), `_pre_compute_tokens` (`pinyin.cpp:3098-3136`) and
  `_get_char_offset_recur` (`pinyin.cpp:3138-3191`).
- **Mechanism:** the function asserts `offset < matrix.size()` and
  `_check_offset` (the register's #14 family — the offset 3/4 rows of
  issue #356's matrix), then the recursion asserts a non-empty column at
  every stepped-to position (`assert(size > 0)`, `:3152` — an input with
  a leading apostrophe run has an empty column 0) and a lone zero key
  where one appears (`assert(1 == size)`, `:3166`). Separately, the
  recursion indexes `cached_tokens` by the characters consumed so far
  with no bound (`g_array_index`, `:3172`): a phrase shorter than the
  key path measured against it reads the array's zero terminator
  (`null_token`, whose `get_phrase_item` fails and leaves the previous
  item) and beyond it the heap — the aux-text over-read's class.
- **What oxpinyin does instead:** the ported law answers the pin's
  `false` rows exactly (`_pre_compute_tokens` finding no token for a
  phrase character — the #356 defect, fixed in
  `oxpinyin-engine/src/char_offset.rs`; a walk no key path satisfies)
  and answers `false` where the pin asserts:
  `EngineError::LookupOffsetOutOfRange`, `EngineError::ZeroKeyOffsetCheck`
  and the new `EngineError::MatrixColumnAssert` rendered as the C ABI's
  `false`. A walk past the cached tokens treats the missing token as a
  pronunciation miss, so that branch fails like a mismatched key rather
  than reading a stale item.
- **Externally observable:** yes — the pin SIGABRTs where oxpinyin
  answers `false` (issue #356's offsets 3 and 4 on `ni'`), and reads off
  its token array where oxpinyin answers a deterministic `false`. The
  consumers (`PYPPinyinEditor.cc:290`, `PYPBopomofoEditor.cc:404`) pass
  the guessed sentence and a normalized cursor offset, which reach
  neither shape. Report-back batch: file with the `_check_offset` assert
  family.

### FORCE_TONE — scheme-specific: every seam honours its scheme law except the pinyin facade's chewing batch (its own entry below); zhuyin batch closed (1671954); double-pinyin batch closed (5ec782ea)

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
  (`pinyin_parse_more_double_pinyins`) was closed subsequently — see the
  Double-pinyin batch closure amendment below.
- **Externally observable:** no longer on the one-key seams, the
  libzhuyin batch seam and the double-pinyin batch seam — all answer
  identically to the pin under every FORCE_TONE profile (one-key seams:
  D3 gate; zhuyin batch: 1671954; double-pinyin batch: 5ec782ea). Still
  on the pinyin facade's chewing batch seam (`pinyin_parse_more_chewings`,
  `ToneForwarding::PinFacade` does not forward FORCE_TONE) — its own
  entry below, row 30 of the policy table. The
  full-pinyin seam itself matches the pin (capi e2e `parse_termination`
  module, harness phase-C 0x60 probes closed).
- **Freeze correction (2026-09-02, historical).** Before the batch
  closure, the freeze-time observable-shape sentence read as if the batch
  seam applied the full-pinyin FORCE_TONE law; the batch parser of that
  time was more precisely option-blind: it ran the tone-less profile
  whatever the caller's option word (the greedy walk rejected every
  three-byte key and retried length 2). The divergence was the same in
  every FORCE_TONE profile — oxpinyin observably less restrictive than
  the pin (which consumes nothing at all under FORCE_TONE without
  USE_TONE, and three-byte toned keys under USE_TONE|FORCE_TONE) — but
  the mechanism was absence of the law, not the full-pinyin law. The
  frozen SPEC fixed the law; the Double-pinyin batch closure amendment
  below implemented it.
- **Double-pinyin batch closure (5ec782ea, 2026-09-02).** The batch
  double-pinyin `parse` surface (`pinyin_parse_more_double_pinyins`) now
  honours the frozen SPEC's Tone law: the caller's full option word
  crosses the seam (the pin's `options = context->m_options`,
  `src/pinyin.cpp:1543`) and drives `DoublePinyinParser::parse_with_options`
  — the additive option-word seam the zhuyin batch closure established.
  `FORCE_TONE` rejects any key that is not exactly three bytes
  (`pinyin_parser2.cpp:412`); a three-byte key carries its trailing
  `1`..`5` digit as the tone only under `USE_TONE` (`:439-451`), so
  FORCE_TONE without USE_TONE consumes nothing at all. The parsed tone
  rides the key into the exact segments. Measured in the debian-testing
  gate container against the pinned tkrzw oracle over full model20 KC
  tables: the scheme differential is byte-identical for double schemes
  1, 2, 4, 5, 6 including the new `tonelaw` probe section (142 lines
  each: three FORCE_TONE/USE_TONE profiles over thirteen tone-digit
  inputs) and for all eight bopomofo keyboards; `run-key-surface-diff.sh`
  stays IDENTICAL (2,131 probe lines). Revert-and-check: the pristine
  parser diverges from the pin on 130 of the new probe lines under the
  same driver. The one residual in the comparison — scheme 3 (Ziguang)
  NBEST row 2 on `zhrgguor` (pin 宗人光卓然 / oxpinyin 总人光卓然; the
  1-best and 2-best rows agree) — is the pre-existing §12 trellis
  hypothesis-selection class, unchanged by this closure (the same
  revert-check reproduces it) and first surfaced by the full-model
  scheme sweep.

### Empty-string phrase lookup SIGFPEs the pin — corrected: the crash is `pinyin_lookup_tokens("")` — WITHDRAWN

- **Withdrawn (2026-10-03 UTC, maintainer ruling):** not reproduced at `074a2219`: no crash and valgrind clean on tkrzw, bdb and kc (2026-10-03); the audit's original reproduction is unavailable; claim withdrawn (policy row 22). The record below is kept as written.
- **Status (2026-09-27 UTC, #549):** class (c), **pending logging (lane C, #525)** — and the symbol is corrected. The audit's execution (R-2) crashes the pin on `pinyin_lookup_tokens(instance, "", …)`, which hands a zero length straight to `m_phrase_table->search` (`pinyin.cpp:2652-2667`); `pinyin_phrase_segment(instance, "")` answers `true` — `PhraseLookup::get_best_match` with length 0 runs no search (`lookup/phrase_lookup.cpp:119-149`: `nstep - 1 == 0`). The bullets below are the original record and name the wrong entry point. Site kind: a fault signal, neither `assert` nor `abort()`. oxpinyin answers `false` for the token lookup (`crates/oxpinyin-capi/src/dict.rs:86`) — the false half holds; the log line is owed (row 22). For `pinyin_phrase_segment("")` the two sides agree on `true`: oxpinyin's span DP backtracks from the start node to an empty result (`crates/oxpinyin-engine/src/phrase.rs:124-130`, `:181-201`), so the "oxpinyin answers `false`" below is stale as well.

- **Upstream source cite:** `pinyin_phrase_segment` →
  `PhraseLookup::get_best_match` with `sentence_length = 0`
  (`src/lookup/phrase_lookup.cpp:121-157`), reaching
  `m_phrase_table->search(0, ...)`; measured SIGFPE (gdb: divide in the
  search path) on the pin-built oracle. **Re-measured 2026-10-03: not
  reproduced** — see policy row 22; the crash claimed here could not be
  re-captured on freshly built tkrzw, bdb and kc oracles.
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
  selection (the `DefaultStore` cfg chain, exactly one backend feature per
  build), but the file names follow the backend family since P6
  (`345af16d`): on Kyoto Cabinet, tkrzw and Berkeley DB — the three DBMs
  libpinyin itself builds against — the files carry libpinyin's own
  constants (`pinyin_index.bin`, `bigram.db`, …), so an install is name-
  and byte-compatible; only the oxpinyin-only containers (redb, LMDB)
  carry `<stem>.<ext>` (`pinyin_index.redb`/`.lmdb`).
  `DEFAULT_STORE_IS_LIBPINYIN_DBM` (`crates/oxpinyin-store/src/lib.rs`) is
  the switch.
- **Externally observable:** only in redb and LMDB data directories,
  which no libpinyin build can open anyway; on Kyoto Cabinet, tkrzw and
  Berkeley DB the files are libpinyin's own fixed names (`bigram.db`,
  `*.bin`), so no libpinyin consumer sees a difference; recorded because
  the naming intentionally diverges from the pin's constants rather than
  mirroring them.
- **Amendment (2026-09-13, drop-in task 10).** Berkeley DB landed as the
  fifth peer and the third libpinyin DBM, so "the two DBMs" this entry
  named is now three and `bdb` sits inside the name-compatible set — it
  carries libpinyin's own constants, not `<stem>.<ext>`. The precedence
  chain the entry used to cite (`kyotocabinet > tkrzw > lmdb > redb`) is
  also gone: an exactly-one-backend `compile_error!` guard refuses any
  build naming none or more than one backend feature, so no order
  survives to fall back on, and the default selection has been tkrzw
  since 2026-09-05. The naming decision itself is unchanged.
- **Amendment (2026-09-27 UTC, Q1 ruling).** The last clause above is
  history: the default selection was tkrzw from 2026-09-05 to
  2026-09-20 and has been Berkeley DB since (`default = ["bdb"]`, e.g.
  `crates/oxpinyin-capi/Cargo.toml:32`), matching the reference build's
  bare `./configure` (`configure.ac:94` at `074a2219`). Neither tkrzw nor
  Kyoto Cabinet is the default; `compatibility-policy.md`, "Amendment —
  rulings recorded", item 1.

## R1 measured on the drop-in compat paths — order-only, sets identical (2026-08-30)

> **SUPERSEDED (see architecture correction).** The libpinyin drop-in /
> compat loader described in this section has been removed. oxpinyin
> reads only its own peer-backend tables (KC, redb, LMDB, tkrzw); it does
> not detect or read libpinyin's on-disk DBM files. The measurements
> below are preserved as a historical record of what the (removed)
> compat path did.
>
> **Amended 2026-09-06:** the banner above was true between f8b81d61
> (2026-08-30) and P6 (345af16d, 2026-09-02) only. Since P6 the runtime
> reads an unmodified libpinyin install's `data/` directly on KC and
> tkrzw through the same readers it uses for its own output — there is
> no compat *layer*, but the drop-in read is back and is the shipped
> shape. The R1 measurement below is reproduced on that path:
> `pred-order-diff` is IDENTICAL on the pin's own KC `data/` (see the
> predicted-candidate entry's P6 amendment).
>
> Everything below this banner — the harnesses, the 2,562 reorder lines,
> the DIVERGE/order-only verdicts — is the 2026-08-30 measurement of the
> removed compat path, kept verbatim as the historical record. It is not
> the current result; the current result is the IDENTICAL above.

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

- **Re-closed 2026-10-03 (#577; #623, merge commit `cb824981`, code
  `7bea86d1`; policy rows 25 and 26):** a mid-key after-cursor lookup
  answers the sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`);
  `zhuyin-diff` now drives offsets 1, 2, 4 and 5 of both families and a
  choose battery, 4558 lines identical on tkrzw, bdb and kc; the
  choose-then-guess half is #609 (`563ed9b1`).
- **Contradicted by #577 (2026-09-27 UTC; audit Z-3; policy rows 25
  and 26), pending investigation — the record this closure answers.** On all three cells, for the chewing
  input `su3cl3` after `zhuyin_guess_sentence`,
  `zhuyin_guess_candidates_after_cursor` at offsets 1, 2 and 5 answers
  n=1 on the pin and n=126/126/94 on oxpinyin, and after a choose
  `zhuyin_guess_candidates_before_cursor` answers 94/126 on the pin and
  1/94 on oxpinyin (pin `src/zhuyin.cpp:1460-1541`, `:1542-1600`;
  oxpinyin `crates/oxpinyin-zhuyin-capi/src/sentence.rs:181`, `:198`).
  `zhuyin-diff` drives no mid-key offset. The CLOSED status below is
  contradicted until #577 is investigated.

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

- **Re-closed 2026-10-03 (#577; #623, merge commit `cb824981`, code
  `7bea86d1`; policy rows 25 and 26):** a mid-key after-cursor lookup
  answers the sentence rows alone (`zhuyin.cpp:1498-1512`, `:1624-1626`);
  `zhuyin-diff` now drives offsets 1, 2, 4 and 5 of both families and a
  choose battery, 4558 lines identical on tkrzw, bdb and kc; the
  choose-then-guess half is #609 (`563ed9b1`).
- **Contradicted by #577 (2026-09-27 UTC; audit Z-3; policy rows 25
  and 26), pending investigation — the record this closure answers.** On all three cells, for the chewing
  input `su3cl3` after `zhuyin_guess_sentence`,
  `zhuyin_guess_candidates_after_cursor` at offsets 1, 2 and 5 answers
  n=1 on the pin and n=126/126/94 on oxpinyin, and after a choose
  `zhuyin_guess_candidates_before_cursor` answers 94/126 on the pin and
  1/94 on oxpinyin (pin `src/zhuyin.cpp:1460-1541`, `:1542-1600`;
  oxpinyin `crates/oxpinyin-zhuyin-capi/src/sentence.rs:181`, `:198`).
  `zhuyin-diff` drives no mid-key offset. The CLOSED status below is
  contradicted until #577 is investigated.

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

  (Amended 2026-09-05, **the choose leg of the same surface** — the window
  this entry closed was displayed correctly but could not be chosen from.
  `zhuyin_choose_candidate` resolved the snapshot row's `source_index`
  through `Session::select`, which indexes the composition-anchored cached
  list, while `snapshot_candidates` had recorded that index against the
  `candidates_ending_at` window the before-cursor guess built. The two
  lists differ in general, so the committed text could be a row the caller
  never displayed. Measured on `fixtures/w3` (redb backend), `su3cl3` —
  `ni3'hao3` in the session's `'`-joined buffer — with `before(6)`:

  | `before(6)` row 1 | displayed | committed | cursor answered |
  |---|---|---|---|
  | before this fix | 好 | **`你'hao`** | **3** |
  | after this fix | 好 | 好 | 6 |

  The cached list's row 1 is 你 (the FIRST key's span, `consumed_bytes` 3);
  the before-cursor window's row 1 is 好 (the SECOND key's span, ending at
  the cursor). Choosing the displayed 好 committed 你 and left the raw
  `hao` tail in the buffer, and answered the first key's end as the new
  cursor.

  **A defect, not a divergence.** Upstream has no equivalent indirection:
  `zhuyin_choose_candidate` receives a `lookup_candidate_t *` carrying the
  row's own token, so no index is resolved against a second list and the
  mismatch is not expressible there. Per this policy's rule that anything
  outside the four exception classes is a defect to be reverted, the fix
  moves toward the pin and needs no class.

  **The fix.** The before-cursor window is retained in
  `InstanceCore::anchored_window` exactly as the after-cursor one is, so
  the later choose resolves through `Session::select_anchored` against the
  same list — in the zhuyin C ABI (`oxpinyin-zhuyin-capi`; a former Python
  `ZhuyinSession` mirror was retired with the binding). The
  anchor is `oxpinyin_facade::BEFORE_CURSOR_ANCHOR` = 0, not the lookup
  offset: `candidates_ending_at` scans the prefix graph `raw[..offset]`,
  whose coordinates are absolute from the buffer start, so every row's
  `consumed_bytes` is the span's absolute END rather than a length
  measured from a start. `select_inner` reads the chosen span as
  `[anchor, anchor + consumed_bytes)`, so 0 is the only anchor that
  reproduces the span end and the consumed advance; reusing the
  after-cursor anchor would read `[offset, 2 * offset)` and walk the
  composition off the end of the buffer.

  **Coverage.** A C-side test pins the property on the zhuyin ABI —
  `oxpinyin-zhuyin-capi`'s
  `choosing_from_a_before_cursor_window_uses_that_window`, the twin of
  `oxpinyin-capi`'s
  `choosing_from_a_reanchored_window_uses_the_anchored_span`. Revert-and-check:
  reverting the facade edit fails the C test at the cursor assertion
  (3 vs 6).

  **Residual — OPEN.** The fix makes the
  committed row the displayed row; it does not make the before-cursor
  choose pin-identical. For a row whose span STARTS after the composition
  offset, the chosen span is recorded as `[0, offset)` rather than
  `[start, offset)`, so the raw bytes before the span are absorbed into the
  chosen text instead of being decoded — `su3cl3` `before(6)` row 1 commits
  `好`, where upstream's constrain-and-re-decode model would keep a
  conversion for the leading key as well. Root cause:
  `oxpinyin_engine::Candidate` carries `consumed_bytes` but no span START,
  so a window whose rows have per-row begins cannot be resolved row by row;
  the pin's rows carry `m_begin` each (`pinyin.cpp:2227`, the `m_begin =
  start` already cited in `Session::select_anchored`'s doc comment) and its
  caller passes that begin as the choose offset, where this facade
  deliberately ignores its `offset` argument. Closing it needs `Candidate`
  to carry its span start plus an end-anchored `Session::select_*` — both
  changes to `oxpinyin-engine`'s supported surface, so AGENTS.md's STOP
  ("needs interface change") applies and the gap is registered rather than
  improvised. (The paragraph's pin-side claims were written without an
  oracle on the authoring host and are superseded by the measured record
  below.)

  (Amended 2026-09-05, **the residual measured against the pin** — the
  owed differential run, on the pin-built oracle
  (`/tmp/oxpinyin-zhuyin-oracle`, `oracle-pin.txt`: libpinyin 2.11.91 at
  `0c5e80e1`, model20 `59c68e89…`, Tkrzw, `shared_object_sha256`
  `5cb23f8b…`) against this branch (`ab36a43c`); the Rust systemdir was
  recompiled from the same SHA-verified model20 export with
  `oxpinyin-datagen compile --backend tkrzw` — the runtime's
  default-backend switch (`05688575`) had made the session's `.redb` dir
  stale. Pin source re-read at the pin (`git show 0c5e80e1:src/zhuyin.cpp`):
  `zhuyin_choose_candidate` for a `NORMAL_CANDIDATE_BEFORE_CURSOR` row
  does `constraints->add_constraint(candidate->m_begin,
  candidate->m_end, token)` and `offset = candidate->m_begin` — the
  `m_begin` law, now measured, not just cited. The driver gained the
  default-off `choose` battery for it (`tools/bisection/zhuyin-diff.c`:
  parse, `guess_sentence` — the consumer protocol — then
  `guess_before(consumed)`, choose row 1, the answered cursor,
  `get_sentence` after the choose and after the next re-guess, over
  `su3cl3`/`su3u3`/`su3u3u3`); the standing default battery is unchanged
  (byte-identical, 2307 lines both sides). Measured, row 1 of
  `before(consumed)` — the row is the same on both sides, windows
  identical (n 94/600/608, head rows equal):

  | input | row 1 (span, pin) | pin cursor | ox cursor | pin re-guess | ox re-guess |
  |---|---|---|---|---|---|
  | `su3cl3` | 好 `[3,6)` | **3** | 6 | 你好 | 你好 |
  | `su3u3` | 拟议 `[0,5)` | **0** | 5 | 拟议 | 拟议 |
  | `su3u3u3` | 意义 `[3,7)` | **3** | 7 | 你意义 | 你意义 |

  What the measurement settles, and what it corrects:
  - **The cursor divergence is universal, and it is the whole measured
    surface.** The pin answers the row's `m_begin` in every case —
    including a whole-composition row (拟议 `[0,5)` → 0), where this
    facade answers the composition end. This facade's law (the span END
    mapped to original coordinates) is measurable at every row: 6 vs 3,
    5 vs 0, 7 vs 3. Honest note: the fix above traded an accidental
    cursor agreement for the displayed-row property — pre-fix this
    facade answered 3 on `su3cl3` (= the pin, by accident of the
    wrongly-chosen 你's consumed length); post-fix it answers 6. The
    text side moved toward the pin (好, the row the caller actually
    saw); the cursor side moved from accidentally-right to wrong.
  - **The sentence-level claim is corrected.** The superseded
    paragraph's "the raw bytes before the span are absorbed into the
    chosen text instead of being decoded" does not show in the re-decoded
    1-best sentence: the pin's constrain-and-re-decode keeps 你好 AND so
    does this facade, on all three inputs. The `[0, offset)` constraint
    span is structurally real (the source law above), but no corpus
    input here flips the 1-best because of it; a sentence-level
    divergence would need an input where the constraint changes the
    decode, none found among the three.
  - **Classification, unchanged and now grounded.** The pin law is plain
    integer bookkeeping — reproducible in principle — so this is not one
    of the four exception classes; it is a gap the compatibility policy's
    corollary makes mandatory to close ("if implementing a symbol
    correctly requires an engine change, that engine change is
    mandatory"). The STOP stands (an `oxpinyin-engine` interface change
    needs an ask), so the entry stays OPEN, registered rather than
    improvised. Closure proof when the engine work lands: this battery
    must go byte-identical — cursor, sentence after the choose, and
    sentence after the re-guess across all three inputs, on both
    protocols.)


  (Amended 2026-09-06, **the residual CLOSED in code** — maintainer
  approval 2026-09-06 for the engine-interface change, with the
  instruction to copy what libpinyin's source does. `Candidate` now
  carries `span_start`, upstream's `m_begin`: 0 for a window measured
  from its anchor, the absolute span start for a before-cursor window
  (`zhuyin.cpp:1595`, `template_item.m_begin = start; m_end = offset`).
  `Session::select_inner` writes the constraint on
  `[anchor + span_start, end)` — `add_constraint(m_begin, m_end, token)`,
  `zhuyin.cpp:1652,1659` — so `su3cl3` `before(6)` row 1 forces `[3, 6)`
  (好) and leaves the leading key to the decode: the re-guess answers
  你好, the shape the 2026-09-05 measurement recorded on the pin. Two
  observable consequences, both toward the pin:
  1. `zhuyin_choose_candidate` answers `m_begin` for a before-cursor
     row (`offset = candidate->m_begin`, `zhuyin.cpp:1660`) — 3 here,
     where the facade answered the span's end (6). The after-cursor and
     BEST_MATCH returns are unchanged (`m_end`, `matrix.size() - 1`).
  2. A before-cursor choose after an earlier selection no longer
     fails: the old `[0, offset)` span regressed the composition offset
     and hit `SelectionAnchorBeforeComposition`; the row's own start
     does not.
  The engine's `commit()` — a Rust/Python surface with no ABI
  counterpart — keeps the leading key's raw bytes as a typed-but-
  unselected gap (`ni好`), the same law an after-cursor re-anchor
  already applied; the ABI surface commits `zhuyin_get_sentence`'s
  re-decode, which is 你好. Pinned by the engine test
  `choosing_a_before_cursor_row_constrains_its_own_span`, the C test
  `choosing_from_a_before_cursor_window_uses_that_window` (cursor 3,
  free cell at 0, forcing at 3, sentence 你好 after the re-guess) and the
  zhuyin parity corpus.)

  (Amended 2026-09-08, **the owed battery run — IDENTICAL.** In the
  perf-matrix container (`tools/bisection/Dockerfile.perf-matrix`,
  arm64), libzhuyin built at the current pin 074a2219 with
  `--with-dbm=Tkrzw --enable-libzhuyin` over the SHA-verified model20
  export (`libzhuyin.so.15.0.0` sha256 `2109e10c…`, 23 data files built
  by the pin's own `data/` step) against this tree's
  `libzhuyin_capi.so` (tkrzw, then the default) reading an
  `oxpinyin-datagen compile --backend tkrzw` systemdir from the same
  export. `tools/bisection/zhuyin-diff.c`: the standard battery is
  byte-identical (2307 lines a side) and the default-off `choose`
  battery is byte-identical (65 lines a side) — cursor 3 / 0 / 3 on
  `su3cl3` / `su3u3` / `su3u3u3`, `get_sentence` after the choose 你好 /
  你一 / 你意义 and after the re-guess 你好 / 拟议 / 你意义 on both
  sides, windows 94 / 600 / 608 with row 1 好 / 拟议 / 意义. The
  2026-09-05 table's three cursor cells (6/5/7 vs the pin's 3/0/3) are
  the ones #374 closed; nothing else in the battery moved. Build note
  for the next run: libpinyin's `configure` takes the DBM in its own
  spelling (`Tkrzw`, as `tools/oracle/build-oracle.sh` maps it) — the
  lower-case form is accepted silently and leaves every backend type
  undefined.)

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

## zhuyin n-best trellis constants: `PhoneticLookup<1, 1>` vs the engine's `<2, 3>` port — CLOSED in code (2026-09-06)

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

  (Amended 2026-09-06, **CLOSED in code** — maintainer approval
  2026-09-06. The pair is a per-session `oxpinyin_engine::NbestShape`
  — `PINYIN` = `<2, 3>` (`pinyin.cpp:55`), `ZHUYIN` = `<1, 1>`
  (`zhuyin.cpp:50`), `Default` = `PINYIN`, no other constructor so
  upstream's `nstore <= nbest` assert (`phonetic_lookup.h:715`) cannot
  be violated — threaded through `nbest_sentences` into the trellis's
  per-node cap and tail count, and set by both zhuyin facades at
  instance allocation beside the display law
  (`Session::set_nbest_shape(NbestShape::ZHUYIN)`). The fallback DP's
  row cap follows the shape too. Pinned by
  `the_zhuyin_shape_extracts_a_single_tail`: two readings of `ni` give
  the pinyin shape two rows and the zhuyin shape one, the same 1-best.
  Per-surface const generics were the recorded fix shape; a per-session
  value is the same law without changing the `Session` type every
  consumer names. Not observable through today's libzhuyin candidate
  surface, as the entry says; the pruning depth now matches the pin for
  whatever surface exposes it next.)

## zhuyin `FORCE_TONE` / `ZHUYIN_INCOMPLETE` default

- **Upstream source cite:** `src/zhuyin.cpp:273` (`context->m_options =
  USE_TONE | FORCE_TONE`; no `ZHUYIN_INCOMPLETE`).
- **Mechanism:** `zhuyin_init` seeds `USE_TONE | FORCE_TONE` and nothing
  else. `FORCE_TONE` is honoured by the chewing parser nested inside
  `USE_TONE` for the Simple / CP26 keyboards and unconditionally for
  Discrete (`zhuyin_parser2.cpp:178,373,387,602`); `ZHUYIN_INCOMPLETE` is
  OFF by default.
- **What oxpinyin does instead:** `CapiContext::try_open` seeds the same
  `USE_TONE | FORCE_TONE` word (`ZHUYIN_DEFAULT_OPTIONS`,
  `crates/oxpinyin-zhuyin-capi/src/state.rs`) and defaults `incomplete` to `false`
  (matching the pin). The FORCE_TONE law is delegated to
  `oxpinyin_core::ZhuyinParser::parse_with_options`, which honours it in the
  Simple/CP26 (nested) and Discrete (unconditional) shapes — the
  implementation matches the pin's three shapes.
- **Externally observable:** no, with the corrected default — the differential
  was run with the pin's default word (`USE_TONE | FORCE_TONE`, no
  `ZHUYIN_INCOMPLETE`) and the parse-length gap above is the only residual.
  Entry kept so a future consumer that sets the bit finds the law already
  analysed.

## pinyin-facade chewing batch seam does not forward `FORCE_TONE` — CLOSED (the whole word crosses the seam)

Surfaced by the bopomofo SPEC audit (2026-09-03). The FORCE_TONE entry
above enumerates the full-pinyin batch, the double one-key, the zhuyin
one-key, the libzhuyin batch and the double-pinyin batch seams; the pinyin
facade's chewing batch seam — `pinyin_parse_more_chewings`, the function the bopomofo SPEC
freezes — was never in that enumeration. This entry completes it.

- **Upstream source cite:** `src/pinyin.cpp:1582-1609` (at pin `0c5e80e1`)
  (`pinyin_parse_more_chewings` passes `context->m_options` with only
  `ZHUYIN_CORRECT_ALL` stripped, the strip at `:1589`);
  `src/storage/zhuyin_parser2.cpp:171-179` (Simple `parse_one_key`:
  `FORCE_TONE` nested under `USE_TONE`), `:373` and `:387` (Discrete),
  `:595-605` (CP26, nested); `pinyin_init` seeds `USE_TONE` alone
  (`src/pinyin.cpp:329`), so the bit is caller-set on this facade.
- **Mechanism:** the pin forwards the whole option word, so a
  `pinyin_context_t` consumer that sets `FORCE_TONE` gets the per-keyboard
  law — a toneless Simple/CP26 syllable and any Discrete key without a
  tone byte refuse, and the greedy walk stops there.
- **What oxpinyin does instead:** `parse_chewing_more`
  (`crates/oxpinyin-capi/src/parse.rs`) calls the three-argument
  `ZhuyinParser::parse(text, use_tone, allow_incomplete)`, so `FORCE_TONE`
  never crosses and the tone-less profile runs whatever the caller's word.
  The comment that justified forwarding one bit — "`ZHUYIN_INCOMPLETE` is
  the one caller bit the Simple parser consults", citing `pinyin.cpp:1621`
  — was false at the pin on both counts and was the reason the seam was
  built this way; corrected in dea0880 together with the SPEC paragraph it
  was copied into. The libzhuyin facade's batch seam forwards the whole
  word through `ZhuyinParser::parse_with_options` (1671954); the fix shape
  here is the same one-line forward of `inst.options().bits()` plus a
  `FORCE_TONE` profile in `tools/bisection/chewing-diff.c`, whose only
  option word today is `IS_ZHUYIN | ZHUYIN_INCOMPLETE | USE_TONE`
  (`chewing-diff.c:53`).
- **Externally observable:** yes, but only under a caller-set `FORCE_TONE`
  — not the pin's `pinyin_init` default and not the bopomofo differential's
  word, which is why every existing gate is IDENTICAL while the seam is
  unimplemented. Under `USE_TONE | FORCE_TONE` the pin consumes 0 on
  toneless `su` (STANDARD ㄋㄧ) where oxpinyin consumes 2. No class among
  (a)–(d) of `compatibility-policy.md` fits: this is a defect to close,
  registered so the bopomofo SPEC freeze can name it as its open
  implementation item rather than stay silent, exactly as the double-pinyin
  freeze carried its batch gate.

  (Amended 2026-09-14: **CLOSED in code.** The `PinFacade` arm of
  `parse_chewing_more` now forwards `self.options().bits() &
  !ZHUYIN_CORRECT_ALL` through `ZhuyinParser::parse_with_options` — the
  pin's exact `pinyin.cpp:1582-1609` law, re-verified against the pin
  checkout (`src/pinyin.cpp` blob `f27f7cf7` at `074a2219`). The
  capi-level tests (`chewing_batch_force_tone` in
  `crates/oxpinyin-capi/src/e2e_tests.rs`) pin the register's measured
  shape — toneless `su` refuses under `USE_TONE | FORCE_TONE`, `su3`
  parses, the strip changes nothing observable on STANDARD, and
  `FORCE_TONE` without `USE_TONE` stays nested-inert — and
  `tools/bisection/chewing-diff.c` gained the FORCE_TONE profile pass
  the entry prescribed. The live pin-vs-oxpinyin run of that profile was
  taken 2026-09-16 (amendment below); every existing gate stays
  IDENTICAL because none sets the bit, so until that run the closure
  rested on the unit-level law plus the profile, in the same shape the
  NbestShape closure (row 28) rested on engine tests without a moving
  gate.)

  (Amended 2026-09-16: **live differential RUN — IDENTICAL on all eight
  implemented keyboards.** `run-scheme-diff.sh bopomofo N` for N = 1–6,
  8, 9 (STANDARD, HSU, IBM, GINYIEH, ETEN, ETEN26, HSU_DVORAK,
  DACHEN_CP26; 7 STANDARD_DVORAK aborts the pin and is never sent) each
  exited 0 IDENTICAL with no `SKIP:` line and both sides visibly
  executed. Environment: a `debian:testing` container (image
  `docker.io/library/debian@sha256:5056ab8a99336d6d71390d640f72229649f12e3d38e987cf6b24dc8675325d73`,
  container `a15849ef7dd2`), the pin oracle prefix mounted read-only,
  both sides on tkrzw — the oracle is the `dbm=tkrzw` parity build and
  the default `cargo build -p oxpinyin-capi` then resolved the tkrzw backend (Berkeley DB since 2026-09-20) —
  with the capi on a P6-native data directory (libpinyin-native
  `.bin`/`.db` naming). The measurement tree was `main` @ `93a462f`
  overlayed with PR #476's `system-dir.sh` (the native-layout
  acceptance; these records change no runner or driver). The
  full both-side driver logs for `bopomofo 1` run 1512 lines each; the
  FORCE_TONE profile section (log line 889) carries the toneless `su`
  on STANDARD identically on both sides: `parse_chewing: consumed=0`,
  `parsed_input_length: 0`, `guess_sentence: false`,
  `n_candidates: 0`. Non-vacuity: reverting the seam alone in a
  throwaway worktree (`crates/oxpinyin-facade/src/parse.rs` and
  `crates/oxpinyin-core/src/scheme.rs` at their pre-`6616fb08` state,
  the driver's FORCE_TONE profile kept) turns `bopomofo 1` into exit 2
  with exactly the row-30 hunk — oracle `consumed=0`, reverted capi
  `consumed=2`, `n_candidates=125`. Logs:
  `~/.local/share/oxpinyin-evidence/2026-09-16/w1/`.)

### The pinyin index DBMs carry uninitialized struct padding

- **Upstream source cite:** `src/storage/chewing_large_table2.h:217-231`
  (`ChewingTableEntry::add_index` inserts a whole `PinyinIndexItem2<L>`
  struct with `m_chunk.insert_content(offset, &add_elem, sizeof(IndexItem))`)
  and `pinyin_phrase3.h:181-195` (`PinyinIndexItem2<L>` = `{u32 m_token,
  ChewingKey m_keys[L]}` — for odd L the struct carries 2 bytes of
  tail padding to the token's 4-byte alignment).
- **Mechanism:** `add_elem` is a stack-allocated `PinyinIndexItem2<L>`
  whose constructor `memmove`s only the keys and never touches the tail
  padding; the whole `sizeof` bytes — padding included — are copied into
  the DBM value. The padding bytes are whatever happened to sit on that
  stack region during `gen_binary_files` — unreproducible garbage
  (observed `17, 236` and `90, 237` in the pin's real files).
- **What oxpinyin does instead:** datagen zeroes the padding
  (`oxpinyin_data::row_format::pinyin_index::encode_item`, written from
  `crates/oxpinyin-data/src/table_entries.rs`), and the runtime reader
  (`pinyin_index::decode_items` via `ChewingTable::search`,
  `crates/oxpinyin-data/src/chewing_table.rs`) reads only the token and
  key fields within each stride, never the padding.
- **Externally observable:** no — the reader never touches the padding
  bytes, so a libpinyin runtime consuming either file decodes identical
  index items; only the file bytes differ, in two bytes per odd-L value
  record. Class (b): upstream's stored bytes *are* uninitialized memory;
  no safe construction reproduces specific garbage, and none should.


## redb write-side emptiness probe creates the table it probes (2026-09-05)

redb `WriteTxn::is_empty` (`RedbWriteTxn` in `crates/oxpinyin-store/src/lib.rs`)
creates the probed table if absent — redb API constraint (4.2.0: `open_table`
creates, `list_tables` is the only lookup), no non-creating write-side
existence probe exists; the store traits cannot tell an empty table from an
absent one, so nothing above them observes it and the only trace is an empty
table in the file.

## Entries registered 2026-09-27 UTC (lane G — round-2 audit reconciliation)

The entries below were registered by the round-2 register
reconciliation (#573; the audit report is
`docs/findings/bug-for-bug-audit-r2-2026-09-23.md`). Each carries its
`compatibility-policy.md` row number. Pin cites are at `074a2219`,
oxpinyin cites at `main` @ `e1d915d0`; both were re-read for this
section.

### `pinyin_init`/`zhuyin_init` leave the process `LC_NUMERIC` at "C" (policy row 39)

- **Upstream source cite:** `src/storage/table_info.cpp:328` and `:372`
  (`UserTableInfo::load`), `:197` and `:291` (`SystemTableInfo2::load`),
  `:378` and `:394` (`UserTableInfo::save`).
- **Mechanism:** each function saves `setlocale(LC_NUMERIC, "C")`'s
  return value and restores it at the end. `setlocale` returns the name
  of the *new* locale (C11 7.11.1.1p7), so the "restore" re-applies
  `"C"`; and every early return — a failed `fopen` (`:330-333`,
  `:199-202`), a failed version directive (`:339-348`) — skips the
  restore altogether. Either way the process's `LC_NUMERIC` is `"C"`
  after `pinyin_init` or `zhuyin_init`, successful or not.
- **What oxpinyin does instead:** since #618 (merge commit `a3ef00f5`,
  code `e5400598`, 2026-10-01 UTC) it installs `"C"` at the same
  points, through the C library's own `setlocale`, in the two C-ABI
  crates. Before that — the state this section was written against —
  there was no `setlocale` call anywhere in `crates/`, so the host's
  `LC_NUMERIC` survived init.
- **Externally observable:** yes — audit D-17 (#539): with
  `LC_ALL=zh_CN.UTF-8` set by the host, `setlocale(LC_NUMERIC, NULL)`
  reads `C` after the pin's init and read `zh_CN.UTF-8` after
  oxpinyin's, before #618. A consumer that formats numbers after init
  saw the difference; `tools/bisection/run-locale-diff.sh` now diffs it
  byte for byte.
- **Status:** ruled bug-for-bug (recorded 2026-09-27 UTC,
  `compatibility-policy.md` "Amendment — rulings recorded" item 4):
  oxpinyin is to reproduce the pin's end state, and the defect is
  drafted for the upstream report (`upstream-report-drafts.md` item 6,
  not filed). **CLOSED** in code: reproduced by #618 (merge commit
  `a3ef00f5`, code `e5400598`, merged 2026-10-01 UTC; recorded
  2026-10-02 UTC); it was a REVERT TARGET until then (#539).

### tkrzw binding: exception-origin `SYSTEM_ERROR` and OS `SYSTEM_ERROR` share one error class (policy row 40)

- **Registered as:** a standing divergence (ruled accepted 2026-08-28;
  a Rust/C++ language-boundary consequence under the language-quirk
  exception, maintainer ruling 2026-10-03), not a numbered exception
  class — `compatibility-policy.md`, "Registered standing divergences". Registered here 2026-09-27 UTC (#551); the full
  record, with the ruling's process note, is
  `docs/findings/tkrzw-langc-exception-classification.md`.
- **Upstream source cite:** none in libpinyin: the pin's tkrzw backend
  (`src/storage/*_tkrzwdb.cpp`) calls tkrzw's C++ API. The cite is
  tkrzw's `tkrzw_langc.cc` at 1.0.32 (every wrapper's
  `catch (const std::exception& e)` sets `TKRZW_STATUS_SYSTEM_ERROR`,
  e.g. `:163-165`, as the finding cites it).
- **Mechanism:** a C++ exception inside tkrzw (allocation failure) and
  an operating-system error both surface as `SYSTEM_ERROR` through the
  C API; the retired cxx shim reported the first as `UNKNOWN_ERROR`.
  Rust cannot catch a C++ exception, so the C API is the only way in,
  and it merges the two before oxpinyin sees either.
- **What oxpinyin does instead:** maps `SYSTEM_ERROR` to
  `StoreError::Io` and every other code to `StoreError::Backend`
  (`crates/oxpinyin-store/src/tkrzw/mod.rs:149`, `:214`), so the
  exception origin is indistinguishable from an I/O error.
- **Externally observable:** only to a Rust caller of `oxpinyin-store`
  branching on `StoreError::Io`, and only under memory exhaustion; no
  test or differential surface reaches it, and nothing crosses the
  libpinyin C ABI.

### Literal `0x0` option gating — CLOSED in code (policy row 17)

- **Upstream source cite:** `src/storage/phonetic_key_matrix.cpp:86-89`
  (`resplit_step` returns without `USE_RESPLIT_TABLE`), `:168-171`
  (`inner_split_step` without `USE_DIVIDED_TABLE`); `src/pinyin.cpp:2194`
  (`pinyin_guess_candidates` answers `false` on an empty matrix).
- **Mechanism:** at the literal option word `0x0` the correction
  aliases (`jv`, `zon`) produce no key and the divided/resplit tables
  add no alternatives, so the guess is empty and `xian`'s inventory
  shrinks.
- **What oxpinyin does:** the same gates (`crates/oxpinyin-engine/src/session/mod.rs:609`,
  `:626`), and no raw-text fallback when the parse consumed nothing
  (`8ec75085`).
- **Status:** closed; `run-option-sweep.sh` with the `0x0` and
  divided-contrast cases ran 2026-09-16, 24/24 PASS. Registered here
  2026-09-27 UTC (#548): until then the row's only record was the
  parked paragraph in `all-off-tails.md`.

### Sort-option input of `pinyin_guess_candidates` — CLOSED in code, reopened 2026-09-27 (#582) and closed 2026-10-03 (policy row 32)

- **Upstream source cite:** `src/pinyin.cpp:2292-2300` (the LONGER and
  sentence prepends gated on `sort_option`, then
  `_remove_duplicated_items_by_phrase_string`); `:1678-1709`
  (`compare_item_with_sort_option`); `:1870-1933`
  (`_prepend_longer_candidates`); `src/pinyin.h:56`
  (`SORT_WITHOUT_SENTENCE_CANDIDATE = 0x1`).
- **Mechanism:** with bit `0x1` set the pin never prepends the n-best
  rows, so its dedup never sees them and every NORMAL row survives.
- **What oxpinyin does instead:** bits `0x2`/`0x4`/`0x8`/`0x10` are
  ported (PR #496, 2026-09-20). Bit `0x1` is applied after an engine
  dedup that already ran with the n-best rows present
  (`crates/oxpinyin-engine/src/session/lookup.rs:754`, then
  `crates/oxpinyin-capi/src/sentence.rs:359`), so a NORMAL row whose
  text equals an n-best string is lost with the sentence row.
- **Externally observable:** yes (#582, all three cells): `li'shi`
  after `guess_sentence` at `0x1f` — pin n=385 headed by 历史, 理事;
  oxpinyin n=383, neither present; `0x1d` the same; `0x1c` identical.
  The 2026-09-20 closure measured `0x1e`/`0x1c`/`0x14` only.
- **Status:** **CLOSED** in code 2026-10-03: #596 (merge commit `45cc6032`) and
  #598 (`efa8337a`); every `0x1`/`0x1f` case of `candidate-assembly-diff` is
  identical on tkrzw, bdb and kc. Reopened 2026-09-27 UTC (#582).

### Whole-row NBEST choose + train writes the user bigram (policy row 33)

- **Upstream source cite:** `src/pinyin.cpp:2515-2520` (a row choose
  installs no `CONSTRAINT_ONESTEP`); `src/lookup/phonetic_lookup.h:866`
  (`train_result3` trains only past a ONESTEP constraint).
- **Mechanism:** after a whole-row n-best choose, `pinyin_train` walks a
  constraint-free result and writes nothing.
- **What oxpinyin does instead:** `Session::train`
  (`crates/oxpinyin-engine/src/session/selection.rs:298-339`) falls back
  to the selection record when no OneStep cell is present and seeds
  `sentence_start → phrase`.
- **Externally observable:** yes — audit D-05 (#527): after the union
  driver's choose and train, oxpinyin writes `测测→你` (count 138) and
  predicts 你; the pin writes and predicts nothing. Row 20's line is
  this mechanism (see row 20).
- **Status:** **CLOSED** in code 2026-10-03: #614 (merge commit `33e2e54d`, code
  `750b8cb6`) and #616 (`fd293e53`); the choose-every-row differential is
  identical on tkrzw, bdb and kc. Work order `revert-plan.md`.

### Imported user phrase lost after `guess_sentence` (policy row 34)

- **Upstream source cite:** `src/pinyin.cpp:2693-2704` (`pinyin_reset`
  is the only clear of `m_nbest_results`); `:2184-2300`
  (`pinyin_guess_candidates` rebuilds from scratch).
- **Mechanism:** the pin keeps n-best results across a parse and, under
  `0x1`, never lets them into the dedup.
- **What oxpinyin does instead:** clears n-best on every parse and
  dedups NBEST-first before the `0x1` filter (row 32's mechanism).
- **Externally observable:** yes — an imported user phrase is offered at
  `0x1f` after a sentence guess on the pin and not on oxpinyin
  (`probe-coverage-abi.md` C); #582 shows the dedup half without any
  import.
- **Status:** **CLOSED** in code 2026-10-03: #598 (merge commit `efa8337a`) with
  #596 (`45cc6032`); phases A and N at `0x1`/`0x1f` are identical on
  tkrzw, bdb and kc.

### User-library tokens refused an n-best step cost — CLOSED in code (policy row 35)

- **Upstream source cite:** `src/lookup/phonetic_lookup.h:643-668`
  (`unigram_gen_next_step` prices any loaded sub-index's item);
  `src/pinyin.cpp:597-605` (an import writes the item with
  `count × unigram_factor`).
- **Mechanism:** a user-dictionary token is a priced trellis step.
- **What oxpinyin does:** since `7c9a6923` (2026-09-20),
  `nbest_step_costs_with_user_delta` prices a visible `USER_FILE` token
  with no system unigram from its user delta
  (`crates/oxpinyin-data/src/lm/mod.rs:570-576`); a masked library's
  token and a missing item keep the default, as the pin's failing
  `get_phrase_item` does.
- **Status:** closed; `7c9a6923`'s same-dir measurement on the pin's
  `data/` (tkrzw): `residue-a-tail-diff` phases A and X byte-identical.
  #548 found the policy still counting it open; corrected 2026-09-27
  UTC.

### Bigram export iterator: `get_next_phrase`'s return value on the last row (policy row 36)

- **Upstream source cite:** `src/pinyin.cpp:894-911`
  (`pinyin_bigram_iterator_get_next_phrase` returns
  `pinyin_bigram_iterator_has_next_phrase(iter)`).
- **Mechanism:** the pin answers `false` on the last row.
- **What oxpinyin did instead (before lane B):** answered `true`
  whenever a row was fetched (`crates/oxpinyin-capi/src/iterators.rs:406-407`
  at `e1d915d0`); #607 returns `has_next` after the increment.
- **Externally observable:** yes — audit D-19 (#541):
  `你好|ni'hao|138|false` on the pin, `…|true` on oxpinyin. A debug
  ibus-libpinyin build wraps the call in `check_result`
  (`PYLibPinyin.cc:321`) and aborts on the pin's last row.
- **Status:** **CLOSED** in code (recorded 2026-10-02 UTC at `a3ef00f5`):
  lane B's #607 (merge commit `6c9bc75d`, code `3415b232`) answers
  `has_next` after the increment, and #608 (merge commit `569420c1`, code
  `d0d849af`) exports from the pin's own in-memory user-bigram container —
  its walk order, its skipped last key and its `sentence_start`
  attribution (#541). Both merged 2026-10-01 UTC; measured by those PRs'
  `bigram-export-diff` differential.

### Candidate window behind the composition offset after a choose (policy row 37)

- **Upstream source cite:** `src/pinyin.cpp:2184-2262`
  (`pinyin_guess_candidates` rebuilds from `offset` over the whole
  matrix); `:2501-2590` (a choose writes a constraint and answers a
  cursor; the instance keeps no composition offset).
- **What oxpinyin does instead:** advances a composition offset on every
  choose (`crates/oxpinyin-engine/src/session/selection.rs`) and serves
  its cached list for any lookup offset at or behind it
  (`crates/oxpinyin-capi/src/sentence.rs`, the re-anchor test).
- **Externally observable:** yes — `probe-coverage-abi.md` E: at
  `guess_candidates(0, 0x1f)` after a whole-composition choose the pin
  answers 127 candidates and oxpinyin 0; ibus-libpinyin's preset 2
  takes that path after every partial choose.
- **Status:** **CLOSED** in code 2026-10-03: #604 (merge commit `06ede4dc`), #609
  (`563ed9b1`) and, for the transformed input schemes, #625 (`c6a61be8`);
  phases C, M, S, T and K are identical on tkrzw, bdb and kc.

### `pinyin_train`/`zhuyin_train` gate (policy row 38)

Updated 2026-10-04 UTC for #524. Valid-index and both empty-result defects
closed in code; the approved bounds class (c) case is measured on all three cells.

The pin tree read and built is `074a2219c90feaf962d0d24f034514033ece5f99`.
`src/pinyin.cpp:2678-2688` refuses empty decoded results, asserts the index
bound, marks the context modified and trains the requested result.
`src/zhuyin.cpp:1704-1713` has the same empty gate and trains row zero.
Selection history alone is insufficient on both facades; the previous
claim that these refuse arms were closed was wrong.

The fix adds `Session::train_nbest(index, user)` using each existing row's
spans (`crates/oxpinyin-engine/src/session/selection.rs:361-376`), retaining
the pin's OneStep/train-next observation gate in `train_spans` (:379-424).
`InstanceCore::train(index)` returns `Result<bool, EngineError>` and leaves
empty and bounds calls unmodified (`crates/oxpinyin-facade/src/instance.rs:245-259`).
The pinyin facade maps bounds to false and exactly one `log_warning`
(`crates/oxpinyin-capi/src/candidates.rs:600-606`), domain `libpinyin`,
warning level 16: approved availability class (c), replacing the pin assert.
Empty results return false quietly, including zhuyin
(`crates/oxpinyin-zhuyin-capi/src/candidates.rs:272`).

Measured on linux/amd64, debian:testing digest
`sha256:16faa8d1cd99fcb2d30eebe90454e26b20499055fa7094e9b55d32d1a7666f08`:
`python3 tools/bisection/train-index-diff.py bdb PREFIX PINYIN_SO ZHUYIN_SO`
passes all 10 cases on bdb; replacing `bdb` with `kc` or `tkrzw` also
passes all 10 cases (30 total). Identical pre-training pinyin rows are 今天/今田/今添;
zhuyin row zero is 你好. Valid calls match returns, native exported records
and complete saved `.dbin` bytes. Pinyin rows 0/1/2 train 今 followed by
天/田/添 respectively, each credited bigram +69, each unigram +483,
each pronunciation +69. Bounds len=3 and 255 isolate pin SIGABRT and
observe subject false, zero training writes and exactly one warning.
Both facades' fresh and parse/choose-without-guess cases are quiet false.
The same command with `--expect-parent` against parent `1dc42d7e`
(the shipped training sources are unchanged from `215365d3`) reproduces
row-1/2 D0 writes, both selection-only training defects, and bounds
true/write/no-warning. §12 passes 491/396/390 on all three cells.

Bdb and tkrzw raw user index-container bytes differ after valid saves while exported
records match; no register row covers that container-byte difference.
Row 18 covers padding in index values, a different issue. No container-byte
fix is included. Kc raw index-container bytes also match. Captures are
not committed; temporary profiles are discarded by the driver.

### An unknown `database format:` in `user.conf` aborts the pin; oxpinyin refuses the open (policy row 44)

Registered 2026-09-27 UTC from the "Register impact" of #591 (merged
into `main` 2026-09-26 with the #578–#591 stack; the fix is
`e5c1f3ad`), row text as #591 wrote it, cites re-read at `074a2219`
and at `main`.

- **Upstream source cite:** `src/storage/table_info.cpp:122-133`
  (`to_table_database_format_type`; the fall-through `abort()` at
  `:132`), called at `:353-354` from `UserTableInfo::load`
  (`:351-352`: `char str[256]` and
  `fscanf(input, "database format:%255s\n", str)`), which
  `check_format` calls at `pinyin.cpp:172-178` and `zhuyin.cpp:126-132`
  (`pinyin_init`/`zhuyin_init` then ignore its result and carry on —
  `pinyin.cpp:344`, `zhuyin.cpp:288` — so the `abort()` is what
  actually stops them).
- **Condition:** the third `fscanf` converts a token (it returns `1`)
  that is not `BerkeleyDB`, `KyotoCabinet` or `Tkrzw` — an
  unrecognised token (a foreign or future backend's, a 255-byte
  truncation of one, an empty field whose `%255s` reads the next word
  across the newline): the source guarantees the `abort()` there. When
  the directive's literal fails to match (a missing `database format:`
  line, or junk the previous line left behind) `fscanf` returns `0`, not
  `EOF`, and writes no `str`, so `strcmp` reads an indeterminate stack
  buffer — undefined behaviour, not a source-guaranteed abort. For those
  two shapes the evidence is execution only: the runner's
  `abort-modelver-junk` and `abort-no-dbformat` cases, oracle exit 134 on
  every cell in #591's runs.
- **Trigger:** an edited, foreign or torn `user.conf`.
  `UserTableInfo::save` is `fopen`/`fprintf`/`fclose` with no atomic
  rename (`table_info.cpp:377-395`), so a write interrupted mid-file is
  reachable in the field. Executed on both facades and all three cells:
  24 runs, every one SIGABRT (exit 134), user dir untouched.
- **What oxpinyin does instead:** `UserTableInfo::parse` answers
  `Err(UserConfError::UnknownDatabaseFormat)` on both sub-cases,
  `persistence::load` propagates it before any conformance judgement
  (`crates/oxpinyin-user/src/persistence.rs:66-72`), `Runtime::open`'s
  user-store step turns it into `OpenError::UnknownDatabaseFormat`, and
  `pinyin_init`/`zhuyin_init` answer NULL with nothing cleaned and no
  marker written — upstream never reaches its own wipe either. Stated
  for the record: the *previous* oxpinyin behaviour (unknown token →
  wipe) was itself a divergence, and this row replaces it.
- **Log line:** `check_format: unknown database format in user.conf`,
  exactly one per attempt, through GLib at warning level
  (`oxpinyin_facade::UNKNOWN_DATABASE_FORMAT_WARNING`,
  `crates/oxpinyin-facade/src/context.rs:49-50`; domain `libpinyin`
  from the pinyin facade, `crates/oxpinyin-capi/src/context.rs:25`,
  `libzhuyin` from the zhuyin one,
  `crates/oxpinyin-zhuyin-capi/src/context.rs:48` — the pin sets no
  `G_LOG_DOMAIN`, so its own `g_warning` sites print domain-less, and
  the shared message text is the fixed string above).
- **Externally observable:** yes, and it is a difference in kind — the
  pin takes the process down, oxpinyin fails the open. No pinned
  differential is possible at the abort point (the register's existing
  abort rows), so `tools/bisection/run-open-counter-diff.sh`'s
  `abort-*` expectation channel asserts the pair instead.
- **Class:** (c), both halves met — the first class-(c) row that is.
  The `table.conf` route to the same `abort()` (`table_info.cpp:232-233`)
  is covered since 2026-10-10 UTC (policy row 79).

### `pinyin_choose_candidate` under `SORT_WITHOUT_SENTENCE_CANDIDATE` with a nonzero offset aborts the pin (policy row 45)

Registered 2026-10-03 UTC from #598's "Register impact" (merge commit
`efa8337a`), row text as #598 wrote it, re-executed at `074a2219` on
freshly built tkrzw, bdb and kc oracles.

- **Upstream source cite:** `src/pinyin.cpp:2565-2576` — the `0x1` leg of
  `pinyin_choose_candidate`, whose first statement is
  `assert(0 == offset)` (`:2566`), then the unigram-only train.
- **Condition:** `m_sort_option & SORT_WITHOUT_SENTENCE_CANDIDATE` (set by
  the last `pinyin_guess_candidates` word) and `offset != 0`. ibus-libpinyin
  1.16.5 always passes 0 on that path (`PYPLibPinyinCandidates.cc:139-145`).
- **Executed:** `pinyin_parse_more_full_pinyins("nihao")`,
  `pinyin_guess_candidates(0, 0x1)`, choose the first NORMAL row at offset 1:
  `Assertion '0 == offset' failed`, SIGABRT (exit 134) on all three cells;
  at offset 0 the call answers `1`.
- **What oxpinyin does instead:** answers `0` and logs exactly one
  `g_warning` in the `libpinyin` domain, `pinyin_choose_candidate: offset
  must be 0 under SORT_WITHOUT_SENTENCE_CANDIDATE`
  (`crates/oxpinyin-capi/src/candidates.rs:362-371`).
- **Class:** (c), both halves met; the site is an **`assert`**.

### An empty-string user dir is the working directory on the pin (policy row 46)

Registered 2026-10-04 UTC with its fix (#619). Pin cites are at
`074a2219`; the measurements were taken on freshly built bdb, kc and
tkrzw oracles (linux/amd64) against `main` @ `8cd06566`.

- **Upstream source cite:** `src/pinyin.cpp:332` and `src/zhuyin.cpp:276`
  (`m_user_dir = g_strdup(userdir)`); `pinyin.cpp:1133` and `:2671`,
  `zhuyin.cpp:548` and `:1697` (the only guards, both on the pointer);
  `pinyin.cpp:176-177`, `:220-232`, `:922-1130` and `zhuyin.cpp:130-131`,
  `:164-176`, `:547-693` (every user file's path is
  `g_build_filename(m_user_dir, <name>, NULL)`, built when the file is
  touched).
- **Mechanism:** `g_build_filename` drops an empty element and stops at
  a NULL one. With `userdir ""` every path is a bare file name, resolved
  against the directory that is current at that file operation — not the
  one current at init — and `m_user_dir` is non-NULL, so the context
  trains and a dirty save writes the profile. With a NULL `userdir` the
  same calls build the empty string: train and save answer `false` and
  nothing is written. The behaviour is defined, has no float in it and
  aborts nothing, so no exception class fits.
- **What oxpinyin does instead:** since #619's fix it reproduces it.
  `pinyin_init`/`zhuyin_init` keep NULL and `""` apart
  (`crates/oxpinyin-capi/src/context.rs`,
  `crates/oxpinyin-zhuyin-capi/src/context.rs`), `ContextCore::try_open`
  carries the difference as `Option<&str>`, and `Runtime::open_with_law`
  opens the store on the empty path and keeps it relative, so each file
  resolves against the directory current when it is opened. Before that
  — the state #619 measured — the runtime dropped an empty path
  (`crates/oxpinyin-runtime/src/lib.rs:1207` at `8cd06566`) after the
  two inits had turned NULL into `""`, so both arguments meant "no user
  dir".
- **Externally observable:** yes — #619. In an empty working directory
  the pin's `pinyin_init(data, "")` writes `user.conf` there, a train
  answers `true`, and the save answers `true`, writes the 11-file
  profile into the working directory and leaves `LC_NUMERIC` at `"C"`
  (row 39's site); a second process reads the training back (the trained
  word's unigram frequency 854 → 1337). `zhuyin_init(data, "")` is the
  same from the train on. Before the fix oxpinyin answered `false` to
  the train and the save, wrote nothing and left `LC_NUMERIC` alone at
  the save. With a chdir after init the pin's save and fini write into
  the new directory and the first keeps the raised open counter.
  `tools/bisection/run-locale-diff.sh` — row 39's differential,
  extended — diffs the two libraries on five forms of the argument — an
  absolute directory, `""`, `""` with a chdir after init, NULL and `"."`
  — two consecutive processes each, byte for byte on every step's
  return value, `LC_NUMERIC`, each directory's inventory and
  `user.conf`, and the trained word's unigram frequency; stderr is
  captured and not compared (#545).
- **Status:** **CLOSED** in code (#619); it was never a registered
  divergence, only an unregistered defect. Two neighbours are separate
  issues and are not reproduced here: a NULL user dir still has
  in-memory user tables on the pin, so an import succeeds there (#642),
  and an empty-string *system* dir is the working directory too (#643).

### Abort sites answered without a log: the #525 site ledger (policy rows 4, 5a, 5c, 5d, 6, 10, 14, 19, 21, 22)

- **Source:** the 87-row per-site table in #525's body (the round-2
  audit's D-03 code-basis pass, pin `074a2219`, subject `34a66bc9`),
  re-grouped here by what oxpinyin owes. Every site is live at the
  reference build: no `-DNDEBUG`, and `check_result`
  (`include/pinyin_utils.h:27-31`) is an `assert` there. Kind is the
  pin's construct — **`assert`** or **`abort()`**. No site's answering
  line logs (#525: `subject_site_log = False` for all 87); the only
  `g_warning` calls in the shipped crates are the init-failure lines
  (`crates/oxpinyin-capi/src/context.rs:25-29`,
  `crates/oxpinyin-zhuyin-capi/src/context.rs:48-50`).
- **Status:** every row below is **pending logging (lane C, #525)**;
  group B additionally **needs a guard, not just a log**.
- **Logged since (lane C, 2026-10-04; each answers `false`/`0` and emits
  exactly one `libpinyin` warning, held by an `abort-*` case of
  `contract-diff.py` that shows the pin dying of SIGABRT):**
  - PR 12a: `pinyin.cpp:457`, `:466`, `:499`, `:709`, `:1189`;
    `storage/pinyin_parser2.cpp:398`, `:611`; `storage/zhuyin_parser2.cpp:295`
    (through `pinyin_set_zhuyin_scheme`).
  - PR 12b: `pinyin.cpp:1474`, `:3035`, `:3057`, `:3067`, `:3092`, `:3203`,
    `:3204`, `:3488` (`:3147` and `:3161` take the same logging path, no
    trigger executed); `storage/phonetic_key_matrix.h:103`
    (through `pinyin_get_pinyin_key` and `_key_rest` on an empty matrix);
    `storage/pinyin_parser2.cpp:170`; `pinyin.cpp:3311` (group B), reached
    by a leading `'` (`'nihao`, `''ni`): column 0 stays empty and all three
    aux-text functions assert on every cursor (review of #652).
  - PR 12c: `pinyin.cpp:2507`, `:2593`, `:2883`, `:3734`, `:3738`
    (`pinyin_choose_candidate`, `pinyin_choose_predicted_candidate`,
    `pinyin_get_candidate_nbest_index`, `pinyin_remove_user_candidate`;
    policy row 64).
  - PR 12d: `lookup/phonetic_lookup.h:868` through `pinyin_train` and
    `zhuyin_train` (policy row 6).
  - PR 12e: `storage/pinyin_phrase3.h:152` (through the lookups that
    search the pinyin table), `pinyin.cpp:2769`.
  - PR 12f: `zhuyin.cpp:372`, `:381`, `:736`, `:1261`, `:1453` (`_check_offset`,
    also one past the reserved slot), `:2158`; `storage/zhuyin_parser2.cpp:295`;
    `storage/pinyin_parser2.cpp:398` through `zhuyin_set_full_pinyin_scheme`;
    `phonetic_key_matrix.h:103` through `zhuyin_get_zhuyin_key` and `_key_rest`;
    `storage/pinyin_parser2.cpp:170` through `zhuyin_parse_full_pinyin`;
    `zhuyin.cpp:2110` through `zhuyin_get_character_offset`. The zhuyin ABI
    logs in its own domain, `libzhuyin`. `zhuyin.cpp:440` and `:457` are
    file-triggered and wait for their own ruling (`:454` is refuted).
  - #694 (2026-10-10 UTC, policy rows 78–80): `storage/table_info.cpp:119`,
    `:132` (the `table.conf` half), `:142`, `:156`, `:175`, `:276`;
    `pinyin.cpp:388`, `:491`, `:457` and `zhuyin.cpp:330`, `:372` driven by the
    rows; `include/memory_chunk.h:493` and `:434` behind a `NULL` file name.
  - PR 12g (batch C, 2026-10-10): `pinyin.cpp:3743`, `:3750`, `:3759`
    (`pinyin_remove_user_candidate`'s three removal asserts; policy row
    78). `:3750` leaves group C, which had called it not applicable to
    the value store; `:3743` and `:3759` leave groups A and B.

| group | sites at `074a2219` (kind) | what oxpinyin answers | owed |
|---|---|---|---|
| A — refuses silently (33) | `include/memory_chunk.h:390` (`assert`, via `check_result`), `:493`, `:543`, `:547` (`assert`); `pinyin.cpp:457`, `:466`, `:499`, `:709`, `:1474`, `:3035`, `:3067`, `:3092`, `:3147`, `:3203`, `:3204`, `:3738` (`assert`), `:1189`, `:3488` (`abort()`); `storage/ngram_bdb.cpp:199`, `ngram_kyotodb.cpp:173`, `ngram_tkrzwdb.cpp:150` (`assert`); `storage/phonetic_key_matrix.h:103` (`assert`); `storage/pinyin_parser2.cpp:170` (`assert`), `:398`, `:611` (`abort()`); `storage/zhuyin_parser2.cpp:295` (`abort()`); `zhuyin.cpp:372`, `:381`, `:440` (trigger not established), `:457`, `:2110`, `:2158` (`assert`), `:736` (`abort()`) | `false`, `Err`, NULL, a skipped library or a dropped row — the false/`Err` half holds | one `g_warning` line per site |
| B — no check at all (32) | `lookup/phonetic_lookup.h:868` (`assert`, row 6); `pinyin.cpp:388`, `:491`, `:902`, `:2507`, `:2566`, `:2593`, `:2684`, `:2769`, `:2883`, `:3311`, `:3734` (`assert`); `storage/chewing_large_table2_bdb.cpp:282`, `:529`, `_kyotodb.cpp:269`, `:499`, `_tkrzwdb.cpp:252`, `:466` (`abort()`); `storage/phonetic_key_matrix.cpp:661`, `:663` (`assert`); `storage/phrase_large_table3.h:95` (`assert`); `storage/pinyin_phrase3.h:152` (`assert`, row 4); `storage/ngram.cpp:70` (`assert`); `zhuyin.cpp:330`, `:1261`, `:1453` (arm (c) of #525's trigger; arms (a)/(b) refuse) (`assert`); `storage/table_info.cpp:119`, `:142`, `:156`, `:175` (`abort()`), `:276` (`assert`), and `:132`'s `table.conf` half (`abort()`; its `user.conf` half is policy row 44, both halves met) | `true`, data, a store write, or an ignored `table.conf` column — e.g. `pinyin_choose_candidate` has no candidate-type check for a predicted row (`:2507`, `crates/oxpinyin-capi/src/candidates.rs:304-409`) and `pinyin_remove_user_candidate` none for a non-`NORMAL` row (`:3734`, `candidates.rs:225-253`) | a guard answering `false`/`Err`, then the log line |
| C — not applicable (3) | `pinyin.cpp:554`, `:568`, `:571` (`assert`) | no counterpart check can exist — see the note below | nothing |
| refuted (8) | `include/memory_chunk.h:434`; `storage/bdb_utils.h:61`, `:67`; `storage/phrase_index_logger.h:245`; `storage/tkrzwdb_utils.h:65`, `:72`; `zhuyin.cpp:454`; `pinyin.cpp:2517` (`check_result`) — all `assert` | the executed trigger did not abort the pin | nothing |
| not executed (5) | `include/memory_chunk.h:438`, `:497` (`assert`); `storage/chewing_large_table2_bdb.cpp:388`, `_kyotodb.cpp:367`, `_tkrzwdb.cpp:342` (`abort()`) | no trigger reached the site | a trigger |
| unverified (3) | `storage/phrase_index.cpp:745`, `storage/phrase_index_logger.h:202`, `pinyin.cpp:1859` (`assert`) | the subject side was not traced (#525) | a trace |

Totals 33 + 32 + 3 + 8 + 5 + 3 = 84; kinds 64 `assert` + 20 `abort()`
(three `assert`s moved to the logged list, above), out of #525's original
87-site table.

**Note — the not-applicable sites (different storage model).** Each of
the three asserts an invariant of upstream's text-searched phrase index
that oxpinyin's user store makes structural, so no check has anything to
test:

- `pinyin.cpp:554` (`PHRASE_INDEX_LIBRARY_INDEX(token) != index`) — the
  pin searches every library by text and asserts at most one hit per
  sub-index. oxpinyin keys a user phrase by (library, text) in
  `PHRASE_BY_LIB_TEXT` (`crates/oxpinyin-user/src/store.rs:744-745`), so
  a second token for one (library, text) pair cannot be stored.
- `pinyin.cpp:568`, `:571` (the found item's length and text equal the
  input) — the pin reads the hit back and compares; oxpinyin looks the
  token up *by* that text (`store.rs:744-757`) and adds a pronunciation
  to it (`:760-768`), never reading an item back to compare.

`pinyin.cpp:3750` was listed here as a fourth not-applicable site; it was
reproduced and is now logged (PR 12g, policy row 78), with the phrase
table loaded from `user_phrase_index.bin` as the membership set the
removal checks.

### NULL pointer arguments: the pin dereferences, oxpinyin null-guards (policy row 41)

- **Upstream source cite:** 68 `pinyin_*` exports, each dereferencing a
  NULL argument before any check at `074a2219`; the per-export first
  dereference is tabulated in #526's body, by pattern: the instance
  first (`src/pinyin.cpp:1312`); the context or iterator first (`:509`,
  `:665`, `:777`, `:1196`); an out-parameter written before any check
  (`:2847`, `:2876`, `:2982`); a NULL candidate inside a live `assert`
  (`:2507`, `:2593`); a NULL key or key-rest (`:2711`, `:2722`,
  `:2733`, `:2744`).
- **Mechanism:** no export checks its pointer arguments; a NULL one is
  dereferenced — undefined behaviour, SIGSEGV in practice.
- **What oxpinyin does instead:** every export (or the helper it
  delegates to) opens with an `is_null()` guard and answers
  `false`/`0`/NULL/void; #526 lists the guard line per export (e.g.
  `crates/oxpinyin-capi/src/instance.rs:19`, `iterators.rs:63`,
  `candidates.rs:309`, `cursor.rs:241`, `keys.rs:219`).
- **Externally observable:** yes — audit D-04: of 83 NULL-class probes,
  70 crash the pin (68 distinct exports, all `signal=11`) and all 83
  return on oxpinyin; the libzhuyin exports show the same shape (#526,
  comment of 2026-09-25). Class (b).

### A guess on an instance whose context was finalised (policy row 42)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`
  deletes the context and its members, not its instances);
  `:1372-1380` (`pinyin_guess_sentence` reads
  `instance->m_context->m_pinyin_lookup`).
- **Mechanism:** a use after free — the instance keeps a pointer to the
  deleted context.
- **What oxpinyin does instead:** each instance holds its own handles to
  the shared engine state (`crates/oxpinyin-capi/src/state.rs:69-77`),
  so the guess completes and answers `true`.
- **Externally observable:** yes — audit D-06, probe
  `instance_outlives_context`: SIGSEGV on the pin, `true` on oxpinyin.
  Class (b). The inverse probe (`alloc_after_fini`) is the next entry
  (policy row 60). The orphan guess itself needs no change (ruled
  2026-10-04): the pin's use after free stays unreproducible.

### Zhuyin import into library index 16 (policy row 43)

- **Upstream source cite:** `src/zhuyin.cpp:392-398`
  (`zhuyin_begin_add_phrases` stores any index), `:475` (`_add_phrase`
  calls `phrase_index->get_range(index, …)`),
  `src/storage/phrase_index.cpp:611` (`m_sub_phrase_indices[phrase_index]`
  with no bound), `src/storage/phrase_index.h:441` (the array has
  `PHRASE_INDEX_LIBRARY_COUNT` = 16 entries).
- **Mechanism:** index 16 reads one element past the array and calls
  through the value found there — an out-of-bounds read.
- **What oxpinyin does instead:** refuses every library that is not a
  user file (`crates/oxpinyin-zhuyin-capi/src/iterators.rs:81`) and
  answers `false`.
- **Externally observable:** yes — the audit's libzhuyin battery
  (#526, comment of 2026-09-25): SIGSEGV on the pin on all three cells,
  `false` on oxpinyin. Class (b). The pinyin facade's `_add_phrase`
  carries the same unchecked index (`src/pinyin.cpp:589`), but the
  audit recorded library 255 *accepted* there (D-12), so that side is
  not a stable crash and is not part of this entry.

### `pinyin_token_get_nth_pronunciation` past the last reading (policy row 51)

- **Upstream source cite:** `src/pinyin.cpp:2801-2819`
  (`ChewingKey buffer[MAX_PHRASE_LENGTH]` at `:2808`, never initialised;
  the return value of `item.get_nth_pronunciation(nth, buffer, freq)` at
  `:2815` ignored; `g_array_append_vals(keys, buffer, len)` at `:2817`);
  `src/storage/phrase_index.cpp:33-44` (`get_nth_pronunciation` refuses an
  out-of-range read through `MemoryChunk::get_content` and writes nothing).
- **Mechanism:** an `nth` past the last reading leaves `buffer` as it was,
  and the function appends `phrase_length` keys of it and answers `true`:
  an uninitialised read. What the stack held depends on earlier calls
  (bdb, 2026-10-04: `0000 0000` for `nth` 1 and 2, `604f0000` for
  `nth` 2^32−1).
- **What oxpinyin does instead:** answers `true` and appends
  `phrase_length` zeroed keys (`crates/oxpinyin-capi/src/dict.rs`,
  `pinyin_token_get_nth_pronunciation`). The return value and the number of
  keys are the pin's.
- **Externally observable:** the content only. Class (b).

### `zhuyin_token_get_nth_pronunciation` past the last reading (policy row 70)

- **Upstream source cite:** `src/zhuyin.cpp:1793-1811`
  (`ChewingKey buffer[MAX_PHRASE_LENGTH]` at `:1800`, never initialised;
  the return value of `item.get_nth_pronunciation(nth, buffer, freq)` at
  `:1807` ignored; `g_array_append_vals(keys, buffer, len)` at `:1809`);
  `src/storage/phrase_index.cpp:33-44`, the same refusing read as the
  pinyin entry above.
- **Mechanism:** as the pinyin entry: an `nth` past the last reading leaves
  `buffer` as it was, and the function appends `phrase_length` keys of it
  and answers `true`. bdb, 2026-10-04: `nth` 1, 2 and 2^32−1 on `你好`
  answer `true` with two keys.
- **What oxpinyin does instead:** answers `true` and appends
  `phrase_length` zeroed keys (`crates/oxpinyin-zhuyin-capi/src/dict.rs`).
- **Externally observable:** the content only. Class (b).

### A save whose write fails for one file (policy rows 68 and 69)

- **Upstream source cite:** `src/pinyin.cpp:940-1147` (`_write_files`
  ignores each file's write result; `_rename_files` renames every file of
  the set and prints `rename %s to %s failed.` for the ones that fail;
  `pinyin_save` answers `_write_files(...) && _rename_files(...)`, `true`),
  `src/storage/table_info.cpp:377-397` (`write %s failed.` for the marker),
  `src/zhuyin.cpp` for the twin.
- **Mechanism:** the files are written and renamed one by one. Blocking
  `user_pinyin_index.bin.tmp` leaves the first `user_pinyin_index.bin` at
  its active final path while the other files take the second save. In the
  final-blocked fixture, the harness first moves `user_phrase_index.bin`
  to `user_phrase_index.bin.moved`; that backup keeps the first save, the
  active final path is a blocking directory, and the other files take the
  second save. Both print one failure line and answer `true`: distinct
  mixed profiles.
- **What oxpinyin does:** the same, since lane C PR 15
  (`persistence::save_with_bigram_reporting`). It had removed what it
  staged and reported all ten renames, leaving the previous profile whole;
  that was a REVERT TARGET (row 69) for a day and is reverted.
  `UserStore::save()` keeps its all-or-nothing commit; only the reporting
  save, which the C `save` entry points use, follows the pin.
- **Externally observable:** yes — the stderr lines, the files left in the
  user dir. Closed: `stderr-save-one-tmp-blocked`,
  `stderr-save-one-final-blocked`, `stderr-save-one-tmp-blocked-zhuyin`
  MATCH the pin.

### `pinyin_alloc_instance` on a finalised context (policy row 60)

- **Upstream source cite:** `src/pinyin.cpp:1194-1222` (`pinyin_fini`
  deletes the context), `:1310-1333` (`pinyin_alloc_instance` reads
  `context->m_phrase_index` at `:1322` to build the instance's
  constraints, `phonetic_lookup.h:427`).
- **Mechanism:** a use after free. valgrind (2026-10-04, bdb): "Invalid
  read of size 8 at `pinyin_alloc_instance (pinyin.cpp:1322)`, address 48
  bytes inside a block of size 1,464 free'd". The call returns a non-NULL
  instance whose constraints hold a dangling pointer, and survives because
  the freed block is still mapped.
- **What oxpinyin does instead:** `pinyin_init` records each context's
  address and `pinyin_fini` forgets it (`crates/oxpinyin-capi/src/live.rs`);
  `pinyin_alloc_instance` answers NULL for an address that is not live, and
  logs nothing, because the pin does not abort. Before this entry the call
  dereferenced the freed Rust context and crashed (valgrind: invalid read in
  `ContextCore::alloc_instance`; SIGSEGV).
- **Externally observable:** yes — `alloc-instance-after-fini` in
  `contract-diff.py`: the pin returns an instance and exits 0, the parent
  build exits with SIGSEGV, this change returns NULL and exits 0. Class (b).
  A new context at a reused address is live again, which the registry cannot
  tell from the old one.
- **The zhuyin twin (lane C, PR 14):** `zhuyin_alloc_instance` reads the
  freed context the same way (`zhuyin.cpp:845-857`). `zhuyin_init` and
  `zhuyin_fini` keep their own registry (`crates/oxpinyin-zhuyin-capi/src/live.rs`)
  and the call answers NULL, silently; `zhuyin-alloc-instance-after-fini`
  holds the exit status.


### Empty system directory (#643; row 71, closed 2026-10-08 UTC)

This was a port defect, not an exception: at pin 074a2219,
`pinyin.cpp:331-338` / `zhuyin.cpp:275-282` resolve an empty system
directory in cwd; NULL instead fails on the empty filename. Both C
facades now preserve that distinction and the NULL failure's exact raw
stderr. `contract-diff.py`'s ten `init-system-*` cases cover both facades,
including parse/guess after a successful open and dot/absolute controls.
The pin tree read was `/home/sheng/work/libpinyin` at 074a2219.

### Remembered pronunciation tones (#699; row 72, closed 2026-10-08 UTC)

Ordinary userdir, library 7, import 你好你好 / ni3'hao3'ni3'hao3 / 100000,
parse ni3hao3ni3hao3, guess, remember count 3: the pin exports the toned
reading at 100003; the pre-fix subject exported toned 100000 plus toneless 3.
`src/pinyin.cpp:3578-3668` at 074a2219 carries complete matrix keys;
`capi/src/user_data.rs` dropped tone while projecting selected syllables.
The fix retains parsed tones without changing the selected syllable sequence.
No public interface, ABI or dependency changes. Both Zhuyin libraries lack
remember/export symbols. Existing contract driver cases remember-tones-pinyin
and remember-api-absent-zhuyin verify this distinction on BDB.
Source read from /home/sheng/work/libpinyin at the pin, pinyin.cpp blob
f27f7cf776724ead8d55e14b4af390e790054a91. This is plain parity.

### NULL user sessions (#642; row 73, closed 2026-10-08 UTC)

At 074a2219, pinyin.cpp:326-444 / zhuyin.cpp:269-358 allocate user
indexes even with NULL userdir. Imports (pinyin.cpp:514-654,
zhuyin.cpp:392-545) modify those indexes. Train (2670-2691 / 1696-1715)
and save (1132-1147 / 547-552) first refuse NULL, without writes.
Pinyin fini attempts an empty marker filename (1194-1200); Zhuyin
fini (741-757) does not write. Source: clean /home/sheng/work/libpinyin
at the pin. Runtime previously passed no UserStore, so imports refused.

The transient store preserves the session without a filesystem target.
Import 你好你好 at toned count 100000 now succeeds and exposes library-7
token 117440513 on both facades; Pinyin candidate zero is the user phrase.
Train/save remain false. Cwd and TMPDIR snapshots remain unchanged after
init, import, lookup, train, save and fini. Exact failed-empty-filename
stderr matches, including Pinyin fini's diagnostic. Ordinary and NULL
fresh-context imports to each library 1–7 are separate contract cases.
The ordinary imports already matched at main 3b016f48; the historical
#525 refusal claim for 1–4 is stale. Zhuyin has no phrase-export API.
Approved additive internal Rust APIs are documented in
../api/transient-user-store.md; C ABI and dependencies are unchanged.

### Import token-count assertion (#525; row 74, class (c), 2026-10-08 UTC)

Read /home/sheng/work/libpinyin at 074a2219. The asserts-live pin calls
reduce_tokens from _add_phrase (pinyin.cpp:541 / zhuyin.cpp:427);
src/storage/phrase_large_table3.h:77-95 counts matching tokens and asserts
`0 <= num && num <= 4` before any insertion. In a fresh ordinary context,
add 你好 with ni3'hao3 (Zhuyin ㄋㄧˇ ㄏㄠˇ), count100000, to libraries
1,2,3,4,5 in sequence: all succeed. Begin library6 succeeds; its add
SIGABRTs on both facades, with five existing tokens. The frequency
argument 100000 is not the bound: the asserted count is matching tokens.

The subject now returns false at that add with exactly one g_warning
under libpinyin / libzhuyin respectively. No state is changed. Existing
contract cases test the fifth-add success boundary and sixth-add refusal,
including tokens, unigram counts, exported readings where supported,
dirty state and profile bytes before/after refusal. Zhuyin has no export
API. No interface, ABI or dependency change. This row covers the import
call measured here, not every independent caller of reduce_tokens.

### `pinyin_bigram_iterator_get_next_phrase` after a walk that ends on a real predecessor (policy row 75)

- **Upstream source cite:** `src/pinyin.cpp:896-911` (`get_next`: the
  assert at `:902`, then `g_strdup(iter->m_phrase)` at `:904` and
  `g_strdup((gchar *) g_ptr_array_index(iter->m_pinyins,
  iter->m_pinyin_index))` at `:905`, then `return
  pinyin_bigram_iterator_has_next_phrase(iter)` at `:910`); `:789-894`
  (`has_next` frees `m_pinyins` and replaces it with an empty array at
  `:798-801` before it looks for the next row, and leaves `m_index_token` at
  the predecessor it loaded last, `:885-886`); `:779` and
  `src/storage/ngram_bdb.cpp:178-200` (the predecessors come in the user
  bigram's hash order, `DB_HASH`).
- **Mechanism:** a walk's `false` row leaves `m_pinyins` empty. If the
  predecessor loaded last is `null_token` or `sentence_start`, a further
  `get_next` asserts (policy row 62). If it is a real phrase token the assert
  passes and `:905` indexes the empty array: on GLib 2.90 `g_ptr_array_new`
  allocates no storage, so the read goes through a NULL pointer and the pin
  dies of SIGSEGV. Undefined behaviour.
- **What oxpinyin does instead:** answers `false` with `phrase`, `pinyin`
  and `count` untouched and logs nothing
  (`crates/oxpinyin-capi/src/iterators.rs:443-444`; the walk names the state
  `BigramStep::Undefined`, `crates/oxpinyin-facade/src/export_rows.rs:369-371`).
- **Externally observable:** yes: the pin's process dies, and oxpinyin
  answers `false`. Class (b). Not a `contract-diff.py` case, because which
  predecessor a walk loads last depends on the backend's hash order.
- **Measured** on bdb, linux/amd64 under Rosetta (native amd64 not run),
  2026-10-08 UTC, from the repository root at `3b016f48`, with `TMPDIR` on the
  container's own filesystem:

  ```
  gcc -shared -fPIC -o segv-report.so segv-report.c
  LD_PRELOAD=$PWD/segv-report.so python3 bigram-905.py \
      <prefix>/lib/libpinyin.so <prefix>/lib/libpinyin/data
  addr2line -f -i -C -e <prefix>/lib/libpinyin.so.15.0.0 0x97748
  python3 bigram-905.py <target>/debug/libpinyin_capi.so <prefix>/lib/libpinyin/data
  ```

  The pin prints `has_next True` and `get_next False 你好`, then
  `SIGSEGV addr=(nil) pc=<prefix>/lib/libpinyin.so+0x97748` and exits 139;
  `addr2line` names `pinyin_bigram_iterator_get_next_phrase` at
  `pinyin.cpp:905`. The pin built with the `bigram-export-strjoinv` patch,
  whose walk cannot over-read (row 1), prints the same two lines and faults
  at `+0x97768`, its `pinyin.cpp:907`: the same statement, two patch lines
  down. oxpinyin prints the same two lines, then
  `get_next again False [True, True, True] []`: `false`, the three
  out-params untouched, no log record. The raw capture is not retained.

  `bigram-905.py`:

  ```python
  # Run from the repository root: python3 bigram-905.py <libpinyin.so> <data-dir>
  import ctypes as C, runpy, sys, tempfile
  h = runpy.run_path('tools/bisection/contract-diff.py')
  P, B, I, S, Z = h['P'], h['B'], h['I'], h['S'], h['Z']
  k = h['Kit']('pinyin', sys.argv[1], sys.argv[2], tempfile.mkdtemp())
  for text, first, second in ((b'nihao', '你', '好'), (b'shijie', '世', '界')):
      inst = k.alloc()
      k.fn('parse_more_full_pinyins', Z, P, S)(inst, text)
      k.fn('guess_sentence', B, P)(inst)
      h['choose_text'](k, inst, h['choose_text'](k, inst, 0, first), second)
      k.fn('guess_sentence', B, P)(inst)
      k.fn('train', B, P, C.c_ubyte)(inst, 0)
  it = k.fn('begin_get_bigram_phrases', P, P)(k.ctx)
  get_next = k.fn('bigram_iterator_get_next_phrase', B, P, C.POINTER(P), C.POINTER(P), C.POINTER(I))
  out = [P(h['UNTOUCHED']), P(h['UNTOUCHED']), I(h['UNTOUCHED'])]
  print('has_next', k.fn('bigram_iterator_has_next_phrase', B, P)(it), flush=True)
  print('get_next', get_next(it, *map(C.byref, out)), k.text(out[0].value), flush=True)
  out = [P(h['UNTOUCHED']), P(h['UNTOUCHED']), I(h['UNTOUCHED'])]
  print('get_next again', get_next(it, *map(C.byref, out)),
        [o.value == h['UNTOUCHED'] for o in out], k.logs, flush=True)
  ```

  `segv-report.c` (gdb cannot ptrace under Rosetta):

  ```c
  /* LD_PRELOAD: on SIGSEGV print the fault address and the faulting PC as a
   * module offset for addr2line (gdb cannot ptrace under Rosetta). */
  #define _GNU_SOURCE
  #include <dlfcn.h>
  #include <signal.h>
  #include <stdio.h>
  #include <string.h>
  #include <ucontext.h>

  static void report(int sig, siginfo_t *si, void *ctx) {
      void *pc = (void *)((ucontext_t *)ctx)->uc_mcontext.gregs[REG_RIP];
      Dl_info d;
      memset(&d, 0, sizeof d);
      dladdr(pc, &d);
      fprintf(stderr, "SIGSEGV addr=%p pc=%s+0x%lx\n", si->si_addr,
              d.dli_fname ? d.dli_fname : "?",
              (unsigned long)((char *)pc - (char *)d.dli_fbase));
      signal(sig, SIG_DFL);
      raise(sig);
  }

  __attribute__((constructor)) static void install(void) {
      struct sigaction sa;
      memset(&sa, 0, sizeof sa);
      sa.sa_sigaction = report;
      sa.sa_flags = SA_SIGINFO;
      sigaction(SIGSEGV, &sa, NULL);
  }
  ```

### `mmap %s failed!`: a system library the pin cannot map, and the empty chunk it dies on (#545; policy row 76)

- **Upstream source cite:** `src/pinyin.cpp:256`, `:290`, `:956`, `:1265` and
  `src/zhuyin.cpp:200`, `:234`, `:589`, `:800` (`if (!chunk->mmap(chunkfilename))
  fprintf(stderr, "mmap %s failed!\n", chunkfilename);`, the `open %s failed!`
  branches beside them being the `#else` of `LIBPINYIN_USE_MMAP`, defined by
  `src/include/memory_chunk.h:31-34` whenever `HAVE_MMAP` is, and
  `config.h:42` of the pin's build defines it); `src/include/memory_chunk.h:470-520`
  (`MemoryChunk::mmap` resets the chunk, then answers `false` for a file that
  will not open, one shorter than the 8-byte header, a length word that is not
  the payload length, or a checksum that does not verify);
  `src/storage/phrase_index.cpp:244` and `:278` (`FacadePhraseIndex::load` and
  `diff` hand the chunk to `SubPhraseIndex::load`, `:335-363`, whose
  `g_return_val_if_fail(*(buf_begin + offset) == c_separate, FALSE)` at `:354`
  reads `buf_begin + 16`). Also `src/storage/facade_chewing_table.h:115` and
  `src/storage/facade_phrase_table2.h:96`, which no public call reaches (below).
- **Mechanism:** every site prints the line and carries on with the chunk
  `mmap` reset, which is empty: `begin()` is NULL and `size()` is 0, so
  `SubPhraseIndex::load` reads the separator byte at `NULL + 16` before its own
  bounds check can refuse the chunk. The pin dies of SIGSEGV. The sites are
  reached by `pinyin_init` / `zhuyin_init` (each default `SYSTEM_FILE`
  library, in index order), `pinyin_load_phrase_library` /
  `zhuyin_load_phrase_library` (a library that was unloaded),
  `pinyin_load_addon_phrase_library` (`:290`, the only entry to a `DICTIONARY`
  library), a modified `pinyin_save` / `zhuyin_save` (every loaded system
  library is mapped again to `diff` it) and `pinyin_mask_out` /
  `zhuyin_mask_out` (every loaded system library is reloaded). A library
  already loaded, an unloaded one in a save or mask-out, an unmodified save
  and a reload of a whole file stay silent. `zhuyin.cpp:234` is reached by no
  call: `zhuyin_init` and `zhuyin_load_phrase_library` `assert` that the default
  table is not a `DICTIONARY` (`:330`, `:372`; the pin is built without
  `NDEBUG`) and the zhuyin facade has no addon entry point.
  `FacadeChewingTable` and `FacadePhraseTable2` are included by
  `src/pinyin_internal.cpp` into the uninstalled `libpinyin_internal.a` and
  instantiated nowhere in `src/`, `tests/` or `utils/`; `libpinyin.so.15` and
  `libzhuyin.so.15` define no symbol of either class.
- **What oxpinyin does instead:** writes the pin's `mmap <path> failed!` line,
  the path as its bytes, at each reachable site on both facades, from
  `oxpinyin_data::pin_stderr`, and answers what it answered before: NULL with
  one GLib warning from an init whose library is short or corrupt, non-NULL
  from one whose library is absent (the library stays unloaded), `true` from a
  reload, a modified save and a mask-out, `false` from the addon load. It
  never reads through the empty chunk. The failure is the typed
  `LibraryError::Unmappable` the chunk reader raises for exactly what
  `MemoryChunk::mmap` refuses; a layout fault past the chunk is
  `LibraryError::Format` and has no line, as `SubPhraseIndex::load`'s
  `g_return_val_if_fail` has none. With several libraries broken it writes one
  line for each, in library order; the pin's bytes are the first. An init that
  fails on a library judges the user profile first, as the pin does, so the pin's
  `open <user>/user.conf failed.`, wipe and marker write precede its line and its NULL.
- **Externally observable:** yes: the pin's process dies, with the line as its
  last output; oxpinyin writes the line and goes on. Class (b).
- **Measured** on bdb, native amd64, 2026-10-09 UTC, in a Debian testing
  container over `1be4e35d` with this change: the `stderr-library-*` cases
  of `tools/bisection/contract-diff.py` break `merged.bin`, `gbk_char.bin` or
  `art.bin` in a private copy of the system directory (absent, cut to four
  bytes, last payload byte flipped) and compare stderr up to the crash. 39
  crash cases (21 pinyin, 18 zhuyin, fresh and non-conforming user dirs among them) match:
  the pin dies of SIGSEGV after the same bytes, and oxpinyin's answers are
  the ones listed above; the 11 controls match. Against the parent build the
  39 differ and the 11 match. Fault address, with a preload that prints
  `si_addr` and re-raises:

  ```
  gcc -shared -fPIC -o segv-addr.so segv-addr.c
  LD_PRELOAD=$PWD/segv-addr.so tools/bisection/run-contract-diff.sh bdb <prefix> \
      <target>/debug/libpinyin_capi.so <target>/debug/libzhuyin_capi.so \
      -- --cases stderr-library-save-missing --observations obs.jsonl
  ```

  The pin's stderr in `obs.jsonl` ends `SEGV si_addr=0x10` for `-init-`,
  `-load-`, `-save-`, `-mask-out-` and `-addon-` alike, pinyin and zhuyin.

  `segv-addr.c`:

  ```c
  #define _GNU_SOURCE
  #include <signal.h>
  #include <stdio.h>
  #include <string.h>
  #include <unistd.h>

  static void on_segv(int sig, siginfo_t *info, void *ctx) {
      char line[64];
      int n = snprintf(line, sizeof line, "SEGV si_addr=%p\n", info->si_addr);
      (void)!write(2, line, n);
      signal(sig, SIG_DFL);
      raise(sig);
  }

  __attribute__((constructor)) static void install(void) {
      struct sigaction sa;
      memset(&sa, 0, sizeof sa);
      sa.sa_sigaction = on_segv;
      sa.sa_flags = SA_SIGINFO | SA_NODEFER;
      sigaction(SIGSEGV, &sa, NULL);
  }
  ```

### `table.conf` follows its rows; malformed ones fail the triggering call (#694 and #525 batch B; policy rows 78, 79, 80 and 83)

Registered 2026-10-10 UTC. Pin source read at `074a2219c90feaf962d0d24f034514033ece5f99`
(`src/storage/table_info.cpp`, `src/pinyin.cpp`, `src/zhuyin.cpp`,
`src/include/memory_chunk.h`, `src/lookup/phonetic_lookup.h`).

- **What the pin does:** `SystemTableInfo2::load` (`table_info.cpp:194-294`)
  reads `binary format version:`, `model data version:`, `lambda parameter:`,
  `source table format:` and `database format:` with five `fscanf`s over one
  stream, then rows of six words (`default <enum name>` or `addon <number>`,
  table, system and user file names, file type) until `feof`. The rows fill
  two arrays of sixteen `pinyin_table_info_t` (default and addon). The init
  loops (`pinyin.cpp:377-392`, `zhuyin.cpp:318-334`) load every default row
  that is not `NOT_USED`; `pinyin_load_addon_phrase_library` loads one addon
  row; `_write_files`/`_rename_files`/`_clean_user_files` write, rename and
  unlink the files the rows name.
- **What oxpinyin did:** read the lambda and version lines only, and
  hard-coded the library layout. A valid but non-stock `table.conf` was
  ignored; a malformed one was accepted without a word.
- **What it does now:** `oxpinyin_data::table_info::TableConf::parse` is the one
  reader (the decoder's λ, the prediction λ, the user marker's versions and
  database format and the library layout all come from it); the layout reaches
  the system dictionary, the addon dictionary and the user store
  (`oxpinyin_user::SystemOriginals`, `UserStore::has_user_library`), and the
  two C ABIs' library calls follow the rows. Malformed files fail the call that
  triggers the pin's abort, with exactly one warning (policy row 79). The
  header's ordinary `false` returns answer NULL with the raw `load %s failed!`
  line and no warning; a missing `source table format:` line is an
  uninitialised read at the pin (row 80). λ is read as `%f` reads it; and
  outside `[0, 1]` the costs go negative or NaN, reproduced with GLib's merge sorts ported (row 83).
- **Evidence:** `tools/bisection/run-contract-diff.sh bdb <oracle prefix>
  <libpinyin_capi.so> <libzhuyin_capi.so> -- --cases <the table-conf-* names>`
  (141 cases; the abort cases hold the pin's SIGABRT, the crash cases its
  SIGSEGV, and the init aborts compare the user directory each side leaves).
  The SIGSEGV shapes were traced with `gdb -batch -ex run -ex bt --args python3
  <script that opens the pin's libpinyin.so on the private system directory,
  parses nihao, calls pinyin_guess_sentence and pinyin_guess_candidates>` in the
  `debian:testing` container on Linux x86-64.
- **Not held:** (1) a `NULL` user-file name on libzhuyin, where the pin's answer
  depends on the file system (row 79); (2) a default row retyped across the
  fixed roles of sub-indices 5–7 beyond the cases measured (row 78).
- **Found on the way:** the `SystemVersions` documentation said upstream reads
  `table.conf` through GLib's key file; it reads it with `fscanf`, as above.
  `UPSTREAM_DB_FORMATS`' comment said an unknown token is `UNKNOWN_FORMAT`; it
  reaches the `abort()` at `table_info.cpp:132`.

### Chunk-header write failure during save (#525; row 77, class (c), 2026-10-10 UTC)

Read libpinyin at `074a2219c90feaf962d0d24f034514033ece5f99`.
`MemoryChunk::save` (`src/include/memory_chunk.h`) writes a chunk's 4-byte
length word and then its 4-byte checksum, each with a `check_result` assert
that the `write` returned the full word (`:543`, `:547`). A filesystem that
refuses the write aborts the pin: with `user.bin.tmp` symlinked to `/dev/full`
the very first write (`:543`) fails, and with `RLIMIT_FSIZE` of 4 the length
word lands and the checksum word (`:547`) is refused. Both abort
`pinyin_save`/`zhuyin_save` of a dirty context on both facades (SIGABRT,
exit -6).

Before this change the subject's header write was an ordinary `Io` error,
which its reporting save tolerated as one more per-file write failure and
carried past into its rename pass. It now stops at these two writes, answers
`false` naming the header word, and the facade emits exactly one `g_warning`:
under `libpinyin` for `pinyin_save` and `libzhuyin` for `zhuyin_save`. The
soft chunk payload writes are unchanged: a refused payload write stays an
ordinary per-file I/O failure that row 69 tolerates, the save continues and
reports the renames and the marker. A header failure comes after staging
starts, so the profile is not untouched: `stage_user_bigram` can already have
replaced the bigram file in place, and the failing chunk is left as a partial
`.tmp`. No interface, ABI or dependency change. This row covers the chunk
header writes reached through the save calls, not the unrelated
`MemoryChunk::mmap` reads of row 76.

Held by `contract-diff.py` cases `abort-save-chunk-header-length`,
`abort-save-chunk-header-checksum`,
`abort-zhuyin-save-chunk-header-length` and
`abort-zhuyin-save-chunk-header-checksum`: the pin dies of SIGABRT (exit -6),
the subject exits 0 with one warning in its own domain and `false`; all four
MATCH on bdb, kc and tkrzw, and DIFFER against the parent build.

### `pinyin_remove_user_candidate`'s removal asserts (#525; row 78, class (c), 2026-10-10 UTC)

Read libpinyin at `074a2219c90feaf962d0d24f034514033ece5f99`.
`pinyin_remove_user_candidate` (`src/pinyin.cpp:3726-3769`) removes a user
phrase from three structures in turn and asserts each removal
(`assert(ERROR_OK == retval)`): the phrase index (`:3742-3743`,
`phrase_index->remove_phrase_item`), the phrase table (`:3749-3750`,
`phrase_table->remove_index`), and the pinyin table once per stored reading
(`:3756-3759`, `pinyin_table->remove_index`). Three reachable triggers abort
the pin (SIGABRT, exit -6), each reproduced on bdb with a small ctypes driver
under an `LD_PRELOAD` SIGABRT handler that prints the failing statement:

- **`:3743`, a candidate removed twice.** Add a single-reading user phrase,
  `pinyin_guess_candidates` it up, remove it (the pin answers `true` and
  deletes the item), then remove the same candidate again: the phrase index no
  longer holds the token, so `remove_phrase_item` fails.
- **`:3750`, `user_phrase_index.bin` absent.** Add a user phrase, `pinyin_save`,
  `pinyin_fini`, delete (or rename away) `<user>/user_phrase_index.bin`, reopen,
  guess and remove. The phrase index is loaded from `user.bin` (so `:3743`
  passes), but the phrase table is loaded from `user_phrase_index.bin`, so
  `remove_index` finds no row.
- **`:3759`, a reading merged rather than indexed.** Add a user phrase with two
  readings: `_add_phrase` indexes only the first reading
  (`pinyin.cpp:569-582`) and merges the second into the same item without a
  pinyin-table row (`#534`), so `remove_index` fails for it.

oxpinyin already refused when the token was absent from `PHRASE` but removed
everything else unconditionally, and `pinyin_remove_user_candidate` returned
its `false` with no log. The store now loads `user_phrase_index.bin` as a
token→text membership set (`PHRASE_TABLE`) that is checked against the
`PHRASE` text, together with every stored reading's `indexed` flag, before
anything is removed; a miss returns `Ok(false)` and leaves the store
untouched. The C facade emits exactly one `g_warning` in `libpinyin` (level
16, `pinyin_remove_user_candidate: assertion 'ERROR_OK == retval' failed`)
and answers `false`. The subject's text lookups are unchanged: they still
derive from `user.bin`, so the phrase table is read as a membership set only.

This retires the #525 ledger's earlier classification of `:3750` among the
not-applicable sites (the different-storage-model note): the divergence was
real and reproducible, not structural. `:3750`'s neighbours `:554`, `:568`
and `:571` remain separate sites.

Held by `contract-diff.py` cases `abort-remove-user-candidate-twice`,
`abort-remove-user-candidate-missing-phrase-table` and
`abort-remove-user-candidate-multi-pron`: the pin dies of SIGABRT (exit -6),
the subject exits 0 with one `libpinyin` warning and `false`; all three MATCH
on bdb, kc and tkrzw, and DIFFER against the parent build. No interface, ABI
or dependency change.
