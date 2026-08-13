# Residual characterisation after the construction freeze

Date: 2026-08-13 · Status: **measured and characterised; nothing changed and
nothing is solved. Post-§8 successor to `parity-climb-residual.md`'s bucket
table.**

The construction contract in `candidate-construction.md` §8 froze the
*absence* of Strategy A / B for the current residual: under the pinned
observation surface the oracle emits only `NORMAL_CANDIDATE` phrase-lookup
candidates (§7.3), so the residual is unigram phrase-cost / coverage
calibration inside the existing phrase-lookup path, not a missing lattice or
two-pass pass (§8.2). This finding characterises **what that residual actually
looks like** at the candidate-list level, so the next calibration step starts
from evidence instead of a blank slate.

**Scope discipline, stated up front.** Every number in §2–§5 is either

- a frozen §0 pin (6,525 / 9,232 / 70 / 65,505 of 98,930 — the two this
  measurement touches, top-1 and absent, are reproduced exactly; nothing
  re-pinned), or
- measured on the **current tree** by replaying our decoder over the parity
  corpus (`Session::reset` + one `type_pinyin` per input, exactly as
  `parity-worst` does) and joining the output against the three portable
  fixtures — `oracle-candidates.txt`, `oracle-candidate-structure.txt`,
  `oracle-paths.txt` — with the export tables at `/tmp/pinyin-rs-export`.

No Rust source, ScoringConfig constant, or parity pin changed; no new baseline
is claimed; the residual is not treated as solved. The construction contract
§8 stands untouched.

**Method, so the two snapshots cannot be mixed.** The §2 bucket table is the
output of the committed tool `cargo run -p pinyin-oracle --release --bin
parity-worst` on the current tree (command quoted in §2, re-runnable today).
The fine-grained signals in §3–§6 were produced on the same tree the same day
by a **throwaway join harness**, run as
`cargo run -p pinyin-oracle --release --bin residual-analysis`, that replays
the exact loop `parity_worst.rs` uses (`reset` + one `type_pinyin` per corpus
input) and joins our candidate texts against the same three portable fixtures
via the export tables at `/tmp/pinyin-rs-export`. **Reproducibility, stated
flatly:** the harness is deliberately not committed (this is a docs-only PR)
and has been deleted, so §4–§6 are **not re-runnable from a committed command
today** — they are marked here as derived from the committed fixtures by the
harness described above, and the tables below are the raw output totals
transcribed verbatim as the auditable record. The run was repeated after the
harness was rebuilt to the same specification — the replay loop described
here plus the signal definitions tabulated in §3–§6 (2026-08-13): **every
total reproduced identically.** §3's invariant is derivable from
`oracle-candidates.txt` alone, with no decoder at all. Promoting the §4–§6
breakdowns to a re-runnable committed command is a small extension of
`parity_worst.rs` and is deliberately left out of this PR's docs-only diff.

## 1. The residual is ranking of NORMAL candidates — re-confirmed from the fixture

Re-reading `oracle-candidate-structure.txt` during this characterisation: the
`type` column has **exactly one distinct value** across all 97,442 triples —
`NORMAL_CANDIDATE` — and **zero** rows carry an `nbest_index` other than `-`.
This is §7.3, reproduced mechanically, and it is what makes every bucket below
a **ranking/calibration** bucket: the phrases involved are all phrase-lookup
candidates, and per §1.5 a phrase we place at any positive rank demonstrably
reached collection, so starvation cannot explain it.

## 2. The post-F1, post-§8 buckets (measured on the current tree, 2026-08-13)

`cargo run -p pinyin-oracle --release --bin parity-worst` (same command §1.5
names; the join-harness re-run reproduced its output row for row):

```text
compared  10,190
top-1 hits  6,525        (reproduces the §0 pin exactly)
misses      3,665
```

| Bucket | Count | Share of misses | Note |
|---|---:|---:|---|
| rank 2–5 (near-miss) | 2,707 | 73.9% | oracle rank-1 held at rank 2–5 |
| rank 6–9 | 337 | 9.2% | held, badly ordered |
| rank 10+ | 551 | 15.0% | held at depth (rank ≥ 10 of ≤ 64) |
| absent | 70 | 1.9% | reproduces the §0 pin exactly |

These are **post-F1, post-§8, current-tree** numbers; the 2,652 / 342 / 584 /
177 table over 3,755 misses in `parity-climb-residual.md` and
`candidate-construction.md` §1.5 is the **pre-F1** snapshot — F1 moved absent
177 → 70 and the other buckets accordingly, and the two tables answer
different questions, so they must not be mixed. The dominant residual is
unchanged in kind: **near-miss ranking is ≈ 74% of misses**, and rank 2 alone
is 1,585 of them (43% of all misses — the oracle's rank-1 sits one slot below
ours more often than in any other position).

## 3. The oracle's recorded surface is length-major (fixture-derived invariant)

Before dissecting our misses, a fact about the target itself that the fixture
proves and that reframes "calibration":

Over all **10,037 distinct inputs** in `oracle-candidates.txt`, the oracle's
**rank-1 is never text-shorter than any candidate in its own recorded top-10** —
zero exceptions. Rank-1 vs rank-2 specifically: rank-1 longer in 1,527 inputs,
equal in 8,486, **shorter in 0**.

Rank-1 text length across the corpus:

| Rank-1 length (chars) | Inputs | Share |
|---|---:|---:|
| 1 | 7,423 | 74.0% |
| 2 | 2,609 | 26.0% |
| 3 | 5 | 0.05% |
| 4+ | 0 | 0 |

Text length in characters is a clean proxy for phrase length here: **no
candidate text in the fixture is ASCII or contains an apostrophe** (measured,
0 of 97,442 triples), so a char count equals the phrase's syllable span for
this corpus. **Exact definition of "shorter":** the comparison is the
candidate **text length in Unicode characters** (scalar-value count of the
UTF-8 text) — not byte length (the two agree in order here anyway, every text
being non-ASCII CJK) and **not key/syllable coverage**, which no fixture
exposes (§1.1). The invariant is therefore a *text-level* property of the recorded
surface, labelled **SHOWN** exactly as stated, and no stronger coverage claim
is implied by it.

This is **SHOWN**, not inferred: the fixture is the oracle's post-sort list
under flags `0x1e = SORT_WITHOUT_LONGER_CANDIDATE | SORT_BY_PHRASE_LENGTH |
SORT_BY_PINYIN_LENGTH | SORT_BY_FREQUENCY` (§1.6 names the flags; the capture
records exactly that surface). The sort is phrase-length-major, so the
recorded order is "longest phrase first, frequency orders **within** a length
class". Consequence for calibration: the oracle's rank-1 for any input is
*always one of the longest phrases its surface offers*, and beating the pin at
rank-1 means (a) surfacing a phrase at least as long, and (b) matching its
frequency order among same-length phrases. Our decoder has no hard
length-major key: length preference is the soft `phrase_key_bonus` credit in a
log-linear cost. Every miss below is one of two shapes: a **same-length
frequency inversion** (within-length-class calibration) or a **length
inversion** (our soft bonus losing the length-major competition).

## 4. Rank 2–5 near-misses: same-length frequency inversions, one slot away

Measured over the 2,707 near-misses (**post-F1, post-§8, current-tree** totals
— not the pre-F1 2,652 in `parity-climb-residual.md`; join-harness output
reproduced verbatim, see the method note at the top of this finding):

| Signal | Count | Share |
|---|---:|---:|
| oracle top-1 **equal text length** to our top-1 | 2,498 | 92.3% |
| oracle top-1 **longer** than our top-1 | 169 | 6.2% |
| oracle top-1 **shorter** than our top-1 | 40 | 1.5% |
| our top-1 is the **oracle's rank-2** (adjacent swap) | 1,354 | 50.0% |
| our top-1 **present** in the oracle's top-10 | 2,560 | 94.6% |
| our top-1 a proper text **prefix of** oracle top-1 | 24 | 0.9% |
| oracle top-1 a proper text **prefix of** our top-1 | 12 | 0.4% |
| oracle top-1 is **1 char** | 2,285 | 84.4% |
| oracle top-1 is **2 chars** | 422 | 15.6% |
| rank distribution | 2: 1,585 · 3: 644 · 4: 333 · 5: 145 | 58.6% at rank 2 |

Reading, in order of what the numbers rule out:

1. **Construction is ruled out twice over.** These are `NORMAL` candidates
   (§1) that we already hold at rank 2–5 — they reached phrase collection, so
   §1.5's starvation argument applies and no lattice or two-pass pass touches
   them (§8.2's negative result). And 94.6% of the time our top-1 is a phrase
   the oracle itself offers in its top-10: the disagreement is purely the
   *order* of a shared set, which is what `collect_prefix_phrases` + the
   stable sort by `Candidate::cost` decide — the frozen construction, not its
   absence.
2. **Coverage length is almost not the issue in this bucket.** 92.3% of
   near-misses have our top-1 at the *same* text length as the oracle's — a
   within-length-class frequency/tie-break inversion, invisible to
   `phrase_key_bonus` (which credits only *extra* keys). Only 169 rows (6.2%)
   are a longer phrase losing to a shorter one, and only 24 of those (0.9%)
   have our top-1 as a literal sub-phrase prefix (the `你` beats `你好` shape).
   The near-miss bucket is dominated by single-character rank-1s (84.4%) —
   the classic `分`/`风`, `沙`/`山` class where both phrases cover one key and
   only frequency separates them.
3. **Half of the bucket is one slot away.** 1,354 of 2,707 (50.0%) have our
   top-1 *equal to the oracle's rank-2*: the oracle's own top two, swapped.
   This is the smallest possible inversion — the calibration gap is the
   ordering of the top two same-length phrases, not a wrong list.
4. **A small residual inside the residual: we top-rank a phrase the oracle
   does not offer.** 147 rows (5.4%) have our top-1 outside the oracle's
   top-10. 40 rows have our top-1 *longer* than the oracle's rank-1; under
   the length-major surface (§3) a longer phrase the oracle offered would
   have outranked the shorter one, so our top-1 there is a phrase the
   oracle's `0x1e` surface suppresses or does not produce at all (the
   `LONGER_CANDIDATE` class is excluded by that flag word, §1.6) — an
   emission divergence, distinct from a ranking one, and tiny.

The lever this bucket names is not a construction one: it is the
**empty-history cost** of same-length phrases — the scaled unigram plus
tie-break, §8's "unigram phrase-cost calibration" — and its measured footprint
is 2,498 equal-length inversions, 1,354 of them adjacent.

## 5. Rank 6–9 and 10+: the deeper the miss, the more it is a length inversion

Same **post-F1, post-§8, current-tree** measurement as §2 and §4; the 337 /
551 rows here are the §2 bucket counts (not the pre-F1 342 / 584 in
`parity-climb-residual.md`).

| Signal | rank 6–9 (337) | rank 10+ (551) |
|---|---:|---:|
| oracle top-1 longer than our top-1 | 195 (57.9%) | 414 (75.1%) |
| equal length | 141 (41.8%) | 135 (24.5%) |
| oracle top-1 1 char / 2 chars | 132 / 205 | 125 / 426 |
| our top-1 present in oracle top-10 | 308 (91.4%) | 435 (78.9%) |
| our top-1 proper prefix of oracle top-1 | 31 (9.2%) | 70 (12.7%) |

The mix flips as the miss deepens: same-length inversions dominate rank 2–5,
while at rank 10+ three quarters of the rows are the oracle's **longer**
phrase buried under many shorter ones, and two-char rank-1s are 77% of the
bucket. These rows are exactly the shape `phrase_key_bonus` exists for
(`你好` above `你`, `scoring-spec.md` inequality 1) — the bonus fires, the
longer phrase reaches the list, but the credit is not strong enough to carry
it past a crowd of shorter, cheaper phrases. Our length preference is one
provisional constant competing in a log-linear cost; the oracle's is a hard
sort key (§3). The 101 rows where our top-1 is a literal prefix of the
oracle's rank-1 (31 + 70) are the cleanest subset: a sub-phrase we placed
above the phrase itself.

## 6. The absent bucket: 70, and what the fixtures do and do not say about it

The absent count measured here is **70, reproducing the §0 pin exactly.** It
is the only bucket where §1.4.1 starvation can hide — a rank 2–5 / 6–9 / 10+
miss proves the phrase reached collection, an absent miss does not (§1.5).

Shapes measured over the 70 (our list is **full at 64 candidates in every
row** — absent means *this specific text* never reached collection, not an
empty list):

| Signal | Count |
|---|---:|
| oracle top-1 equal text length to our top-1 | 40 (57.1%) |
| oracle top-1 longer | 29 (41.4%) |
| oracle top-1 shorter | 1 |
| oracle top-1 1 char / 2 chars / 3 chars | 39 / 30 / 1 |

The examples are the `分`→`风`, `沙`→`山`, `巨额`→`觉`, `货仓`→`和` class:
same-length-different-phrase or longer-phrase-missing, never an empty list.

**What does not exist yet: a starvation measurement.** These three fixtures
cannot separate, for the 70, *phrase absent from our tables* from *phrase
present but its segmentation never reached phrase collection* (§1.4.1).
`oracle-paths.txt` records the oracle's **selected** path (present for 10,022
of the 10,037 fixture inputs) but no fixture exposes a candidate's own
segmentation — the public API does not carry it and the homograph caveat of
§1.6 applies to any reconstruction. So this finding **does not** quantify
starvation, does not estimate it from these 70, and does not invent a
measurement that §1.6 / §7.5 explicitly left for a future, different
instrument. The 70 stays exactly the §0 pin, and §8.3's re-open condition (b)
— a measured starved-segmentation population large enough to move the pins —
remains unsatisfied, which is the frozen state of the contract.

## 7. Carried open: NORMAL ≠ "exactly one dictionary phrase"

§7.5 / §8.3 keep open that `NORMAL_CANDIDATE` means "phrase-lookup
candidate", not "one token": the token decomposition of e.g. `中国人` is
unreachable from the public API. Nothing in §2–§5 depends on that question —
the signals measured here (text length, prefix relation, membership in the
oracle's top-10) are properties of the **recorded text lists**, available to
both sides without touching token structure. The open point is carried
forward unchanged; resolving it would need an instrument that does not exist
yet, and it is not required to run the calibration experiments in §8.

## 8. Calibration experiments still legal under the §8 contract

The contract freezes *construction* — the `refresh` pipeline of §8.1 stands
(k-best → collect → pool → stable-sort → dedup → truncate). It does **not**
freeze the provisional weights inside that pipeline; `scoring-spec.md` marks
every numeric weight provisional on purpose. The following levers are named
from the tree, each with the bucket it targets and the measured population it
must move. Examples only — **no numbers are proposed here**; every candidate
value must come from a sweep measured against the §4 gates (the five
`assert_eq!` pins are bit-exact and will trip on any ranking change; a
deliberate re-pin is a reviewed step, never a silent edit).

1. **Phrase-length / coverage term strength — `ScoringConfig::phrase_key_bonus`
   (provisional 1,000).** Currently a linear credit per extra key,
   **kind-blind**: a two-key phrase gets the same 1,000 through an `Exact`,
   `Segmentation`, or `Incomplete` edge, while the penalties beside it are
   kind-aware. Open variations, all inside the frozen form: (a) a
   kind-weighted bonus (discount coverage earned through incomplete edges);
   (b) a nonlinear credit in extra keys; (c) coverage counted per key vs per
   covered character/byte. Target: the 778 present-but-outranked longer-phrase
   rows (169 rank 2–5 + 195 rank 6–9 + 414 rank 10+), especially the 101
   prefix rows of §5, with rank 2–5's 92.3% same-length majority watched for
   collateral re-ordering.
2. **Unigram scale / tie-break for same-length phrases — `UNIGRAM_TIEBREAK_SCALE`
   (16, `crates/pinyin-data/src/lm.rs`, empty-history branch only).** F2 swept
   this globally and 16 won (negative result) — but the sweep moved **one**
   global scale. Unmeasured and legal: (a) a length-class-dependent scale or
   tie-break (the same-length competition is where the signal acts, §4);
   (b) a secondary sort key by raw frequency at equal cost — `rank_phrases`
   uses a stable sort with no frequency tie-break, and insertion order from
   the dictionary is the current tie-breaker. Target: the 2,498 equal-length
   rank 2–5 inversions, 1,354 of them adjacent swaps, without disturbing
   `f2-unigram-tiebreak-sweep.md`'s measured global optimum.
3. **Structural penalties — `segmentation_penalty` (750) /
   `incomplete_penalty` (999) / `exact_penalty` (0).** They enter **both** the
   k-best path cost and the phrase cost, so rebalancing them shifts which
   segmentations survive to phrase collection *inside the unchanged
   construction*. This is calibration, not a path-set change — but it is the
   one lever here whose effect can reach the absent bucket, and §4 gate 5
   (path-set / tie-swap accounting) applies: a change that only reshuffles
   paths without moving top-1 is rejected.
4. **Model-term strength — `ScoringConfig::lm_weight` and λ.** Both
   provisional (`scoring-spec.md`; λ = 1/2 authored neutral in
   `crates/pinyin-data/src/lm.rs`). Any sweep is global and must clear the
   same gates.
5. **Not a lever (stated to keep the boundary clean).** The `expansion_limit`
   empty-product gap (`expand_keys` returns nothing past 64 expansions) is the
   F4 item, skipped because its trigger (absent > 100) is not met (absent =
   70); it is not a near-miss lever and is not proposed here. The §8.1
   emission order (collection loop order, dedup-by-text, truncation to 64) is
   frozen by the contract and out of scope for calibration.

Each experiment is a constants-only change inside the frozen `refresh`
pipeline, measured through `cargo test --release --locked -p pinyin-oracle
--test real_tables_integration` and `parity-worst` (§4), serial == parallel.
The decision after measurement is evidence-gated, exactly as §6 step 4 of
`candidate-construction.md` now records.

## 9. What this document does not do

- No Rust source, ScoringConfig constant, or parity pin changed.
- No lattice edges, no two-pass re-rank, no construction change — §8 stands.
- No new baseline claimed: 6,525 and 70 are the existing §0 pins, reproduced
  by the measurement; the bucket table in §2 is a re-measurement of §1.5's
  categories on the current tree, not a re-pin.
- No starvation measurement invented (§6).
- The residual is characterised, not solved.

STOP and report rather than inventing evidence.

Assisted-by: opencode:deepseek-v4-pro
