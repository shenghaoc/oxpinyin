# Sentence surface (W14)

Date: 2026-08-17 · Status: characterization (pre-implementation), then record

The three-part W14 divergence (#100): sentence candidates do not emit with
real unigrams, the merged rows are not typed `NBEST_MATCH`, and
`pinyin_get_sentence` returns the raw form instead of the decoded 1-best.
This note characterizes the pinned oracle (libpinyin 2.11.91 @ 0c5e80e, tree
at `target/oracle-pin-build/src/libpinyin-2.11.91/`, `$S` below) from source
and from a live probe over the model20 tables.

## 1. Where the rows come from

There is no `collect_sentence` upstream. The sentence surface is the n-best
lookup plus two passes at the end of `pinyin_guess_candidates`:

- `pinyin_guess_sentence` (`$S/src/pinyin.cpp:1373-1385`) resets
  `m_prefixes` to `[sentence_start]` and calls
  `PhoneticLookup<2, 3>::get_nbest_match`, filling
  `instance->m_nbest_results` (`pinyin.cpp:89`).
- `pinyin_guess_candidates` sorts the phrase candidates
  (`pinyin.cpp:2284-2286`), then — when the sort word lacks
  `SORT_WITHOUT_SENTENCE_CANDIDATE` (0x1, `pinyin.h:56-58`) — calls
  `_prepend_sentence_candidates` (`pinyin.cpp:2292-2293`).
- `_prepend_sentence_candidates` (`pinyin.cpp:1934-1951`) prepends one
  `NBEST_MATCH_CANDIDATE` per n-best result **in reverse index order**, so
  n-best index 0 (the 1-best) sits at the very head of the list, ahead of
  every phrase candidate. Zero results ⇒ no rows at all.
- `_compute_phrase_strings_of_items` fills each row's display string by
  calling `pinyin_get_sentence(instance, candidate->m_nbest_index, ...)`
  (`pinyin.cpp:2004-2009`).
- `_remove_duplicated_items_by_phrase_string` (`pinyin.cpp:2058-2160`)
  then deduplicates the whole list by display string with the NBEST-wins
  rule: a phrase candidate colliding with an NBEST row is retyped
  `ZOMBIE_CANDIDATE` and removed (`pinyin.cpp:2116-2126`); two NBEST rows
  with the same string keep the lower index (`pinyin.cpp:2103-2114`).

**Gating.** The parity sort word `SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_
AND_FREQUENCY` = 0x1e (`pinyin.h:66-69`) does **not** carry
`SORT_WITHOUT_SENTENCE_CANDIDATE`, so rows appear exactly when
`m_nbest_results` is non-empty — i.e. when `pinyin_guess_sentence` ran since
the last reset (`pinyin_reset` clears it, `pinyin.cpp:2698`; nothing else
does). The frozen corpus profile never calls `pinyin_guess_sentence`
(`crates/pinyin-oracle/src/live.rs`, `observe_without_reset` →
`collect_candidates`), which is why the W4 candidate fixture and the pin
suite exclude sentence rows: **a pin move when rows are correctly gated
behind `guess_sentence` is a leak**.

**Live-probe evidence** (probe over the model20 tables, empty user store,
ibus call order `guess_sentence` → `guess_candidates(0, 0x1e)`):

- `nihao` → sentences `[你好, 你好, 你好]`; candidate total 126: the three
  rows collapse to one (lower index wins) and the phrase `你好` is
  zombied, so the total equals the phrase-only total (the #97 single-phrase
  half of the evidence).
- `nihaoshijie` → `[你好世界, 你好时节, 你好是届]`; total 129 = 126 + 3
  (the #97 multi-phrase half).
- `cecenihao` → `[测测你好, 测测你号, 测测你好]`; rows 0 and 1 survive the
  NBEST-vs-NBEST rule, row 2 is zombied.
- `zhongguoren` → `[中国人, 中国人, 中国任]`; surviving rows carry n-best
  indices 0 and 2 — the index is the *original* tail rank, not a
  renumbering.

## 2. The n-best lookup itself

`PhoneticLookup<nstore = 2, nbest = 3>` (`$S/src/pinyin.cpp:55,406`) over
the full `PhoneticKeyMatrix`:

- Trellis steps are matrix columns; nodes at a step are keyed by the phrase
  token **ending** there; each node keeps the best `nstore = 2` values
  (`insert_candidate`, `phonetic_lookup.h:311-330`; the heap store is
  `phonetic_lookup_heap.h`).
- Per step the beam keeps the best `nbeam = 32` values across all nodes
  (`phonetic_lookup.h:37`, `771`), ordered by `trellis_value_less_than`
  (`phonetic_lookup.h:66-87`): longer sentences win, except a shorter one
  whose accumulated cost beats the longer one's by more than
  `LONG_SENTENCE_PENALTY = log(1.2)` (`phonetic_lookup.h:39`); at equal
  sentence length the lower cost wins.
- Span search widens `m` from `i + 1` until the pinyin table search stops
  returning `SEARCH_CONTINUED` (`phonetic_lookup.h:800-827`) — the same
  prefix-driven widening the engine's window scan ports.
- Per span, `search_bigram2` expands **all** beam values over the merged
  (system+user) bigram successors of their last token
  (`phonetic_lookup.h:572-620`); `search_unigram2` expands **only the
  beam's first value** over every token the span spells
  (`phonetic_lookup.h:539-542`, `552-570`).
- Step cost (`phonetic_lookup.h:628-676`):
  `log((λ·bigram_poss + (1−λ)·unigram_poss) · pinyin_poss)` for the bigram
  branch, `log(unigram_poss · pinyin_poss · (1−λ))` for the unigram branch,
  where `unigram_poss = phrase_unigram / phrase_index_total` and
  `pinyin_poss = compute_pronunciation_possibility`
  (`phonetic_key_matrix.cpp:120-160`) = matched-pronunciation-frequency /
  total-pronunciation-frequency of the phrase item, summed over the matrix
  paths of the span (polyphone discounting).
- Tails: the top `nbest = 3` values at the last step, sorted best-first
  (`get_tails`, `phonetic_lookup.h:330-341`; the sort is
  `trellis_value_compare`, `phonetic_lookup.h:174-177`), each backtracked
  into a `MatchResult` token array (`extract_result`,
  `phonetic_lookup.h:369-396`).

`pinyin_get_sentence` (`pinyin.cpp:1463-1482`) returns
`convert_to_utf8(phrase_index, result, NULL, false)` — the decoded n-best
sentence row selected by `index`, rendered through the phrase index
(`lookup.cpp:27-70`) — only when an active sentence lookup has a result at
`index < m_nbest_results.size()`; otherwise it returns `false`. Index 0 is
the 1-best. In this fork, the pre-W14 raw form (scheme keystroke buffer /
session preedit) remains the pre-lookup fallback until
`pinyin_guess_sentence` runs; after that gate is active,
`pinyin_get_sentence` answers only from the decoded rows or `false`,
including when the lookup produced no rows.

`pinyin_choose_candidate` on an NBEST row (`pinyin.cpp:2511-2519`) diffs
`constraints` between result 0 and the chosen index and returns
`matrix.size() - 1`: choosing a sentence selects the whole input.

## 3. What the engine must reproduce

1. Rows only after `guess_sentence` (the `m_nbest_results` gate), ≤ 3,
   tail order preserved (index visible through
   `pinyin_get_candidate_nbest_index`), NBEST-wins merge against phrase
   candidates, lower-index-wins among rows.
2. Decoded n-best rows through `pinyin_get_sentence` (index 0 is the
   1-best), `false` past the row count.
3. The trellis ordering: length-preferring comparator with the log(1.2)
   penalty, beam 32, two stores per (step, token), unigram expansion only
   from the beam head, λ-blended bigram/unigram costs on the same counts
   the engine's `LanguageModel` already exposes.

Ported divergences (recorded, not chased):

- `pinyin_poss` takes the looked-up spelling's matched/total share once
  per span (§4): it does not sum the possibility over every matrix path
  of one span the way `compute_pronunciation_possibility` does (first
  path per token wins), and a matched share rarer than the 40-bit cost
  floor is treated as upstream's below-ε step skip. The per-pronunciation
  counts themselves are carried, so the polyphone discount applies.
- Upstream accumulates `gfloat` costs; the port uses the core fixed-point
  surprisal scale. Ordering agrees except within float rounding of a tie.
- The ported comparator is **not a strict weak order** — neither is
  upstream's — and beam/tail top-k run it through Rust's `sort_by`, whose
  contract wants a total order and which the pinned toolchain
  demonstrates panicking on adversarial value sets (three beam values
  with lengths 2/3/4 and costs 600/300/0 form a cycle; reproduced under
  1.97.1). No corpus or fuzz input has tripped the detector, and the
  frozen agreement pins were measured under this exact ordering: a
  heap-shaped pairwise selection (upstream's `get_top_results` shape)
  was implemented and measured at 459/238/211 — a pin move, so it was
  rejected per the frozen-pin rule. Recorded for the upstream
  report-back; revisit only alongside a deliberate pin re-freeze.
- The constraint machinery (`CONSTRAINT_ONESTEP`/`diff_result`) **is
  ported** (`feat/constraint-machinery`, 2026-08-22 — the store in
  `constraint.rs`, the gated full-matrix walk, the reset split, the
  constraint-aware train, `pinyin_clear_constraint`; measured in
  `live-typing.md`). The remaining-input re-seed (the W6 surface) still
  carries every empty-store decode — the frozen pins' shape — and the
  §10 prefix context remains the display rule on that path.

## 4. Implementation record

- `LanguageModel::nbest_step_costs` (defaulted; sanctioned growth per
  `core-trait-seam.md` "future growth must use methods with default
  implementations"): the two branch costs of §2 for one step.
  `BigramLanguageModel` implements both branches from the same counts
  `score()` blends; the blended branch answers only when the previous
  token's bigram row actually carries the successor (count > 0), matching
  `search_bigram2`'s merged-gram-successor walk.
- `PhraseEntry` grows `pronunciation_possibility (matched, total)`
  (builder on; `new` leaves it unset). `SystemDictionary::lookup` fills it
  from the pinyin-index record's frequency (the matched share) and the
  aggregated unigram map (the item's pronunciation total) — the export ABI
  carries exactly the per-pronunciation counts
  `pinyin_iterator_get_next_phrase` reports (`pinyin.cpp:698-743`). The
  trellis adds `surprisal(matched, total)` to every step; a matched share
  of 0 skips the step. The candidate scan never reads the field, so the
  corpus pins are untouched by it.
- Engine: `nbest.rs` trellis + `Session::guess_sentence`/`sentence_text`;
  `refresh` prepends stored rows and merges (NBEST wins, lower index
  wins), mirroring §1. Rows are cleared by `reset`, exactly the
  `pinyin_reset` rule. `Candidate::nbest_index` carries the tail rank.
  Without real unigrams the per-path DP supplies up to three rows so the
  fallback surface exists whenever the model reports
  `has_real_unigrams() == false` (the fallback candidate list keeps its
  pre-W14 shape until a guess happens). With real unigrams the trellis
  branch instead requires the model to answer `nbest_step_costs` — the
  default trait impl returns no cost data, so a model that has real
  unigrams but leaves the default in place produces no rows. §6 records
  the C ABI façade currently in that shape.
- Rows carry their token path, and choosing a row — identified by its
  own tail rank through `Candidate::nbest_row()` (`Option<u8>`, `None`
  for every non-row candidate, so a fallback sentence candidate records
  nothing; §7 — originally by list position, which the NBEST-wins dedup
  can shift) — records **all** of it:
  upstream keeps the chosen `MatchResult` on the instance and
  `pinyin_train` walks it, so the engine's selection record
  (`Session::select` → `history`) extends with the row's tokens. The
  fallback DP's rows carry tokens too — without this, the
  `predict-diff` driver's train-then-predict flow lost the second
  phrase's token (the driver picks the NBEST row for `nihao` → `你好`,
  which has no single token) and the user bigram never landed. Caught by
  `tools/bisection/run-predict-diff.sh`; regression-tested green after
  the fix.
- C ABI: `pinyin_guess_sentence` fills the rows and marks the lookup
  active (`Session::sentence_lookup_active`, cleared only by `reset`);
  while a lookup is active, `pinyin_get_sentence` answers decoded-or-
  `false` — never the raw form — even when the lookup produced no rows
  (upstream's `get_nbest_match` clears `m_nbest_results` before every
  attempt, so the `0 == results.size()` false covers that case too). The
  pre-W14 raw form survives only before any lookup, for scheme parses
  and un-guessed instances. `pinyin_get_candidate_nbest_index` reports
  the tail rank; `pinyin_guess_candidates` honours
  `SORT_WITHOUT_SENTENCE_CANDIDATE` by excluding sentence candidates.

## 5. Measured agreement

`fixtures/w4/oracle-sentence-surface.txt` freezes the oracle's surface for
a deterministic 503-input W2 sample (every 20th corpus input plus the
`nihao` / `nihaoshijie` / `cecenihao` / `zhongguoren` anchors); regenerate
with `cargo run -p pinyin-oracle --features oracle-ffi --bin
oracle-sentence-surface`. `sentence_surface_reports_parity` pins the
engine's agreement against it:

- **typing and structure exact everywhere**: NBEST rows at the head in
  tail order, `n/0` first; no sentence candidate before `guess_sentence`
  (the gating that keeps the W4 corpus pins frozen — re-measured
  bit-identical: top-1 10177, top-5-set 10189, absent 1, prefix-10
  94871/98930, tie-swaps 1036); `guess` retval agrees on all 503.
- **decoded 1-best (`pinyin_get_sentence` row 0): 488/496 = 98.4%** of the
  comparable inputs (the 7 junk-leading inputs the oracle does not parse
  at all are excluded — no sentence surface exists on either side).
- **full sentence lists 385/496, first-6 candidate rows 370/496.** The
  residuals are segmentation near-ties (`shuan` resplit vs whole) and
  second/third-tail ties, the §3 divergences: upstream accumulates `gfloat`
  and breaks comparator ties in heap order, the port uses fixed-point
  surprisal with insertion-order tie-breaks.

Before the pronunciation discount the row-0 agreement was 74.5% on a
200-input probe — the polyphone term is the load-bearing scoring input,
not a refinement.

`sentence_surface_fixture_is_fresh` (oracle-ffi, ignored) re-captures the
sample live and asserts the fixture matches; it passed against the pinned
oracle at freeze time.

## 6. W13 scheme re-measure — the C ABI n-best wiring gap

Date: 2026-08-19 · main tip `489e94d` (PR #113 merge, W14 landed).

The #97 verdict table re-run against the current tip through
`tools/bisection/run-scheme-diff.sh`:

| Surface | Pre-W14 (#97) | Post-W14 (`489e94d`) |
|---|---|---|
| `PARSE_AUX_ONLY` double | PARSE_AUX_IDENTICAL | **PARSE_AUX_IDENTICAL** |
| `PARSE_AUX_ONLY` STANDARD bopomofo | PARSE_AUX_IDENTICAL | **PARSE_AUX_IDENTICAL** |
| Full-candidate double (pin model both sides) | DIVERGE — sentence/NBEST gap + `get_sentence` raw vs decoded + tail tie-order | **DIVERGE** — sentence/NBEST rows still absent, `get_sentence: (null)`, tail tie-order |
| Full-candidate STANDARD bopomofo | DIVERGE — same full-pinyin picture | **DIVERGE** — same |
| Sentence rows on real unigrams (via C ABI) | absent (nbest = 0), main and W13 | **absent** on both double and STANDARD, full-pinyin C ABI included |

Default candidate pins re-measured under this branch: **10177 / 10189 /
94871 of 98930 / absent 1 / tie-swaps 1036**, bit-identical. Sentence-
surface pins re-measured: **488/385/370**, bit-identical. Nothing moved.

**Where the sentence rows go.** The W14 trellis works: the direct-Session
test `sentence_surface_reports_parity` pins the 488/385/370 agreement
against the oracle fixture, and that path uses `BigramLanguageModel`
directly. Through the C ABI the lookup does activate — `pinyin_guess_
sentence` runs, clears prior rows, and returns `true`, so
`Session::sentence_lookup_active()` flips on — but the row set comes back
empty. The reason is one line: `SharedLm`
(`crates/oxpinyin-capi/src/state.rs:301-363`) implements `LanguageModel`
but does not override `nbest_step_costs`. The trait's default is
`Ok(NbestStepCosts::default())` — no cost data — so the trellis in
`Session::guess_sentence` produces zero rows for every C ABI caller
regardless of scheme or full pinyin. With the lookup active but empty,
`pinyin_get_sentence` answers false / `(null)` (the W14 decoded-or-
nothing gate over an empty row set) and `pinyin_guess_candidates`
prepends nothing.

A one-input C-ABI probe against the same tables confirms the reach:
`pinyin_parse_more_full_pinyins("nihao")` → `guess_sentence: true`,
`get_sentence: (null)`, `cand[0] type=NORMAL text="你好"`. The scheme
paths inherit this from the shared session; they are not the source.

**Classification vs the #97 baseline.**

- PARSE_AUX rows: **gone as a divergence class** — both surfaces still
  PARSE_AUX_IDENTICAL, unchanged from #97.
- Full-candidate rows: **still DIVERGE, same underlying cause** (sentence
  surface absent on the C ABI path). The visible signature on
  `get_sentence` shifted from raw input to `(null)` because W14 tightened
  the gate — that is the intended W14 semantics, not a scheme-side
  regression. The tail tie-order residual is unchanged.
- No new / unexpected divergences. No default-pin movement.

**Not fixed here.** The C ABI wiring gap belongs to whoever owns the
`SharedLm` façade; it is documented above as a one-method extension. The
scheme paths are correct on the surfaces they own (PARSE_AUX, aux text,
consumed offsets); closing the sentence rows for them requires the same
`nbest_step_costs` forward that fixes the full-pinyin C ABI. Recorded,
not chased.

The scheme differential driver itself needs no changes: it already
exercises the exact surface where the gap shows.

## 7. The C ABI wiring gap, closed

Date: 2026-08-19 · branch `fix/w14-shared-lm-nbest-costs`.

`SharedLm` now overrides `nbest_step_costs`, forwarding to the
`BigramLanguageModel` behind it — the same model the rest of the C-ABI
decode scores against. One method, no trellis/comparator/candidate
changes; the trait default stands for other implementors.

**Scope: the forward is system-only.** `UserStore` count deltas are
deliberately not folded into the step costs here — this change replaces
the trait default's *empty* costs with the system model's, so C-ABI
`guess_sentence` emits rows at all. The §5 user overlay (merge before
the presence gate, merged denominators — the semantics upstream's
`PhoneticLookup` runs) is the next workstream, gated on the
shifted-row selection-record fix (#117): until the chosen row's own
token path is recorded, a user-trained (你→浩)-style pair trains the
wrong path and any user-merged differential is vacuous. All pins and
probes below therefore run an empty user store, where merged ==
system.

Re-verification:

- One-input C-ABI probe (full-pinyin `nihao`, model20 tables):
  `guess_sentence: true`, `get_sentence(0): 你好`, `cand[0]` is
  `NBEST_MATCH` with n-best index 0, 126 candidates — byte-identical to
  the same probe against the pin-built oracle.
- `run-scheme-diff.sh` double and STANDARD bopomofo (pin model both
  sides): `get_sentence` is decoded text on every input, no `(null)`,
  NBEST rows present on both surfaces. Both still end DIVERGE — the
  residual is the known tie-order class (tail order and decoded-1-best
  near-ties, e.g. `我们`/`我吗`, `最` appearing at rank 5 instead of a
  second NBEST row), the same §3 comparator divergences the direct-Session
  488/385/370 measurement already prices in. Not chased here. (Amended 2026-08-27: with matched unigram data and the exact scheme-key seam — `docs/findings/bopomofo-spec.md` § exact seam — the scheme differentials run IDENTICAL; the double/full share of this residual was a harness data mismatch, the bopomofo share the re-segmentation bug. The §12 direct-Session residual stands as its own measurement.)
- Pins: default candidates 10177 / 10189 / 94871 of 98930 / absent 1 /
  tie-swaps 1036; sentence surface 488/385/370 — all bit-identical
  (`real_tables_session_reports_parity`, `sentence_surface_reports_parity`).
- union / train / import / predict diffs and bisect+valgrind all green;
  fmt, clippy `-D warnings`, `test --locked --workspace` green.

## 8. The selection record's row mapping, fixed

Date: 2026-08-19 · branch `fix/w14-nbest-select-path`.

**The defect.** `Session::select_inner` recorded a chosen n-best row's
token path as `nbest_rows[list_position]` (the `kind == Sentence &&
index < nbest_rows.len()` test, pre-fix at `session.rs:346-357`). The
NBEST-wins dedup keeps the lower-index row when two rows share a string,
so a surviving row's list position equals its rank only while no earlier
row was dropped. Rows `[好, 好, 浩]` present 好 (rank 0) at position 0 and
浩 (rank 2) at position 1: choosing 浩 recorded row 1's — 好's — tokens,
and `pinyin_train` then wrote 你→好 instead of 你→浩. Upstream never had
the problem because `pinyin_choose_candidate` keeps the chosen
`MatchResult` on the instance (`pinyin.cpp:2511-2519`) and train walks
that, not a positional lookup. `run-train-diff.sh` could not see it: its
sequence always chooses 你→好, whose candidate sits at position 0 = row
0.

Found while building a user-merged-costs n-best differential: training
(你→浩) ×3 through the C ABI exported `你浩|138` (one seed) plus
`你好|11178` on the engine side, while the pin-built oracle exported
`你浩|1242` and `你好|1242`.

**The fix.** `Candidate`'s `nbest_index: u8` ("0 also means not a row")
became `Option<u8>`: `Some(rank)` on row-origin candidates only, `None`
for everything else — including fallback sentence candidates, which keep
their record-nothing behaviour (`a_fallback_sentence_never_records_row_
tokens`). `Candidate::nbest_index()` still answers `u8` for the C ABI
(`pinyin_get_candidate_nbest_index`), and `Candidate::nbest_row()` is
the origin marker the selection record reads: row-origin candidates look
up `nbest_rows[rank]`, bounded by `get`; no candidate's record goes
through the list position again.

**Evidence.**

- Engine: `a_shifted_row_records_its_own_rank_not_its_position`
  reconstructs rows `[好, 好, 浩]` and asserts the 浩 row sits at position
  1 with rank 2 (the shift precondition) and records 浩's tokens.
  Reverting the lookup to the positional form fails the test with
  `[0x101]` (the deduped 好 row) against `[0x102]` expected.
- C ABI (`tools/bisection/run-nbest-train-diff.sh`, matched model20
  tables, export lines vs the pin-built oracle): (你→浩) × 3 exports
  `你浩|ni'hao|1242` + `你好|ni'hao|1242` — identical to the oracle, with
  你浩 doubling 138 → 414 → 1242 across rounds instead of stalling at
  138 while seeds pile onto 你好. Measured on this branch stacked with the
  §7 `SharedLm` forward (#116): without it the C ABI emits no
  rows on the full-pinyin surface, every round chooses the NORMAL
  candidate, and the run is trivially green — the runner prints a
  vacuity warning in that shape.
- Pins re-measured on this branch alone: default candidates 10177 /
  10189 / 94871 / absent 1 / tie-swaps 1036; sentence surface
  488/385/370 — bit-identical. union / train / import / predict diffs and
  bisect+valgrind green; fmt, clippy `-D warnings`, workspace tests
  green.

**Recorded, fixed in §10.** A second divergence seen while debugging:
our offset-decode sentence rows covered only the remaining input (single
chars 好/浩) while the oracle's carry the full context (你好/你浩) — the
§3 constraint-machinery gap. It did not affect this fix (the record
follows the chosen row, whatever its text). §10 prepends `selected` to
each n-best row's text in `guess_sentence`, closing the gap without
porting the constraint trellis.

## 9. User-merged n-best step costs, landed

Date: 2026-08-20 · branch `feat/w14-nbest-user-delta`.

**The gap.** The §7 `SharedLm` forward (#116) answered the
`BigramLanguageModel`'s system-only costs: rows existed, but their step
costs ignored the §5 user overlay (`user-store.md` §5), so a trained
pair could not cheapen its own row the way `score` already allowed.
Upstream's n-best runs on the same user-aware lookup as `score` —
`merge_single_gram` before the presence gate (`ngram.cpp:277`) — so the
step costs must merge user counts into **both** branches, with merged
denominators, before the observed-successor gate.

**The change.**

- `BigramLanguageModel::nbest_step_costs_with_user_delta(prev, token,
  user)` recomputes both branches over the merged counts: the unigram
  term over `system + user.unigram_delta` / `system_total +
  user.unigram_total_delta`, the blended branch over
  `merged_transition` (system load then `merge_bigram`) — merged
  *before* the count > 0 presence gate. A user-trained successor with
  no system count therefore blends instead of falling through to the
  unigram branch. Two miss shapes: a prev with no system row at all
  (denominator is the user total), and a prev that has a system row
  but not this next token — the 你→浩 shape (denominator is
  `system_row_total + user_total`). The trait method delegates with
  `UserCountDelta::ZERO`, bit-identical to the pre-change body.
- `SharedLm::nbest_step_costs` takes the same `user_delta` overlay
  `score` takes (`count_delta(Some(prev), token)`) and forwards.
  Nothing in the trellis, comparator, or the §8 selection record
  changed.

**Evidence.**

- Unit (`oxpinyin-data`): `nbest_zero_delta_is_bit_identical_to_trait_impl`
  covers every trait shape — observed successor, count-0 non-successor in
  an existing row, missing prev, no unigram table. The augmented delta
  test asserts numerator *and* denominator merge (the blended cost
  strictly cheapens). `nbest_user_only_pair_produces_a_blended_step`
  asserts a prev with no system row blends over the user total.
  `nbest_user_successor_on_existing_row_blends_over_merged_total` is the
  你→浩 shape: 你's system row exists, the successor is count-0 in it,
  and the blended denominator is `system_row_total + user_total`, not
  the user total alone. Both user-trained shapes undercut the
  unigram-only branch; the system-only answer has no blended step.
- C ABI (`tools/bisection/run-nbest-train-diff.sh`, matched model20
  tables, pin-built oracle): the runner now diffs the **full logs** —
  probe surfaces included, not just the export triples. That widening is
  load-bearing: with the `count_delta` forward dropped, the export
  triples stay identical (training still lands the same user state
  through the NORMAL candidate; the seam moves row *ranking*, not the
  recorded path) while the user-only probe stops flipping — ours stayed
  你好/你好/你好 (n=126) against the oracle's 你浩/你好/你浩 (n=127),
  runner exit 2. Merged: full logs byte-identical, baseline rows
  你好×3 flipping to 你浩/你好/你浩 after (你→浩)×3, with 你浩 growing
  138 → 414 → 1242 across the rounds on both engines.
- The §8-recorded offset-decode divergence (our mid-train rows covered
  only the remaining input, the oracle's the full context) did not
  surface in the compared logs: it lives in the unchecked mid-train
  decode whose return value the driver deliberately ignores. Fixed in
  §10.
- Pins re-measured on this branch: default candidates 10177 / 10189 /
  94871 of 98930 / absent 1 / tie-swaps 1036; sentence surface
  488/385/370 — bit-identical. union / train / import / predict diffs
  green; fmt, clippy `-D warnings`, workspace tests green.

## 10. Offset-decode prefix context, landed

Date: 2026-08-21 · branch `claude/offset-decode-context-xdrlsb`.

**The gap.** Upstream's `PhoneticLookup` runs over the full
`PhoneticKeyMatrix` with `CONSTRAINT_ONESTEP`/`diff_result` forcing
chosen tokens at prefix positions, so decoded sentences naturally carry
the full context (你好/你浩). The engine's `guess_sentence` rebuilds a
fresh `SegmentGraph` from `self.raw[self.consumed..]` only, producing
remaining-input-only rows (好/浩). Documented in §8 as "recorded, not
chased"; §9 confirmed it did not affect the compared logs.

**The change.**

- `Session::guess_sentence`: after computing n-best rows from the
  remaining input, prepends `self.selected` (the accumulated text of
  prior selections) to each row's text when `selected` is non-empty.
  The row's `span`, `tokens`, `keys`, and `cost` are unchanged — only
  the display text carries the prefix. The same call snapshots
  `self.history` into `nbest_history` beside the rows (cleared with
  them by the next lookup and by `reset`): the seed context the rows
  were decoded against.
- `Session::select_inner`: when selecting an n-best row candidate
  (identified by `nbest_row().is_some()`), assigns the candidate text
  to `self.selected` rather than appending, because the text already
  contains the prefix. Non-row candidates continue to append. The row
  selection also restores the `nbest_history` snapshot into
  `self.history` before extending it with the row's tokens — a normal
  selection made between the lookup and the row choice is replaced on
  the record exactly as its text is replaced by the assignment, so
  `selected` and the token record stay synchronized (the review
  follow-up to the text-side-only assignment).

**Evidence.**

- CAPI test `offset_decode_rows_carry_prefix_context`: parses "nihao",
  selects 你, calls `guess_sentence` on the remaining "hao", asserts
  `pinyin_get_sentence` returns a string starting with 你, and asserts
  every `NBEST_MATCH_CANDIDATE` in the candidate snapshot also starts
  with 你.
- Engine regression
  `a_row_choice_after_a_normal_selection_records_the_row_path_only`
  (tests/decoding.rs): a normal 你好 selection
  between the lookup and the row choice records its own token at that
  point, and the later row selection over "nihaozhongguo" must land on
  the same committed text and the same `selected_tokens` as a control
  run that chooses the row directly — pre-fix the record held the
  stale token plus the row path ([11, 11, 33] for [11, 33]).
- Pins unchanged: default candidates 10178 / 10190 / 94872 of 98930 /
  absent 0; sentence surface 488/385/370 — bit-identical. fmt, clippy
  `-D warnings`, workspace tests green.

## 11. Class A candidate tie law ported — sentence pins re-measured

Date: 2026-08-22 · branch `feat/w12-class-a-comparator`.

The candidate comparator's frequency key became the pin's amplified
scale (`trunc(((1−λ)·(unigram+1)/total)·2²⁴)` in `f32`,
`pinyin.cpp:1855-1866`; the model20 item unigram is interpolation2
count + 1, total 51,051,831), and the window scan flushes each window
token-ascending — the array order the oracle's stable
`g_array_sort_with_data` keeps for comparator-0 pairs. The W12 Class A
candidate residual closed to zero (10,190 / 10,190 / 98,930 of 98,930 /
absent 0 / tie-swaps 0; `docs/findings/corpus-tail.md`,
`pin-refreeze-2026-08.md` third amendment).

**This surface:** `loses_to`, the trellis, and the n-best row machinery
are untouched — §3's trellis divergences stand. The measured agreement:

- row 0 (decoded 1-best): **488/496**, unchanged.
- full sentence lists: **385/496**, unchanged.
- first-6 candidate rows: **379/496**, up from 370 — the candidate list
  is the tail of those rows, so closing the candidate residual lifts
  exactly this figure; the remaining **117** (this line first read "17",
  an error — see §12) are the §3 trellis-side near-ties, not
  candidate-order divergence.

`sentence_surface_matches_the_declared_residual` holds 488/385/379. §12
defines what each of those three numbers measures — they are three comparison
strictnesses, not one measure — and records the residual; whether to freeze it
as a permanent Stage-1 divergence is the maintainer's decision, pending
approval.

## 12. The residual, defined and measured — a Stage-1 divergence pending a freeze decision

Date: 2026-08-24 · branch `feat/gfloat-accumulation-parity`

The `488 / 385 / 379` of §5/§11 are **three comparison strictnesses over one
set of 496 comparable inputs** (the 7 junk-leading inputs the oracle cannot
parse are excluded — no surface on either side), not one measure at three
surfaces. Named the way `corpus-tail.md` separates top-1 / top-5-set /
order-only:

- **1-best agreement — 488/496.** `pinyin_get_sentence(0)` equal: the decoded
  top sentence.
- **n-best distinct-set agreement — 385/496.** The set of decoded sentences
  `get_sentence(0..=proven)`, **order- and duplicate-insensitive**. This is
  the number §5/§11 printed as "full sentence lists".
- **n-best ordered-list agreement — 379/496**, which coincides with
  **first-6 candidate rows — 379/496.** The full *ordered* `get_sentence`
  vector, and the ordered `(type, nbest, text)` rows; they move together
  because the rows are those sentences prepended.

So `385` is the *distinct-set* number and `379` the *ordered* number; §5/§11
set `385` ("full sentence lists") beside the ordered `379` without saying they
measure differently. **This is a reinterpretation of the definitions, not just
a clarification** — "full sentence lists" here now means the order- and
duplicate-insensitive *distinct-set*, and the strictly-ordered list is the
separate `379`. Text elsewhere that reads `385` as an ordered-list agreement
is reading the old, conflated definition. The **"17" in §11 was an error**:
the ordered / first-6 residual is **117**, and every one is trellis-side. The
read-only `sentence-tail` binary finds **0 candidate-surface leaks** — the 83
rows it labels "phrase-window" are the phrase slice shifting as the surviving-
NBEST-row count differs (an intact phrase order seen through a shifted 6-row
window; `guanyo` the exemplar), and the candidate surface is bit-identical on
the same tables (`corpus-tail`: 10190/10190, absent 0, order-only 0,
prefix-10 gap 0).

Reproduce, read-only, no oracle FFI (the fixture holds the pin's answer):

```bash
# PINYIN_MODEL_DIR must be a COMPLETE extracted model20 (all 18 files: the 17
# phrase tables plus interpolation2.text). locate_model_dir rejects a partial
# dir such as the 4-file ~/.cache/oxpinyin-data, so point it at a full extract.
PINYIN_EXPORT_DIR=<exported redb> PINYIN_MODEL_DIR=<complete extracted model20> \
  cargo run -p pinyin-oracle --release --bin sentence-tail
```

The same measurement is the gate: the integration test
`sentence_surface_parity` (`crates/pinyin-oracle/tests/`) calls the shared
`pinyin_oracle::sentence_tail::measure` and asserts `488 / 385 / 379` plus the
residual invariants (`0` order-only, the `6` distinct-same). It self-skips
when the tables are absent — the same self-skip the real-tables tier uses — so
`cargo test --workspace` stays green without them, and fails loudly for anyone
who runs it with them if the numbers move. A move is a deliberate re-freeze of
this section, not a silent drift.

**Where the 117 live.** First divergence at row 0: 8, row 1: 83, row 2: 26 (a
shorter list first missing a rank counts at that rank);
**0 order-only** (no case is one list reordered); **6 distinct-same** (exactly
the `385 − 379` gap — e.g. `tuihui`, oracle `[退回, 退回, 退会]` vs port
`[退回, 退会]`: the same two sentences, a duplicate path at rank 1 on one side
only). The residual is **hypothesis selection**, not display order: port and
pin keep *different* 1st/2nd/3rd survivors in the top-3, the 8 row-0 misses
being a different global best (`跑錶` vs `炮表`, `杂拴帕清` vs `杂树安帕清`).

**Why it is not portable — the Phase-1 gate, now confirmed by the dump.** The
selection runs on `gfloat m_poss`, accumulated `m_poss += log(...)` per step
with a round-to-`f32` at every node (`$S/src/lookup/phonetic_lookup.h:663,
692`), and compared throughout — node store, beam-32, tail-3 — by exact-float
`trellis_value_less_than` (`:66-91`); the final tail sort truncates the float
difference to `gint`, so poss within 1.0 nat ties and keeps heap-pop order
(`trellis_value_compare`, `:174-178`). Matching any of the three strictnesses
means reproducing those `gfloat` values, which means a floating-point natural
`log` per step. `f64::ln` delegates to the platform libm with no
cross-platform bit-exactness guarantee; the build forbids `-march=native` for
exactly this reason; constitution item 6 requires output to be a pure
function of (input, user state, config) on every OS; and
`crates/oxpinyin-core/src/cost.rs:11-15` records this as the reason the cost
scale is integer fixed-point at all. The float dependency reaches the
tiebreak too — heap-pop order is seeded by the exact-float comparator — so no
fixed-point transform recovers the order either.

This is **not** the Class A species. Class A's `amplified_frequency`
(`crates/oxpinyin-engine/src/session.rs:1835-1841`) is
`(1−λ)·unigram/total·2²⁴` — multiply, divide, cast: IEEE-754 basic ops,
correctly rounded and bit-identical on every platform, which is why it ported
to 100%. The `gfloat` cost adds a transcendental. And the surfaces do not
share the arithmetic: candidates rank on `amplified_frequency` → `RankKey`,
the trellis on `nbest_step_costs` → `surprisal` — so this divergence is
contained to the sentence tails and costs nothing on the candidate surface.

**Recommendation, pending maintainer approval.** `488 / 385 / 379` is the
measured Stage-1 sentence-surface residual — one named, understood divergence:
the well-defined fixed-point behaviour where upstream's is platform-dependent
and unreproducible, the same shape as the aux over-read and `_check_offset`.
The recommendation is to accept it as a permanent divergence and rely on the
report-back entry in `upstream-divergences.md`; **freezing it as permanent is
the maintainer's call, not taken here.** It would be revisited only under a
deliberate pin re-freeze that accepts platform-locked floating point, which
the constitution does not permit.
