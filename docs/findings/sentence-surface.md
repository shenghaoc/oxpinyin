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
- The constraint machinery (`CONSTRAINT_ONESTEP`/`diff_result`) is not
  ported; the engine's selection model re-seeds the remaining input from
  the recorded history, which is the established W6 surface.

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
  surface exists for every model (the fallback candidate list keeps its
  pre-W14 shape until a guess happens).
- Rows carry their token path, and choosing a row — identified by its
  list position among the prepended head entries, never by the kind or
  `nbest_index` alone, which a fallback sentence candidate also carries
  as 0 — records **all** of it:
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
