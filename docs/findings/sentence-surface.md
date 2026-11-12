# Sentence surface (W14)

Date: 2026-08-17 · Status: **machine-read**. This file records how the
pin builds sentence rows, what oxpinyin must reproduce, and the current
measured residual.

- §1–§11 state the current sentence behaviour and the rules that apply
  to it. Older measurements in them are one-line recorded evidence.
- §12 and its subsections hold the measured residual. The current
  declaration is the `#1378 current sentence-selection residuals`
  subsection of §12 and the subsections after it. The §12 text before
  it is dated measurement.
- The 500-row gate is 496 / 492 / 491. The supplementary fixture has
  8,436 pin-equal inputs and 156 exact ruled selection residuals.

The pin source citations in §1–§2 are against the libpinyin 2.11.91
`0c5e80e` tree, the pin at freeze, at
`target/oracle-pin-build/src/libpinyin-2.11.91/` (`$S` below). Some line
numbers differ at the current pin, `074a2219`. With no n-best row, `pinyin_get_sentence` answers `false`
and leaves `*sentence` untouched, as the pin does (`pinyin.cpp:1470-1471`;
register row 53 in `compatibility-policy.md`).

## 1. Where the rows come from

The pin has no `collect_sentence`. The sentence surface is the n-best
lookup plus two passes at the end of `pinyin_guess_candidates`:

- `pinyin_guess_sentence` (`$S/src/pinyin.cpp:1373-1385`) resets
  `m_prefixes` to `[sentence_start]` and calls
  `PhoneticLookup<2, 3>::get_nbest_match`. The call fills
  `instance->m_nbest_results` (`pinyin.cpp:89`).
- `pinyin_guess_candidates` sorts the phrase candidates
  (`pinyin.cpp:2284-2286`). When the sort word does not carry
  `SORT_WITHOUT_SENTENCE_CANDIDATE` (0x1, `pinyin.h:56-58`), it then calls
  `_prepend_sentence_candidates` (`pinyin.cpp:2292-2293`).
- `_prepend_sentence_candidates` (`pinyin.cpp:1934-1951`) prepends one
  `NBEST_MATCH_CANDIDATE` per n-best result **in reverse index order**.
  N-best index 0 (the 1-best) is therefore at the head of the list, before
  every phrase candidate. Zero results give no rows.
- `_compute_phrase_strings_of_items` sets the display string of each row
  with `pinyin_get_sentence(instance, candidate->m_nbest_index, ...)`
  (`pinyin.cpp:2004-2009`).
- `_remove_duplicated_items_by_phrase_string` (`pinyin.cpp:2058-2160`)
  deduplicates the whole list by display string, and the NBEST row wins.
  A phrase candidate with the same string as an NBEST row becomes
  `ZOMBIE_CANDIDATE` and is removed (`pinyin.cpp:2116-2126`). Of two NBEST
  rows with the same string, the lower index stays (`pinyin.cpp:2103-2114`).

**Gating.** The parity sort word `SORT_BY_PHRASE_LENGTH_AND_PINYIN_LENGTH_
AND_FREQUENCY` = 0x1e (`pinyin.h:66-69`) does **not** carry
`SORT_WITHOUT_SENTENCE_CANDIDATE`. Rows therefore appear exactly when
`m_nbest_results` is non-empty, that is, when `pinyin_guess_sentence` ran
after the last reset. Only `pinyin_reset` clears it (`pinyin.cpp:2698`).
The frozen corpus profile never calls `pinyin_guess_sentence` (the
capture's `observe_without_reset` → `collect_candidates`). For this
reason the W4 candidate fixture and the pin suite exclude sentence rows:
**a pin move when rows are correctly gated behind `guess_sentence` is a
leak**.

**Recorded evidence** (live probe over the model20 tables, empty user
store, ibus call order `guess_sentence` → `guess_candidates(0, 0x1e)`):

- `nihao` → sentences `[你好, 你好, 你好]`, candidate total 126. The three
  rows become one (the lower index wins) and the phrase `你好` becomes a
  zombie, so the total equals the phrase-only total.
- `nihaoshijie` → `[你好世界, 你好时节, 你好是届]`, total 129 = 126 + 3.
- `cecenihao` → `[测测你好, 测测你号, 测测你好]`. Rows 0 and 1 stay under
  the NBEST-against-NBEST rule; row 2 becomes a zombie.
- `zhongguoren` → `[中国人, 中国人, 中国任]`. The rows that stay carry
  n-best indices 0 and 2: the index is the *original* tail rank, not a
  new number.

## 2. The n-best lookup itself

The pin runs `PhoneticLookup<nstore = 2, nbest = 3>`
(`$S/src/pinyin.cpp:55,406`) over the full `PhoneticKeyMatrix`:

- Trellis steps are matrix columns. A node at a step is keyed by the
  phrase token that **ends** there. Each node keeps the best `nstore = 2`
  values (`insert_candidate`, `phonetic_lookup.h:311-330`; the heap store
  is `phonetic_lookup_heap.h`).
- At each step the beam keeps the best `nbeam = 32` values across all
  nodes (`phonetic_lookup.h:37`, `771`). The order is
  `trellis_value_less_than` (`phonetic_lookup.h:66-87`): a longer sentence
  wins, unless a shorter one has an accumulated cost better by more than
  `LONG_SENTENCE_PENALTY = log(1.2)` (`phonetic_lookup.h:39`). At equal
  sentence length the lower cost wins.
- Span search widens `m` from `i + 1` until the pinyin table search stops
  returning `SEARCH_CONTINUED` (`phonetic_lookup.h:800-827`). The engine
  window scan ports the same prefix-driven widening.
- For each span, `search_bigram2` expands **all** beam values over the
  merged (system+user) bigram successors of their last token
  (`phonetic_lookup.h:572-620`). `search_unigram2` expands **only the
  first beam value** over every token that the span spells
  (`phonetic_lookup.h:539-542`, `552-570`).
- Step cost (`phonetic_lookup.h:628-676`):
  - bigram branch: `log((λ·bigram_poss + (1−λ)·unigram_poss) · pinyin_poss)`;
  - unigram branch: `log(unigram_poss · pinyin_poss · (1−λ))`;
  - `unigram_poss = phrase_unigram / phrase_index_total`;
  - `pinyin_poss = compute_pronunciation_possibility`
    (`phonetic_key_matrix.cpp:587-601` at `074a2219`): the matched pronunciation
    frequency over the total pronunciation frequency of the phrase item,
    summed over the matrix paths of the span (polyphone discount).
- Tails are the top `nbest = 3` values at the last step, sorted best-first
  (`get_tails`, `phonetic_lookup.h:330-341`; the sort is
  `trellis_value_compare`, `phonetic_lookup.h:174-177`). Each tail is
  backtracked into a `MatchResult` token array (`extract_result`,
  `phonetic_lookup.h:369-396`).

`pinyin_get_sentence` (`pinyin.cpp:1463-1482`) returns
`convert_to_utf8(phrase_index, result, NULL, false)`. This is the decoded
n-best row at `index`, rendered through the phrase index
(`lookup.cpp:27-70`). It answers only when an active sentence lookup has a
result at `index < m_nbest_results.size()`; otherwise it returns `false`.
Index 0 is the 1-best. When no n-best row exists, including before any
lookup, `pinyin_get_sentence` returns `false` and leaves the output
untouched. The raw scheme keystroke buffer or session preedit is never an
answer.

`pinyin_choose_candidate` on an NBEST row (`pinyin.cpp:2511-2519`) diffs
`constraints` between result 0 and the chosen index and returns
`matrix.size() - 1`. Choosing a sentence selects the whole input.

## 3. What the engine must reproduce

1. Rows appear only after `guess_sentence` (the `m_nbest_results` gate).
   There are at most 3 rows, in tail order; the index is visible through
   `pinyin_get_candidate_nbest_index`. The NBEST row wins the merge
   against phrase candidates, and the lower index wins among rows.
2. `pinyin_get_sentence` returns the decoded n-best rows (index 0 is the
   1-best) and `false` past the row count.
3. The trellis order: the length-preferring comparator with the log(1.2)
   penalty, beam 32, two stores per (step, token), unigram expansion only
   from the beam head, and λ-blended bigram/unigram costs on the same
   counts that the engine's `LanguageModel` exposes.

Current scoring and selection:

- **Float accumulation.** Real-table scoring uses f32 accumulation of std
  `f64::ln` and the pin's pre-log epsilon gates (#1378 PR B, maintainer
  approval 2026-10-29 UTC). Register row 11 of `compatibility-policy.md`
  carries the classification.
- **Selection.** The beam, node and tail order follow the S1/S2/S3 repairs
  of the
  [algorithm-precondition rules](compatibility-policy.md#algorithm-precondition-violations).
  §12 "Lane D selection port" records the ported heap and merge order.
  No Rust sort takes the cyclic trellis comparator.
- **Pronunciation.** `pinyin_poss` takes the share of the looked-up
  spelling once per span: the first matrix path per token wins. It does
  not sum the possibility over every matrix path of one span, as
  `compute_pronunciation_possibility` does. This divergence is recorded,
  not chased. The per-pronunciation counts are carried, so the polyphone
  discount applies.
- **Constraints.** The constraint machinery (`CONSTRAINT_ONESTEP`,
  `diff_result`) is ported: the store in `constraint.rs`, the gated
  full-matrix walk, the reset split, the constraint-aware train and
  `pinyin_clear_constraint`. The remaining-input re-seed (the W6 surface)
  still carries every empty-store decode, which is the shape of the frozen
  pins. On that path the §10 prefix context is the display rule.

## 4. Implementation record

- `LanguageModel::nbest_step_costs` gives the two pre-log branches of §2
  for one step. It is a defaulted method of the `LanguageModel` trait in
  `crates/oxpinyin-core/src/lib.rs`. The default returns no cost data, so a model that
  keeps the default produces no rows. The blended branch answers only
  when the previous token's bigram row carries the successor (count > 0),
  as `search_bigram2` walks merged-gram successors.
- `PhraseEntry` carries `pronunciation_possibility (matched, total)`.
  `SystemDictionary::lookup` fills it from the pinyin-index record's
  frequency (the matched share) and the aggregated unigram map (the
  pronunciation total of the item). These are exactly the
  per-pronunciation counts that `pinyin_iterator_get_next_phrase` reports
  (`pinyin.cpp:698-743`). A matched share of 0 skips the step. The
  candidate scan never reads the field.
- Engine: the `nbest.rs` trellis and `Session::guess_sentence` /
  `sentence_text`. `refresh` prepends the stored rows and merges them by
  the §1 rules. `reset` clears the rows, as `pinyin_reset` does.
  `Candidate::nbest_index` carries the tail rank.
- A chosen row is identified by its own tail rank through
  `Candidate::nbest_row()` (`Option<u8>`), never by list position (§8).
  `None` marks every non-row candidate, so a fallback sentence candidate
  records nothing. Choosing a row records **all** of its token path, as
  `pinyin_train` walks the chosen `MatchResult`: `Session::select` extends
  `history` with the tokens of the row.
- C ABI: `pinyin_guess_sentence` fills the rows and marks the lookup
  active (`Session::sentence_lookup_active`; only `reset` clears it).
  While a lookup is active, `pinyin_get_sentence` answers the decoded row
  or `false`, never the raw form, also when the lookup produced no rows.
  The pin's `get_nbest_match` clears `m_nbest_results` before every
  attempt, so its `0 == results.size()` false covers that case too.
  Before a lookup the call also returns `false` and leaves the output
  untouched. `pinyin_get_candidate_nbest_index` reports the tail rank.
  `pinyin_guess_candidates` honours `SORT_WITHOUT_SENTENCE_CANDIDATE` and
  excludes sentence candidates.

## 5. Measured agreement

`fixtures/w4/oracle-sentence-surface.txt` freezes the pin's surface for a
deterministic W2 sample: every 20th corpus input plus anchors such as
`nihao`, `nihaoshijie`, `cecenihao` and `zhongguoren`. It now has 507
rows and 500 comparable inputs (§12 "G-m1 coverage extension"). The 7
junk-leading inputs that the pin does not parse are excluded; neither
side has a sentence surface for them. The fixture has no regenerator
today ([goldens and pins](../runbooks/goldens-and-pins.md)).

The gate `sentence_surface_parity` checks these rules on every
comparable input:

- NBEST rows are at the head in tail order, `n/0` first.
- No sentence candidate appears before `guess_sentence`. This gating
  keeps the W4 corpus pins frozen.
- The `guess_sentence` return value agrees.

§12 defines the three agreement counts and holds the current numbers.

Recorded evidence: the first W14 measurement gave 488 / 385 / 370 of 496
(decoded 1-best / full sentence lists / first-6 candidate rows). §12
defines these three counts. Before the pronunciation discount, a
200-input probe gave 74.5% on the decoded 1-best, so the polyphone term
is a required scoring input.

## 6. W13 scheme re-measure — the C ABI n-best wiring gap

Recorded evidence (2026-08-19, main `489e94d`): through the C ABI,
`pinyin_guess_sentence` returned `true` but produced no rows on every
scheme and on full pinyin. The cause was that `SharedLm` did not override
`nbest_step_costs`, so the trellis had no cost data. §7 closes the gap.

The current behaviour is in §4: a `LanguageModel` that keeps the trait
default of `nbest_step_costs` produces no rows.

## 7. The C ABI wiring gap, closed

`SharedLm` (a re-export of `oxpinyin_runtime::RuntimeLm`) overrides
`nbest_step_costs` and forwards to the model behind it, the same model
that the rest of the C ABI decode scores against. The trellis, the
comparator and the candidate code did not change for this. The trait
default stays for other implementors.

Recorded evidence: a one-input C ABI probe (full-pinyin `nihao`, model20
tables) gave `guess_sentence: true`, `get_sentence(0): 你好`, `cand[0]`
`NBEST_MATCH` with n-best index 0 and 126 candidates, byte-identical to
the pin. With matched unigram data and the exact scheme-key seam
(`docs/findings/bopomofo-spec.md` § exact seam), the double-pinyin and
STANDARD bopomofo differentials run IDENTICAL.

## 8. The selection record's row mapping, fixed

**Rule.** The selection record finds a chosen n-best row by its tail
rank, never by its list position. The NBEST-wins deduplication keeps the
lower-index row when two rows have the same string. A row's list position
therefore equals its rank only while no earlier row was dropped.

Example: rows `[好, 好, 浩]` show 好 (rank 0) at position 0 and 浩 (rank 2)
at position 1. A positional record of 浩 records the tokens of 好, and
`pinyin_train` then writes 你→好 instead of 你→浩. The pin does not have
this problem: `pinyin_choose_candidate` keeps the chosen `MatchResult` on
the instance (`pinyin.cpp:2511-2519`), and train walks that result.

**Implementation.** `Candidate` stores `Some(rank)` on row-origin
candidates only and `None` on every other candidate, including fallback
sentence candidates. `Candidate::nbest_index()` still answers `u8` for
the C ABI (`pinyin_get_candidate_nbest_index`). `Candidate::nbest_row()`
is the origin marker that the selection record reads. A row-origin
candidate looks up the stored row at its rank, bounded by `get`.

**Evidence.**

- Engine test `a_shifted_row_records_its_own_rank_not_its_position`
  builds rows `[好, 好, 浩]`. It asserts that the 浩 row is at position 1
  with rank 2 and that it records the tokens of 浩.
- Recorded evidence (n-best training differential, matched model20
  tables): (你→浩) × 3 exports `你浩|ni'hao|1242` + `你好|ni'hao|1242`,
  identical to the pin. 你浩 grows 138 → 414 → 1242 across the rounds.
  Without the §7 forward the C ABI emits no rows and the run is vacuous;
  the runner prints a vacuity warning in that shape.

## 9. User-merged n-best step costs, landed

**Rule.** The n-best step costs merge user counts into **both** branches,
with merged denominators, before the observed-successor gate. This is
the same user-aware lookup that `score` uses, as the pin applies
`merge_single_gram` before the presence gate (`ngram.cpp:277`). The user
overlay is `user-store.md` §5.

**Implementation.**

- `BigramLanguageModel::nbest_step_costs_with_user_delta(prev, token,
  user)` computes both branches over the merged counts:
  - the unigram term over `system + user.unigram_delta` /
    `system_total + user.unigram_total_delta`;
  - the blended branch over `merged_transition` (system load, then
    `merge_bigram`), merged *before* the count > 0 presence gate.
- A user-trained successor with no system count therefore blends and
  does not fall through to the unigram branch. There are two miss shapes:
  - a prev with no system row: the denominator is the user total;
  - a prev with a system row that lacks this next token (the 你→浩 shape):
    the denominator is `system_row_total + user_total`.
- The trait method delegates with `UserCountDelta::ZERO`.
- `SharedLm::nbest_step_costs` takes the same user delta that
  `score` takes and forwards it.

Recorded evidence (n-best training differential over full logs, matched
model20 tables): after (你→浩) × 3 the rows change from 你好 × 3 to
你浩/你好/你浩 on both engines. Without the user delta, the export lines
stay identical but the user-only probe stays 你好/你好/你好 (n=126)
against the pin's 你浩/你好/你浩 (n=127). The full-log comparison detects
this defect; the export lines alone do not.

## 10. Offset-decode prefix context, landed

**Rule.** A sentence row decoded after earlier selections displays the
full context (你好/你浩), as the pin does. The pin's `PhoneticLookup` runs
over the full `PhoneticKeyMatrix` with `CONSTRAINT_ONESTEP` /
`diff_result` forcing the chosen tokens at prefix positions.

**Implementation.**

- `Session::guess_sentence` computes the n-best rows from the remaining
  input. When the selected text is not empty, it prepends that text to
  the text of each row. The span, tokens, keys and cost of the row do not
  change; only the display text carries the prefix.
- The same call keeps a snapshot of the selection history beside the
  rows: the seed context that the rows were decoded against. The next
  lookup and `reset` clear it with the rows.
- When the user selects an n-best row (`nbest_row().is_some()`),
  `Session::select_target` assigns the candidate text to `self.selected`
  and does not append it, because the text already contains the prefix.
  Other candidates append. The row selection also restores the history
  snapshot before it extends the history with the tokens of the row. A
  normal selection made between the lookup and the row choice is thus
  replaced in the record, as its text is replaced. The selected text and
  the token record stay synchronized.

## 11. Class A candidate tie law ported — sentence pins re-measured

The candidate comparator and the trellis do not share arithmetic.
Candidates rank on the amplified frequency key
(`trunc(((1−λ)·(unigram+1)/total)·2²⁴)` in `f32`, `pinyin.cpp:1855-1866`)
→ `RankKey`. The trellis ranks on `nbest_step_costs`. A change to the
candidate tie law therefore does not change the sentence selection.

The first-6 candidate-row count can still move with the candidate
surface, because the candidate list follows the sentence rows in those
six rows.

Recorded evidence (2026-08-22): the Class A port changed only the
first-6 candidate-row count, from 370 to 379 of 496.

## 12. The residual, defined and measured — FROZEN as a permanent Stage-1 divergence

Date: 2026-08-24 · branch `feat/gfloat-accumulation-parity`

The `488 / 385 / 379` of §5/§11 are **three comparison strictnesses over one
set of 496 comparable inputs** (the 7 junk-leading inputs the oracle cannot
parse are excluded — no surface on either side), not one measure at three
surfaces. Named the way candidate-construction §8.2 separates top-1 / top-5-set /
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
the same tables (recorded evidence: 10190/10190, absent 0, order-only 0,
prefix-10 gap 0).

The measurement is read-only and runs no pin: the fixture holds the pin's
answer. The gate is the integration test `sentence_surface_parity`
(`crates/oxpinyin-runtime/tests/`), which calls
`sentence_tail::measure` over the pinned libpinyin-data package
(`just fetch-contract-data`) and asserts the current numbers (see
[the current declaration](#1378-current-sentence-selection-residuals)).
A missing package is a failure. A move is a deliberate re-freeze of this
section, not a silent drift. At the time of this subsection the gate held
`488 / 385 / 379` plus the residual invariants (`0` order-only, the `6`
distinct-same).

**Where the 117 live.** First divergence at row 0: 8, row 1: 83, row 2: 26 (a
shorter list first missing a rank counts at that rank);
**0 order-only** (no case is one list reordered); **6 distinct-same** (exactly
the `385 − 379` gap — e.g. `tuihui`, oracle `[退回, 退回, 退会]` vs port
`[退回, 退会]`: the same two sentences, a duplicate path at rank 1 on one side
only). The residual is **hypothesis selection**, not display order: port and
pin keep *different* 1st/2nd/3rd survivors in the top-3, the 8 row-0 misses
being a different global best (`跑錶` vs `炮表`, `杂拴帕清` vs `杂树安帕清`).

**`tuihui` note (2026-09-19 UTC).** The example above was measured over
exported model20 tables against `fixtures/w4/oracle-sentence-surface.txt`.
On the pin's own `data/` (same-dir ABI dump, `probe-coverage-abi.md` A)
the port emits `[退回, 退回, 退会]`, matching the pin — the duplicate-
text path is among the survivors.

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
(`amplified_frequency`, `crates/oxpinyin-engine/src/session/mod.rs`) is
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

**Ruling (maintainer, 2026-09-02): FROZEN as a permanent Stage-1
divergence.** The recommendation above is accepted. The residual is one named,
understood mechanism — the pin's platform-dependent `gfloat` accumulation
against this port's defined fixed-point arithmetic — and bit-exact
reproduction is ruled out by constitution item 6, so `488 / 385 / 379` is a
recorded constant, not a target of zero. The disposition is the same one the
predicted-candidate tie order took under the 2026-08-25 ruling: upstream
deterministic but not reproducibly so. Two consequences, stated explicitly:

1. The gate `sentence_surface_matches_the_declared_residual` asserts a
   **defined-residual**, not a parity target: it holds the frozen numbers and
   the mechanism invariants (`0` order-only, the `6` distinct-same), and any
   move remains a deliberate re-freeze of this section, never a silent drift.
2. Row 11 of `compatibility-policy.md` carries the classification.

**Re-opening condition:** a deliberate pin re-freeze that itself accepts
platform-locked floating point — which the constitution does not permit for
this project — or a measured leak of the residual onto the candidate surface
(today's measurement shows 0 leaks; the candidate surface is bit-identical).

### §12 re-freeze — 2026-09-04

Re-measured at tip `4eb72efd` by `sentence_tail::measure`
(backend-independent, reproduced at the freeze ref and P6 bracket). The
entire move is `345af16` (P6 — run the production engine on libpinyin's
own data): the sentence trellis's P_unigram switched from the
interpolation2.text-derived construction (UNKNOWN_COST floor) to
PhraseItem::get_unigram_frequency (count + gen_unigram + 1) / total,
1/Σ floor. The two constructions differ by ~1 part in 5×10⁷ on the
pinned model — sufficient to flip exact-comparator near-tie resolution
for three inputs (`paobiao`, `lisuancengmenlutingmouchangdielvejin`,
`ran'hua'zhui'hu'guo'zhan'zhen'hei`). P2–P5 affirmatively ruled out by
bracket; freeze ref `181bd7a` reproduced exactly. Ordered-miss residual:
117 → 106 (breakdown 8→5 / 83→81 / 26→20); 11 inputs fixed, 0
regressed (the inputs missing at the tip are a strict subset of those
missing at the freeze).
Mechanism invariants held: guessed_disagree 0, list_order_only 0,
list_distinct_extra 6. New frozen numbers: **491 / 396 / 390**.
Maintainer ruling: 2026-09-04.

### Lane D selection port — 2026-10-04 UTC (#535, #594, #574)

Seven selection differences are ported together against libpinyin 2.11.92
`074a2219`: retention and physical heap slot order
(`src/lookup/phonetic_lookup_heap.h:25–29,56–81`), unreduced library/range/item
stream, nstore-gated comparator (`phonetic_lookup.h:66–91`), heap-pop beam
and tails (`:272–297,329–339`), fixed-final-step tails (`:825–837`), and
final `trellis_value_compare` gint truncation (`:174–178`).
`g_ptr_array_sort` calls GLib's stable `g_sort_array` merge
(`gqsort.c:msort_with_tmp`, split n/2, left on <= 0).
The Rust port uses hand-written libstdc++ push/pop/make-heap assignments
and that merge; no Rust sort takes the cyclic trellis comparator.
The remaining generation sort orders integer library identifiers only.
See `crates/oxpinyin-engine/src/nbest.rs` (`loses_to`, `heap_push`,
`heap_adjust`, `heap_pop`, `heap_make`, `tail_compare`, `tail_merge`,
`GenerationView`) and `session/guess.rs` (private separator provenance).

The fixture has **503 data rows**, despite sample=504, and 496 comparable
inputs. The approved port gate is **495 / 495 / 495**, one fixture
surface difference. The full corpus residual is **14 / 10,465**, versus
main's 2,185 differences, with zero regressions among main-matching inputs.
All 14 declared #594 cases and 36 Luoma/secondary-zhuyin cases close:
**129 / 129** assembly cases matched at the port (the suite has since grown; register row 11 carries the current count). Canonical formatting carries its
separator provenance through the temporary
`Session::replace_raw_with_physical_separators` intake; ordinary lookup,
matrix construction, ScanKey and the exact graph are unchanged.
The intake is expected to disappear with #585/#626.

Only the following six shared-comparison rows are class (a), as ruled.
These are the Phase-1 captures at the deciding comparison, not attributions
inferred merely from a float call graph. Pin costs differ in float bits;
the subject retains its native fixed-point costs.

| Input | Pin deciding f32 bits | Subject native costs |
|---|---|---|
| bo | c110d58e / c110d429 | 13059 / 13059 |
| quguan | c171e63c / c171e5cd | 21812 / 21812 |
| kaliantai | c1c7778d / c1c7769d | 35970 / 35970 |
| huilianpei | c13f886e / c13f8796 | 17270 / 17270 |
| yunhoulvlvlunqianaonaoruoqubeiji | c2a2cba9 / c2a2cba1 | 117432 / 117433 |
| nou'y | c193a149 / c193a09f | 26623 / 26623 |

The other eight remain **unattributed**, by name:
`pang'rao'xiane'qing'lo'sheiting'suanzhi`,
`nvchi'chui'lianai'baobeng'fang`, `bu'qing'na'wanou'panyao'you`,
`bianrang'lianai'seng'sunguo`, `'`, `''`, `'''`, and `'ni`.
The first four had different generation counts (pin/subject:
3534/1462, 2583/1165, 2510/2062, 2307/882); the cause is attributed
below (#639) and they now match. The four separator rows pre-exist the
port. No arithmetic attribution is claimed for these.

Recorded evidence: `sentence_surface_parity`, the full 10,465-input C-ABI
corpus enumeration and the candidate-assembly differential over pin-built
data, plus ZIGUANG and the 卧 selection replay. The per-cell PR tables
record the results. Bulk comparison captures were not retained.
The #574 coverage member corrects the sample header and adds four inputs
with native-cost assertions, detecting a +1 per-step mutation even when
the visible sentence surface does not move.

### G-m1 coverage extension — 2026-10-04 UTC (#574)

The historical sample=504 header described 503 data rows. Four pin-generated
rows (yaomeichong, xiehenshuaitong, nuanmanqianzhaofang, nic), identical on
bdb/kc/tkrzw, extend the fixture to **507 rows / 500 comparable inputs**.
The surface gate is **499 / 499 / 499**. A companion ignored integration
test in sentence_surface_parity.rs asserts native first-sentence costs
32746, 46792, 59752 and 17763, respectively. Adding one cost unit per
phrase expansion must fail; reverting it must pass. These assertions
cover the four recovered G-m1 inputs even if the port's retention changes
which visible rows the mutation alters. This extension changes coverage,
not scoring or selection. The companion test is
`sentence_surface_detects_per_step_score_shift`, which now asserts the
f32 bits of each first-sentence log possibility. No mutable test hook ships.

These four inputs are anchors of the fixture. The old header count included
`#yaguai`, whose rendered capture is a comment to the reader; the corrected
header counts readable data rows and retains capture order. The fixture has
no regenerator today ([goldens and pins](../runbooks/goldens-and-pins.md)).

### Resplit after a separator — 2026-10-07 UTC (#639)

`resplit_additions` (`session/mod.rs`) rejected a pair whose first key
followed a separator, and would have measured the split from the key's
`from`, the apostrophe. The pin keys that first key at its own
`m_raw_begin`, after the apostrophe, and measures from there
(`phonetic_key_matrix.cpp:113-150`, pin 074a2219; `pinyin_parser2.cpp:282`).
`lia` in `chui'lianai` therefore never became `lian` + `ai`, and the four
rows above decoded 俩乃 / 下呢 / 纳瓦耨 instead of 恋爱 / 限额 / 那玩偶. The
guard is dropped and the split is measured from `syllable_start`; the
left half keeps the separator-riding span. Not a class (a), (b) or (c)
divergence: it was a defect and is repaired.

Measured on bdb with `sentence-tail` (debug): 1-best **500 / 500**, n-best
distinct-set **500 / 500**, ordered **500 / 500**, first-6 rows **500 / 500**,
guessed disagreements 0. The `sentence_surface_parity` expectations move
from 499 to 500 with this repair. The full 10,465-input corpus residual
(14 before) is not re-measured here. The `'`, `''`, `'''` and `'ni` rows
are unchanged.


### Fixed-final-step coverage — 2026-10-10 UTC (#594)

The fixed-final-step rule already landed in `55d861de`; this lane adds
regression coverage, with no selection or scoring change. Read from the
retained bdb oracle source checkout at the pin;
`src/lookup/phonetic_lookup.h` hashes to Git blob
`c092e7610cb0e3bdcc29394e6f7f1ba3515d3214`, equal to the committed blob.
The source law is
[`get_tails`:330–340](https://github.com/libpinyin/libpinyin/blob/074a2219c90feaf962d0d24f034514033ece5f99/src/lookup/phonetic_lookup.h#L330):
`size() - 1` → `get_candidates` (:272–297) → `get_top_results` (:143–171).
If that step has fewer than `nbest` values, the heap drains and returns
only those values. If it has none, `get_top_results` clears the output
and returns false; `get_tails` ignores that return, succeeds with an empty
list, and the extraction loop (:825–841) adds no results and returns true.
There is no earlier-step fallback. A zero-sized input matrix instead
returns false before clearing existing results (:743–748).

On the retained bdb oracle under Fedora 44, both facades match in **50/50**
`final-step-*` cases (recorded evidence): `ba'kua`, `li'shi`, `xi'an`,
`ni'hao`, `ba'ku`, `ni'h`, `nihao'`, `n`, `nih`, `ni`, `nihao`, `zhongguo`
at explicit `USE_TONE` (0x20) and `USE_TONE | PINYIN_INCOMPLETE` (0x28),
plus a fresh `罢跨/ba'kua/5` import on each facade. Complete ordered
sentence vectors retain duplicate texts; complete candidate lists retain
type/text and, for pinyin NBEST rows, their tail indices. Pinyin covers
sort words 0, 1, 0x1c, 0x1d, 0x1e, 0x1f. Zhuyin exposes one sentence and
no sort-word argument; its after-cursor candidate list is compared whole.

| `ba'kua`, explicit 0x20 | Indexed sentences, pin = subject | Displayed NBEST rows | Candidate count |
|---|---|---|---|
| pinyin, fresh | 把跨 / 把跨 / 把夸 | 把跨 (index 0), 把夸 (index 2) | 107 with sentences; 105 without |
| pinyin, imported | 罢跨 / 把跨 / 把跨 | 罢跨 (index 0), 把跨 (index 1) | 107 with sentences; 106 without |
| zhuyin, fresh | 把跨 | 把跨 | 106 |
| zhuyin, imported | 罢跨 | 罢跨 | 106 |

The retained oracle binary and stable/unstable data manifests verify
against `oracle-pin.txt` and both SHA-256 manifests. This capture has
three indexed pinyin results (including a duplicate), unlike the issue's
historical two indexed results; two displayed NBEST rows alone do not
establish an indexed count. No explanation for the historical count change
is inferred from this capture. There is no pin/subject difference here,
including no extra 把垮. These are controls against current main, which
already implements the rule, not claims of a new production repair.
The frozen fixture expectations are not re-measured by this scoped lane.

The 50 cases are recorded evidence from the contract differential of
that date. The engine guards cover an empty final step after three earlier
values on both shapes and a two-value final step after three earlier
values on the pinyin shape.

A revert-and-check that reinstated the earlier-populated-step selection
failed the empty-final-step assertion in `nbest::tests::tails_*`; both
guards passed after byte-for-byte source restoration. The 50 facade
cases also match under this single-rule mutation and again after restoration:
these current-model controls confirm parity; the direct engine guard supplies
the fallback-rule sensitivity. No wider differential was run.

### #1378 current sentence-selection residuals

Maintainer approval of 2026-10-29 UTC permits this re-measurement and the
corresponding frozen fixture/test updates. Recorded 2026-10-29T09:07:22Z (`date -u`).
The old six-row math declaration above is historical. Five now match the
pin (`bo`, `quguan`, `huilianpei`, `yunhoulvlvlunqianaonaoruoqubeiji`,
`nou'y`). `kaliantai` remains a selection residual, not class (a): both
current main and PR B choose `卡利安胎` at rank 2 where the pin chooses
`卡连太`, with ranks 0–1 identical. It is unchanged by the float scale.

The supplementary fixture contains 8,592 distinct inputs (8,744 corpus
occurrences). Current main `9015a178` has 163 actual pin differences;
PR B has 156, all byte-for-byte identical to main. Exactly seven surfaces
move, all to pin equality. The five legacy math-row closures include
`quguan` and `nou'y`, which already matched on current main. The seven
PR B movements are recorded in `float-scoring-1378`.

The remaining cases pass only against their complete exact subject surfaces
in `oracle-sentence-tie-order.txt`, with POLICY-ACCEPTED under the registered
S1/S2/S3 repairs. This does not create another precondition-violation site.
The source is the same beam, node and tail path described by the
2026-10-28 amendment: adjusted S and earlier batch ties in S1/S2, followed
by S3's stable descending raw score. The pin uses its pairwise length
predicate and truncated tail comparison. These current-main selection
residuals are not attributed to libm or silently called MATCH. Pin captures,
selected inputs, native selection metadata and existing pool entries are
preserved; only measured subject vectors change. No new PR B regression
is accepted by this table.

| Input | Disposition |
| --- | --- |
| `diu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `ri` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `seng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `pingshuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `baori` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `lianting` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `jiangtuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `ponuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `jiadiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `qiaxian` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `gediu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `chuanseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `shuanxiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `miaori` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `foudiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `jiangseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `heiri` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `luanseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `dianzhuang` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `binri` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zangdiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `pengnuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `naidiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `rengnuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `rangdiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `daizuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `potuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `maidiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `wairi` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `raori` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `xieaituan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `geizuiri` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tuanbeiseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `niuanshuannuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `baobangshuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zeniangsongdiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `songhaituan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `yingzuanzhou` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `xiongkenxiongtuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `xinzuiliuzuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `yuqiamenuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `jiangniemanuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `xikabendiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `kuishakuangdiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zashuanpaqing` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tapikaori` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `kaliantai` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `liaoqianuo` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `qianzhaokongceng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `senyangtuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `shehecairi` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `juedaozuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `xuanshuanpaoshan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zengongdiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zangnanuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tuangeidong` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tetianyun` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `chongfangzongdiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `shuanfousha` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `bangzhuaishuailozengbeilandiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `junshuiyunxianshoukuanshangniukeniu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `kuogaishaoniangtuanfu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `popashangqianguokuazhushuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `dianchangyoutengkuangliaocituan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `saibiaoshuanxingmudun` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `touzhuoguarennengailuchouyangkuidiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `nuanzuanduiguafa` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `funuanchaneiwudingoukuaipingceng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `caidunshaosongleisibaoruanxuanzuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `hunbiankuafengzanxiahangshuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `baishuangjianluzhuwawuwoyaoqinri` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `futaixieliangseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zhinaikaoniaozhengshuanlengerchangseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `shuangmiaozuanqingtawolaocucaogou` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `biraoguofenshenshuangxianrun` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tianchushugubanzunhahanyeshuangtuanfu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `qunruihuoshuisongchonggaohuainaoounvenuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tunshicehongshuanxiangxingmeng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `dupanggentuanming` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `difolandaochediu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `chanshuanguadiebaizezarengwangguigui` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `liekuairichazeng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `wujuebeiminchenshuanzonghao` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `cicangcuohuangheizahuangaiguodiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `mangmiaoerkaosawangliaomiunglengbieyu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `chuaiernaoshuansheguitao` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `benchehaishuannuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `diancuchuaijuanyuancui` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zhuainukuaitiluo` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zaicuituanshidie` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `junsantuoliaolan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `guanwaimushuanbenfanraokang` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `naduanriyannuanshoushuanshenzhouruisong` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `quanqiongbingaoreayuanfengshuanzetian` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `ranjieruipaodiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `paipieshunmouruogengaineshaihuannuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `maiduandouhadiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tanggoutaoshuandiukuanniecailanseque` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `liaonipuluosang` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `huanlingaojingkuncadiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `geiersichanshuandengruandanglangduoheian` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `paojuanmangaopadianzhaicenggehunguaimei` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `paimanqusengkasiqieguanchaihuichawa` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zhi'maofeng'reze'lun'lianzhuang'bin` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `qian'ce'tai'pan'han'yuanshuan'shunguai'tiechen` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tunen'shang'zaoyu'souzen'liecai'couling'ruhuan'diu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `fojin'haotie` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `pan'chuangga'xue'leng'katuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `menxia'sunchou'men'shuan'wan'bi'shuo'rao` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `qinglei'rengning'bu'shuan'li'maohuai'diaozou'er` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `sanxia'fang'chidiao'dai'taihuang'shun'youbing'pengdiu` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `rongkan'mu'nenlin'duo'zen'xiucou'pian'chuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zi'weng'pendian` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `pan'lian'a` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `peiku'tuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `jianshai'hengwai'sun'xianzui'rongnve'nurui'liaoka` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `ruo'cuo'hao'gunnuan'tao'tuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `fankai'ng'lieduan'se'kengri` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `hun'liao'zou'chuogang'cengtuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `suiman'zhi'lengtuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zha'shuo'he'luoao'chanzuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `ruanxun'shu'oulao'shuan'wangshuan'sengruo'tiao` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `pao'nenfou'shuannu'yuken` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `ga'yao'kongo'pa'longri` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `chou'mieshuan'pinpie'gengchang'jiang` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `fenbiao'jie'lang'pinxun'neinuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `fen'wo'hun'huochui'saoting'jile'gui'zuanbo` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `shuangluanheixion` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `luantia` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `huikangyuantia` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `goug` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `banrengou'gon` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tunshuanzengch` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tuanduanluanra` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `luotuanquans` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `leichel` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `piaonuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `piaotuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `bieyuemian` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `shengseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `gangnuochuangnuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `piexiannuochang` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `yangdianlingduan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `dangshuandie` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `zuoqiebang` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `xuanshuanhang` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `nvetiantuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `diajiongjuantuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `xingshuan` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `piaomiequanseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `kuangshuansao` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `hengshuaiseng` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `tingshuanduanhuo` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `seng)` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |
| `shenzuanjiao%` | POLICY-ACCEPTED, unchanged S1/S2/S3 selection |


### 500-input reconciliation (2026-11-01 UTC)

Recorded 2026-11-01T12:11:06Z (`date -u`), against main `bf41d2c0`.
The existing #1378 `sentence-tail` report supplies these nine complete
ordered-list differences; no new pin execution was used to discover them.
The first-best / distinct-set / ordered-list counts remain **496 / 492 / 491**.
Four first differ at rank 0, three at rank 1 and two at rank 2. None is a
permutation; `rongkan…` changes duplicate multiplicity while keeping the
distinct set.

Every row below is POLICY-ACCEPTED under an existing selection ruling,
not class (a). The seven named #1378 entries above bind the same complete
outputs in `oracle-sentence-tie-order.txt`. That fixture reads all three
indexed tails. The 500-input fixture reads only through the last surviving
NBEST candidate's index: the final repeated row of `qinglei…` on both
sides and of `rongkan…` on the subject is therefore not read here.

| Input | Pin ordered sentences | Subject ordered sentences | Covering record |
| --- | --- | --- | --- |
| `fupie` | 副撇 → 副瞥 → 副暼 | 副撇 → 副瞥 → 副氕 | S2 earlier batch tie; derivation below |
| `liangmudianse` | 两亩点色 → 两亩电色 → 两亩点瑟 | 两亩点色 → 两目的安瑟 → 两亩电色 | S1/S2 node admission and S3; derivation below |
| `xinzuiliuzuan` | 新最刘钻 → 新最刘纂 → 新最刘赚 | 新最刘钻 → 新最刘纂 → 新最六祖庵 | #1378 exact S1/S2/S3 entry |
| `zashuanpaqing` | 杂拴帕清 → 杂拴帕请 → 杂拴帕情 | 杂树安帕清 → 杂树安帕请 → 杂树安帕情 | #1378 exact S1/S2/S3 entry |
| `tunshicehongshuanxiangxingmeng` | 吞噬册红拴象形梦 → 吞噬册红拴向星盟 → 吞噬册红拴象形蒙 | 吞噬册红薯安详星梦 → 吞噬册红薯安详星盟 → 吞噬册红薯安详幸蒙 | #1378 exact S1/S2/S3 entry |
| `dupanggentuanming` | 度旁根团名 → 度旁根团名 → 度旁根团明 | 度旁根团名 → 度旁根团明 → 度旁根湍鸣 | #1378 exact S1/S2/S3 entry |
| `junsantuoliaolan` | 军三托蓼兰 → 军三托蓼蓝 → 军三脱了兰 | 军三托里奥兰 → 军三托里奥蓝 → 军三托里奥栏 | #1378 exact S1/S2/S3 entry |
| `qinglei'rengning'bu'shuan'li'maohuai'diaozou'er` | 清泪仍宁不拴礼貌淮调走而 → 清泪仍宁不拴礼貌淮调走二 | 清泪仍宁部署案例毛淮调走而 → 清泪仍宁部署案例毛淮调走二 | #1378 exact S1/S2/S3 entry |
| `rongkan'mu'nenlin'duo'zen'xiucou'pian'chuan` | 容看亩嫩林多怎修凑片传 → 容看亩嫩林多怎修凑片传 → 容看亩嫩林多怎修凑篇串 | 容看亩嫩林多怎修凑片传 → 容看亩嫩林多怎修凑篇串 | #1378 exact S1/S2/S3 entry |

The unchanged `fixtures/w4/oracle-sentence-surface.txt` holds each complete
pin observation, including the six candidate rows.
`crates/oxpinyin-runtime/tests/fixtures/sentence-surface/residuals.txt` uses the same four-column TSV
format to bind all nine complete subject observations. `sentence_tail::measure`
compares every comparable input to that binding, or to the pin where no
residual is declared. The gate checks guessed status, the entire indexed
list and all six candidate rows before checking aggregate counts. A fixed
residual and a new regression therefore cannot cancel. No pin vector,
supplementary selection metadata, or aggregate expectation changes.

#### Source disposition of the two previously unbound cases

Read the `074a2219c90feaf962d0d24f034514033ece5f99` Git blobs from the
libpinyin clone at `/home/sheng/repos/libpinyin`; the clone's working-tree
revision was not used. Read oxpinyin at `bf41d2c0`. Model inputs are the
pinned model20 text tables and the provisioned tkrzw chunk headers.

**`fupie`: S2's earlier batch-position tie.** The first differing candidate
is rank 2, `副暼` versus `副氕`. `氕` is token 16783062 in `gb_char.table`;
`暼` is token 33563818 in `gbk_char.table`. Each has only the `pie` reading
(count 3), no interpolation unigram entry, and no incoming bigram entry.
Their compiled unigram is therefore 1: the pin's
`utils/training/gen_unigram.cpp:49–67` adds one to every phrase. Both extend
the same unigram predecessor `副`, with identical length, pronunciation
probability 1 and pre-log expression; there is no numerical difference to
classify. The other single-reading unigram-1 GBK alternatives tie too.
The earlier GB alternatives `潎` and `苤` have pronunciation fractions
33/69 and 3/6; `丿` has 3/201, so its unigram 13 does not beat count 1
with pronunciation 1.

The pin's unigram expansion walks library/range/token order
(`src/lookup/phonetic_lookup.h:540–572`); the candidate batch walks node
creation order and physical slots (`:278–296`). `氕` precedes `暼`.
`get_top_results<2>` supplies no tie key (`:81–90,143–167,331–335`).
Oxpinyin's `nbest::expand_entries`, `Trellis::candidates` and
`selection_compare` preserve that batch and apply the already approved
earlier-position tie, selecting `氕`. S3 cannot change the selected set.
The captured pin choice is retained as evidence; its incidental heap tie
order is not reconstructed.

**`liangmudianse`: S1/S2 node admission, then S3 raw-score ordering.**
The first differing output is rank 1: `两亩电色` versus `两目的安瑟`.
The distinguishing candidate paths are `两/目的/安/瑟` (length 5) and
`两/亩/点/瑟` (length 4), which reach the same final `瑟` node.
The long path takes the `安 → 瑟` bigram, count 22 over row total 12967;
the short path has no `点 → 瑟` bigram and takes the later unigram pass.
The pin and oxpinyin both expand all bigram predecessors before that
unigram pass (`phonetic_lookup.h:578–639,787–788`; `nbest::expand_entries`).

With facade total 51051831 and the stored f32 lambda 0.312699, the source
expressions in `phonetic_lookup.h:649–663,677–693` and
`BigramLanguageModel::nbest_step_costs_using` / `nbest::expand_one` give
the following approximate f32 scores. These are source/data calculations,
not an execution trace of the pin. `色` includes its 51000/51040
pronunciation fraction.

| Path | Length | Raw possibility | Adjusted S |
| --- | ---: | ---: | ---: |
| `两/亩/点/色` | 4 | -30.591896 | -31.321182 |
| `两/亩/电/色` | 4 | -31.676743 | -32.406029 |
| `两/目的/安/瑟` | 5 | -31.497780 | -32.409389 |
| `两/亩/点/瑟` | 4 | -33.428593 | -34.157879 |

For the two `瑟` paths the pin predicate reports both directions less:
long < short by length (`phonetic_lookup.h:86–88`), short < long by the
adjacent-length margin (`:71–73`). The node calls that same predicate on
its resident front and the later newcomer
(`phonetic_lookup_heap.h:61–76`). In particular, the shorter unigram can
eject a stronger longer front on the pin. The approved adjusted score
rejects that worse newcomer (`nbest::node_loses_to` / `Node::keep`), while
retaining the pin's front eviction and physical-slot rule. This is the
existing S1/S2 repair's expressly recorded node application, not a new
exception or a pruning fix.

The retained longer path ranks just after `两亩电色` by adjusted S but
ahead of it by raw possibility. The pin's truncated tail comparator
would treat that raw difference (about 0.179) as zero
(`phonetic_lookup.h:174–178,337`). S3's `nbest::tail_merge` orders raw
possibility stably without truncation, giving the recorded rank 1.
No epsilon, extra tie-break, math exception or heap reconstruction is
needed for either case.

#### Verification

`cells-for-diff.py origin/main` selected tkrzw alone. The focused gate
passed with nine exact residual bindings, zero unexpected surfaces and
496 / 492 / 491 agreement. Fmt and `git diff --check` passed. The named
`FUPIE_SAME_COUNTS_SURFACE` harness mutation replaced observed rank 2
`副氕` with `副鐅` in both indexed sentences and candidate rows: all
aggregate counts stayed unchanged, but the exact binding rejected it.
The mutation was removed, restoring the tested source exactly.

Run the gate over the pinned libpinyin-data package (`just fetch-contract-data`, tkrzw):

```sh
cargo test --locked -p oxpinyin-runtime --test sentence_surface_parity \
  sentence_surface_matches_the_declared_residual -- --exact --nocapture
```

CI supplies the Rust tests, Clippy and rustdoc gate. No shipping scoring,
pruning, admission or tie rule changes, and no performance measurement.

### Phrases-profile S2 tail ties: `neng` and `nou` (2026-11-04 UTC)

Recorded 2026-11-04T23:43:12Z (`date -u`). The pin was read from the
`074a2219c90feaf962d0d24f034514033ece5f99` Git blobs and oxpinyin at
`fedf2451`. Model inputs are the pinned model20 text tables.

The teardown startup preflight (recorded evidence; tkrzw, pinyin facade,
`phrases` profile, options `0x18a`, sort word `0x1e`) showed two complete-observation differences, identical on every oxpinyin
arm. Each is the third n-best sentence, the `NBEST_MATCH_CANDIDATE` row
with n-best index 2.

| Input | Preflight document | Pin ordered sentences | Subject ordered sentences | Covering record |
| --- | --- | --- | --- | --- |
| `neng` | 33 | 能 → 能 → 䏻 | 能 → 能 → 㴰 | S2 earlier batch tie |
| `nou` | 35 | 耨 → 耨 → 鐞 | 耨 → 耨 → 鎒 | S2 earlier batch tie |

Both are POLICY-ACCEPTED under the existing S2 ruling, with the same
disposition as `fupie` above. They exist only on the `phrases` profile,
so the fresh-profile fixtures do not hold them and
`crates/oxpinyin-runtime/tests/fixtures/sentence-surface/residuals.txt` is unchanged. The preflight
compared complete observations and had no policy verdict, so its
`phrases` witness left both inputs out.

**Profile.** The profile imports 10,000 library-1 rows at count 20000.
An import of a phrase already in that library adds the count to the
named pronunciation only (`src/pinyin.cpp:563–583`); the unigram and the
total do not move. The rows that matter are 能 `neng`, 儜 `neng`,
竜 `long`, 螚 `nai`, 耨 `nou`, 獳 `nou`, 鎒 `nou` and 嬬 `xu`.

**Tie groups.** Every member has a single reading, no interpolation
unigram entry (compiled unigram 1, as for `fupie`) and no sentence-start
bigram entry.

| Input | Member | Token | Table | Reading count |
| --- | --- | --- | --- | --- |
| `neng` | 㴰 | 33561629 | `gbk_char.table` | 3 |
| `neng` | 㲌 | 33561630 | `gbk_char.table` | 3 |
| `neng` | 䏻 | 33561633 | `gbk_char.table` | 3 |
| `nou` | 鎒 | 16781836 | `gb_char.table` | 3 + 20000 imported |
| `nou` | 鐞 | 33561649 | `gbk_char.table` | 3 |
| `nou` | 啂 | 33561653 | `gbk_char.table` | 3 |
| `nou` | 譳 | 33561655 | `gbk_char.table` | 3 |
| `nou` | 㝹 | 33561657 | `gbk_char.table` | 3 |
| `nou` | 羺 | 33561658 | `gbk_char.table` | 3 |
| `nou` | 槈 | 33561659 | `gbk_char.table` | 3 |

Each member extends the sentence start by one unigram step of length 1
with unigram 1 and pronunciation probability 1, so the pre-log expression
and `m_poss` are identical within a group
(`src/lookup/phonetic_lookup.h:650–663`). The first two tails are the two
values of one node, its bigram step and its unigram step (`:670–698`,
`phonetic_lookup_heap.h:56–81`): 能 (unigram 59704, sentence-start bigram
9476) and 耨 (unigram 125, bigram 10). The nearest values below each group
are 儜 at pronunciation 20003/20006 and 獳 at 20003/20053. The imports
leave 竜 (unigram 80, bigram 69) at pronunciation 50/20053 and 嬬
(unigram 6) at 50/20103, both below the group.

**Batch.** The final-step batch walks nodes in creation order and physical
slots (`:272–297`): the bigram pass first, then the unigram pass in
library, range and token order (`:540–572,578–641`).

| Input | Batch, positions from 0 | Tie positions |
| --- | --- | --- |
| `neng` | 能, 能, 竜, 竜, 螚, 儜, 䪏, 䘅, 㴰, 㲌, 薴, 濘, 䏻 | 8, 9, 12 |
| `nou` | 耨, 耨, 獳, 鎒, 嬬, 檽, 䫖, 㕢, 䔈, 㜌, 㳶, 鐞, 䘫, 䰭, 䨲, 啂, 譨, 譳, 䅶, 㝹, 羺, 槈, 嗕, 搙 | 3, 11, 15, 17, 19, 20, 21 |

**Pin.** `get_top_results<2>(3, …)` selects the tails with `make_heap`
and `pop_heap` (`:142–171,331–335`), which `std_lite` takes from the
C++ standard library (`src/include/stl_lite.h:38–44`). With equal lengths
the comparator is `m_poss <` and supplies no tie key (`:81–90`). The
third pop is position 12 (䏻) for `neng` and position 11 (鐞) for `nou`:
libstdc++'s order among equal heap elements.

**Oxpinyin.** `Trellis::candidates` builds the same batch
(`crates/oxpinyin-engine/src/nbest.rs:427–445`) and `selection_compare`
orders equal adjusted scores by earlier batch position (`:223–234`),
selecting position 8 (㴰) and position 3 (鎒). S3 cannot change the
selected set.

**Ruling.** The S2 text of the
[witnessed algorithm-precondition amendment](compatibility-policy.md#algorithm-precondition-violations)
says: "Selection uses the total key `(ordered S, batch_position)`, with
earlier batch position first on equal S." The
[source policy](../source-policy.md#internal-language-artefacts) says:
"Equal heap-element order, search results on unsorted input and a float's
last bit are not contract guarantees." The pin's choice inside each group
is not reconstructed.

**Candidate rows.** The sentence prepend and the phrase-string
deduplication (`src/pinyin.cpp:1934–1947,2116–2125`) remove the selected
character's NORMAL row on each side. The remaining NORMAL rows keep one
relative order on both sides; no other row moves.

No shipping code changes.
