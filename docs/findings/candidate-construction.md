# Candidate-construction SPEC (Discrepancy 2)

Date: 2026-08-13 · Status: **characterisation frozen; W2-CAND captured and
analysed (§7); construction contract still NOT frozen — next step is §6 step 3
(the contract PR).**

This document characterises the candidate-construction gap between our decoder
and the pinned oracle, and it decides one thing deliberately: it does **not**
freeze a construction contract. The evidence needed to freeze one is not in the
fixture the parity test measures on, and guessing the missing half is the exact
failure this project has STOPped for twice — the fabricated `pinyin_index`
encoder mapping (PR #28, refuted) and the `[b, ing]` skip-and-continue claim.
The deliverable here is therefore the residual
characterisation, the narrow invariant the fixture *can* prove, and the
**capture extension** that must land before any lattice or two-pass code is
written.

Baselines and neighbours: `docs/findings/parity-climb-residual.md`,
`docs/findings/f3-bigram-kbest.md`, `docs/findings/f2-unigram-tiebreak-sweep.md`,
`docs/findings/scoring-spec.md`, `docs/findings/segment-graph.md`,
`docs/findings/kbest-search.md`, `docs/findings/session-api.md`.

## 0. The pinned baseline (the climb floor)

The frozen number every change in this area is measured against, pinned in
`crates/pinyin-oracle/tests/real_tables_integration.rs`
(`real_tables_session_reports_parity`) over `fixtures/w4/oracle-candidates.txt`:

```text
compared            10190
top-1                6525   64%     assert_eq! pin
top-5-set            9232   90%     assert_eq! pin
prefix-10 overlap   65505 of 98930  66%   assert_eq! pins (numerator + denominator)
absent                 70          assert_eq! pin
```

These five `assert_eq!` are bit-exact pins; the tolerant floors beneath them
(top-1 ≥ 55%, top-5 ≥ 80%, absent ≤ 4%) are the regression envelope. This is
the post-F1 floor (`docs/findings/f1-junk-aware-parse.md`); the pre-F1 snapshot
in `parity-climb-residual.md` is 63% / 177 absent.

## 1. Residual characterisation

The hypothesis under test (from an uncommitted audit, never before written to a
finding):

> Upstream runs a phrase-token lattice DP, selects one best sentence, then
> returns that sentence's first phrase plus alternatives competing at the same
> position, with bigrams participating in path selection. Ours takes k-best
> **syllable** paths, collects every prefix-length phrase the dictionary emits
> along them, greedy-DPs over the collected phrases, and applies bigrams
> post-hoc so they cannot influence segmentation.

The rest of §1 separates what `fixtures/w4/oracle-candidates.txt` demonstrably
shows from what the hypothesis asserts about oracle internals the fixture does
not expose. Every claim is tagged **SHOWN** (grounded in cited fixture lines) or
**INFERRED** (not fixture-derivable; a capture extension named in §1.6 would
settle it).

### 1.1 What the fixture is, exactly

The fixture carries one thing: `input → ranked candidate *texts*`, depth 10.
Header and format, verbatim:

```text
# format: input<TAB>rank<TAB>candidate_text
# total_triples=97442 (distinct inputs with candidates 10037)
```

It is produced by `oracle_candidates.rs`, which records
`OracleObservation.candidates` — a `Vec<String>`. Per candidate the harness keeps
**only the string**; `live.rs::collect_candidates` calls
`pinyin_get_candidate_string` and nothing else. Two different things are dropped,
and the difference decides what any capture could ever recover:

- The candidate's **type** and **n-best index** *are* exposed by the pinned
  public header — `pinyin_get_candidate_type` (the `lookup_candidate_type_t`
  enum) and `pinyin_get_candidate_nbest_index` — and are simply not recorded
  today. A capture can add them (§1.6).
- The candidate's **segmentation**, **covered byte range / consumed length**, and
  **phrase-token decomposition** are *not* exposed at all. In the pinned header
  (`include/libpinyin-2.11.91/pinyin.h`) `lookup_candidate_t` is opaque; there is
  no public accessor for begin/end, tokens, or a model score. No capture can
  record them without reading upstream C/C++ layout (forbidden) or an indirect
  probe with named limits (§1.6).

This is the root reason §1 cannot promote the hypothesis to a contract: the
mechanism the hypothesis is *about* — which segmentation and which phrase tokens
each candidate used — is not exposed by the public API the parity test measures
through. Only the type / n-best split is.

A second, separate portable fixture — `fixtures/w4/oracle-paths.txt`
(`segment-graph.md`) — records the oracle's *selected segmentation* (one path
per input) in `pinyin-capture-v1` notation. It is the only mechanism-adjacent
signal we have, and §1.3 uses it against the candidate fixture.

### 1.2 SHOWN: candidate construction pools phrases across segmentations

`fixtures/w4/oracle-candidates.txt`, verbatim:

```text
xian	1	西安
xian	2	西岸
xian	3	锡安
xian	4	县
xian	5	见
```

`西安`, `西岸`, `锡安` require the segmentation `xi` + `an`. `县`, `见` require
the single syllable `xian`. Both segmentations contribute phrases to one ranked
list. A one-segmentation decoder could not emit this list. **SHOWN.**

Same behaviour where two full-coverage phrases come from disjoint splits:

```text
fangan	1	方案
fangan	2	反感
```

`方案` is `fang` + `an`; `反感` is `fan` + `gan`. Interleaved in one list.
**SHOWN.** (`scoring-spec.md` proves the same point from
`fixtures/foundation/f-a.txt` as `ambiguous-fangan`; `divergence-taxonomy.md`
counts 468 `tie-swap` inputs of this shape, ranks 1–128.)

Coverage-beats-prefix, from single segmentations:

```text
nihao	1	你好
nihao	2	你
zhongguoren	1	中国人
zhongguoren	2	中国
zhongguoren	3	中
```

The longer phrase outranks its own first syllable. **SHOWN** (already the basis
of `phrase_key_bonus > 0` in `scoring-spec.md`).

**Consequence for the hypothesis.** The clause "ours … cannot pool across
segmentations" is **false against our own tree.** `Session::refresh`
(`crates/pinyin-engine/src/session.rs:461`) iterates *every* one of the
`SEGMENTATION_K = 8` best paths and calls both `collect_prefix_phrases` and
`collect_sentence` on each, then pools, sorts by `Candidate::cost`, dedups by
text, and truncates to `MAX_CANDIDATES = 64`. The doc comment at
`session.rs:457` names `xian` and `fangan` as the reason. We pool across
segmentations today. The accurate framing of our gap is not "we don't pool" — it
is §1.4.

### 1.3 SHOWN: rank-1 is not the first phrase of the selected *syllable* path

Cross-reference the two portable fixtures. `oracle-paths.txt`, verbatim:

```text
xian	4	xian@0:4:complete
fangan	6	fan@0:3:complete,gan@3:6:complete
```

- `xian`: selected path is the **single syllable** `xian`, yet candidate rank-1
  is `西安` (from `xi` + `an`, a segmentation the selected path does not use).
- `fangan`: selected path is `[fan, gan]`, yet candidate rank-1 is `方案` (from
  `[fang, an]`, the *other* split).

In both cases rank-1 comes from a segmentation different from the selected path.
`oracle-paths.txt` is the pin's selected **syllable segmentation** (recovered at
the parse layer, `segment-graph.md`), not a phrase-token sentence. So what is
**contradicted (SHOWN)** is the narrow reading "rank-1 = first phrase of the
selected *syllable* path": whatever orders the candidate list, it is not that.
The candidate list is a *pooled, re-ranked* set drawn from multiple
segmentations — structurally what `refresh` already does.

This does **not** dispose of the hypothesis's phrase-token "best sentence." That
is a *different layer* — a phrase-token decode, not the parse-level segmentation
this fixture carries — and whether the oracle emits "first phrase of the best
sentence plus position-matched alternatives" at that layer stays **INFERRED**
until candidate type / n-best index land (§1.6). §1.3 falsifies the syllable-path
reading only; it does not close Strategy A's emission model.

### 1.4 The precise gap (part SHOWN, part INFERRED)

Refining the hypothesis's "ours" against `session.rs`:

1. **Segmentation *survival* is unigram-only.** k-best
   (`kbest-search.md`, `SEGMENTATION_K = 8`) ranks **syllable** paths by
   per-key unigram cost + structural penalties (`scoring-spec.md`); the
   `EdgeCost` table is precomputed per key. No phrase or bigram term enters path
   selection. A segmentation whose *phrases* are cheap but whose *per-key
   unigrams* are not can fall outside the top 8 and never reach phrase
   collection, and no downstream phrase cost can pull it back. This is a fact
   about our code, **SHOWN** for our side. (It is exactly the architectural
   limit `f3-bigram-kbest.md` hit: single-key token proxies in the edge cost
   are "too noisy," and "the correct fix requires phrase-level lattice edges.")
   **Where this shows up:** a phrase that never reaches collection is **absent**
   from our list, not merely mis-ranked. So starvation can only produce the
   **absent** bucket (§1.5); any oracle rank-1 we place at rank 2+ demonstrably
   reached collection, and is therefore a ranking/calibration miss rather than a
   starvation miss.

2. **First-position ranking is history-free for a fresh observe.** Pooled
   candidates are re-sorted by `Candidate::cost`, computed against
   `self.history`, which is **empty** for a fresh composition. So the rank-1
   among pooled candidates is decided by unigram-phrase cost + coverage bonus; a
   left-bigram cannot order it. `collect_sentence` (`session.rs:538`) *does*
   thread a running bigram history (`prefix_history`) — but only for the
   multi-phrase whole-path sentence candidate, whose **first** phrase still
   starts from the empty history. **SHOWN** for our side
   (`parity-climb-residual.md` §"Rank 2–5", point 2).

3. **Does the oracle's rank-1 ordering actually depend on a bigram / a
   phrase-level lattice?** **INFERRED — not fixture-derivable.** The candidate
   texts do not say whether a given oracle rank-1 is a *single dictionary
   phrase* (你好, 方案, 中国人 read equally well as one phrase) or a
   *bigram-linked two-phrase sentence*. If most oracle rank-1s are single
   phrases, then neither the hypothesis's "bigrams in path selection" nor a
   two-pass bigram re-rank is the operative lever — the lever is unigram
   phrase-cost / coverage calibration, and `f2-unigram-tiebreak-sweep.md`
   already reports the unigram tiebreak scale optimal at 16 (negative). We
   cannot tell which regime we are in from this fixture. This is the load-bearing
   inference, and it is unresolved.

4. **Bigrams "participating in path selection."** **INFERRED.** For a fresh
   single `observe` (how both the oracle capture and our parity harness run:
   `open_with_temp_user_dir` + per-input `observe`; our side `reset()` +
   one `type_pinyin`), the first-position phrase has no left context. A left
   bigram cannot rank it. libpinyin may apply a sentence-start term, but whether
   it does and whether it changes rank-1 order is not observable from candidate
   texts. `f3-bigram-kbest.md` is the empirical warning against guessing the
   bigram's role at the segment level: it measured **−1pp top-1** and no code
   was kept.

### 1.4.5 What was falsified vs what is still open

The original characterisation carried three claims. §1 corrects **one** and
keeps the other two live; the correction in §1.2 must not be read as discarding
all three. Each is labelled with its evidence status so a reviewer sees the
split at a glance.

- **Oracle-side: "rank-1 = first phrase of the selected *syllable* path." —
  FALSIFIED (SHOWN, §1.3).** Candidate rank-1 is *not* the first phrase of the
  oracle's selected syllable path (`xian` path `[xian]` → rank-1 `西安`; `fangan`
  path `[fan, gan]` → rank-1 `方案`). The phrase-token "best sentence" is a
  different layer and stays INFERRED (§1.3, §1.6); it is **not** falsified here.

- **Ours: "bigrams apply post-hoc and cannot influence segmentation." — STILL
  LIVE (SHOWN for our side, §1.4.1–§1.4.2).** k-best selects syllable paths on
  per-key unigram + structural cost only; no phrase or bigram term enters path
  selection, and a fresh-observe first position has empty history. Read directly
  from the tree; the §1.2 pooling correction leaves it untouched. It is not
  discarded — it is the standing description of our path stage.

- **Ours: "the final stage is a greedy DP over the pooled phrases." — STILL
  LIVE, with a wording correction (SHOWN for our side).** The DP is *per path*
  inside `collect_sentence` (a min-cost `best[]` recurrence, `session.rs:551`,
  optimal over that path rather than greedy); the *cross-path* final stage is a
  stable sort by precomputed `Candidate::cost` with dedup and truncation
  (`session.rs:483`), not one global DP. The limb stands as a description of our
  construction; only the "greedy" / "single final-stage DP" phrasing is tightened
  here, not withdrawn.

The falsification of the oracle-side claim is the **§1.3** result; the pooling
correction — "we *do* pool across segmentations" — is **§1.2**, a distinct point.
The two OUR-side limbs are descriptions of our own code, readable from the tree,
and neither is discarded. What stays **INFERRED** is not any of these limbs, but
whether replacing the second and third with a global phrase-lattice DP
(Strategy A, §2) would move parity — the §1.4.3 question W2-CAND exists to
answer.

### 1.5 Residual buckets, and why the fixture caps the analysis

From `parity-climb-residual.md` (regenerate with
`cargo run -p pinyin-oracle --release --bin parity-worst`), the 3,755 top-1
misses over 10,190:

| Bucket | Count | Share of misses |
|---|---:|---:|
| rank 2–5 (near-miss ranking) | 2,652 | 70.6% |
| rank 6–9 | 342 | 9.1% |
| rank 10+ | 584 | 15.6% |
| absent | 177 (70 post-F1) | 4.7% |

The dominant residual is **near-miss ranking**: the oracle's rank-1 is in our top
five but not first. A phrase we place at any positive rank *reached phrase
collection*, so **starvation (§1.4.1) is ruled out for rank 2–5, 6–9 and 10+ by
construction** — it can only produce the **absent** bucket (70 post-F1). The
near-miss buckets are therefore ranking/calibration, split between two of our own
mechanisms: history-free first-position cost (§1.4.2) and unigram phrase-cost /
coverage calibration. `parity-worst` categorises by *comparing candidate texts*
(its worst-50 shows `wrong-segmentation 0`, `wrong-scoring 0`), so it cannot
separate those two — but the discriminator is **not** starvation. What W2-CAND
adds (§1.6) is orthogonal to that split: whether the oracle's rank-1 is a
`NORMAL` phrase candidate or an `NBEST_MATCH` sentence candidate (§1.4.3), which
decides whether a bigram/sentence lever is in play at all.

### 1.6 Verdict of §1 and the capture extension (W2-CAND)

**The candidate fixture is sufficient to prove cross-segmentation pooling
(§1.2) and that rank-1 is a pooled re-rank rather than the selected *syllable*
path's first phrase (§1.3). It is NOT sufficient to freeze a construction
contract**, because the contract turns on §1.4.3–§1.4.4, which are inferences
about oracle internals the fixture does not expose. Freezing lattice/two-pass behaviour now would repeat
the PR #28 encoder mistake. Per this task's own escape hatch, the deliverable is
scoped to the capture that must come first.

**W2-CAND — per-candidate type capture.** Extend the existing W2 oracle harness
(Linux, `oracle-ffi`; a one-time run, its output committed as a portable fixture
and replayed in CI with no oracle build, exactly like `oracle-candidates.txt` and
`oracle-paths.txt`). For each candidate already borrowed via
`pinyin_get_candidate` in `live.rs::collect_candidates`, additionally record the
two fields the pinned public header actually exports:

- `pinyin_get_candidate_type` → the public `lookup_candidate_type_t` value;
- `pinyin_get_candidate_nbest_index` → the n-best index.

**What the public API cannot yield, and must not be faked.** `lookup_candidate_t`
is opaque in `include/libpinyin-2.11.91/pinyin.h`. There is **no** public accessor
for a candidate's begin/end, phrase tokens, or model score, so the capture does
**not** record consumed length, token ids, or a score. The only allowed
*indirect* route to tokens is `pinyin_lookup_tokens` on the candidate **string**
(already wrapped as `Live::lookup_tokens`); it returns the dictionary token(s)
whose phrase text equals the string, so it **cannot** disambiguate homographs
(same text, different token) and gives **no** offsets. `pinyin_choose_candidate`
returns a post-choose cursor but is mutating and would have to be followed by a
reset. Neither is part of the frozen capture; either may be used in *analysis*
(§6) with these caveats stated. Reading the struct layout from upstream C/C++ to
recover the hidden fields is forbidden (AGENTS.md) — the absence of a public
accessor is a hard stop, not an invitation to improvise (this is the exact
failure this document exists to prevent).

**Which type values can appear.** `collect_candidates` calls
`pinyin_guess_candidates(instance, 0, 0x1e)`, where `0x1e =
SORT_WITHOUT_LONGER_CANDIDATE | SORT_BY_PHRASE_LENGTH | SORT_BY_PINYIN_LENGTH |
SORT_BY_FREQUENCY` (`flags.rs`). So `LONGER_CANDIDATE` is excluded by
construction, and no `PREDICTED_*` type can appear (those come only from
`pinyin_guess_predicted_candidates*`, which this path never calls). The values
this fixture can carry are `NBEST_MATCH_CANDIDATE`, `NORMAL_CANDIDATE`, and
possibly `ZOMBIE_CANDIDATE` / `ADDON_CANDIDATE`. The §1.4.3 discriminator is
`NORMAL_CANDIDATE` (a phrase-lookup candidate) vs `NBEST_MATCH_CANDIDATE` (a
sentence-decode candidate). `NORMAL` is **not** asserted here to mean "exactly one
dictionary phrase"; that stronger reading, if needed, is checked in analysis via
`lookup_tokens` with the homograph caveat above.

**Wire format** — `fixtures/w4/oracle-candidate-structure.txt`, in the
`oracle-candidates.txt` house style (`capture-fixtures.md`): a `#` header block
(`# pinyin-oracle-candidate-structure-v1`, `# pin_ref=…`, `# corpus=…`,
`# depth=10`, `# total_triples=…`), then one TAB record per (input, rank):

```text
# format: input<TAB>rank<TAB>candidate_text<TAB>type<TAB>nbest_index
```

`type` is the public enum **name** (e.g. `NORMAL_CANDIDATE`); `nbest_index` is
the integer, or `-` when the accessor reports none (never a fabricated `0`).
Sorted by input (bytewise) then rank, deduplicated by input, exactly as
`oracle-candidates.txt`. No `begin`, `end`, or `tokens` column — the public API
does not populate them.

- **Portability / constitution.** This reads only libpinyin's **public candidate
  API**, which `pinyin-oracle` already links (`ffi.rs` declares
  `pinyin_get_candidate`, `pinyin_get_candidate_string`). Adding FFI declarations
  for `pinyin_get_candidate_type` / `pinyin_get_candidate_nbest_index` from the
  installed **public header** is the same established practice `ffi.rs` embodies,
  and is categorically distinct from reading upstream **C/C++ implementation**
  (forbidden by AGENTS.md spec discipline). The oracle is a test subject, never
  shipped (`.kiro/steering/structure.md`).
- **What it settles, and what it does not.** It settles §1.4.3: whether the
  oracle's rank-1 (in the near-miss buckets, where we already hold the phrase) is
  a `NORMAL` phrase candidate or an `NBEST_MATCH` sentence candidate — which is
  what decides whether a bigram/sentence pass is even the right lever. It does
  **not** settle §1.4.1 starvation: that is the **absent** bucket, answerable
  largely from *existing* fixtures (oracle rank-1 text vs the segmentations our
  graph admits but top-8 k-best drops), not from W2-CAND, which cannot see
  consumed length.

W2-CAND has landed and is analysed in §7; no lattice or two-pass code is
written until the construction-contract PR (§6 step 3).

## 2. Target behaviour (candidate strategies)

Two constructions are on the table. Neither is frozen here; W2-CAND decides
between them. Both must preserve every invariant in §3 and pass the gates in §4.

### Strategy A — phrase-level lattice edges

**Add** a lattice whose **edges are concrete `PhraseToken`s** (a dictionary phrase
covering a byte span) *alongside* the frozen syllable-path collection — never in
place of it. Path cost is the interpolated model term over the actual phrase
sequence, so **bigrams and phrase costs decide which segmentation survives**, not
just how survivors rank. The emitted list is the frozen prefix / sentence
collection (the §3.1 floor) **unioned** with the lattice's first-position
phrases, then ranked by §3.5. Because §3.1 forbids dropping today's collection, A
must union, not replace; "first phrase of the best sentence plus position-matched
alternatives" is then one candidate-emission rule layered on top.

- **Fixes** §1.4.1 (starvation) for the **absent** bucket: a phrase-cheap
  segmentation is no longer gated behind unigram-only syllable k-best. This is
  the fix `f3-bigram-kbest.md` identified as the real one. It does nothing for
  the near-miss buckets, which are not starved (§1.5).
- **Risk.** It embeds the strongest unobservable assumption — the phrase-token
  "best sentence" emission rule that §1.3 leaves **INFERRED** (§1.3 falsifies only
  the syllable-path reading, not this one). Emission must be specified from
  W2-CAND's type / n-best-index evidence, not assumed. Largest architectural
  change, and the RSS risk of §4.

### Strategy B — two-pass re-rank over the existing graph (primary)

Keep `SegmentGraph` + syllable k-best exactly as frozen. After phrase
collection, run a **second, sentence-level pass** that materialises the induced
phrase sequences and re-ranks them with real bigram transitions, then emits
under the **unchanged** contract of §3.

- **Advantages.** Preserves the frozen syllable path-set as a lower bound
  (§3) with zero parser/graph/k-best change; touches no trait signature (it
  re-uses `Scorer::rank_phrases` and `LanguageModel::score`); any data-side
  growth is a **defaulted** trait method (see §2.1); directly measurable against
  the §4 gates without betting the architecture on an unobservable emission rule.
- **Known limit.** B re-ranks only what k-best already surfaced; it does **not**
  fix §1.4.1 starvation (the absent bucket), and for the dominant near-miss
  buckets its bigram pass is inert wherever the oracle's rank-1 is a `NORMAL`
  single-phrase candidate (§1.4.3) or the first position has empty history
  (§1.4.2). Whether that limit matters is exactly the §1.4.3 question W2-CAND
  answers.

**Primary: Strategy B**, as the minimal change that keeps the frozen seams and is
honestly measurable. **Strategy A is the measured alternative**, escalated to only
if analysis of the **absent** bucket — the only place §1.4.1 starvation can live,
the near-miss residual being ruled out by §1.5 — shows a starved-segmentation
population large enough to move the §0 pins. A must never be justified by the
dominant near-miss residual, which is a ranking/calibration problem A does not
touch. This is an evidence-gated decision, not a taste one.

### 2.1 Reachability via defaulted trait methods

Stage-1 call sites must stay valid. The `Dictionary`, `LanguageModel` (and
`UserModel`) traits are unsealed and **grow only by defaulted methods**
(`.kiro/steering/structure.md`; the pattern F4 assumed in
`parity-climb-residual.md`).

- **Strategy B** needs **no new trait method**: it is engine-local
  (`Session::refresh` gains a second pass) and reuses `Scorer::rank_phrases`
  (already history-aware) and `LanguageModel::score(history, token, edge_cost)`.
- **Strategy A** may want a phrase-lattice enumerator — "phrases beginning at
  byte `b`" — richer than the current exact-key `Dictionary::lookup(&[keys])`.
  If added, it is a **defaulted** `Dictionary` method with a fallback expressed
  over existing lookups, so every current `impl` and Stage-1 call site keeps
  compiling. `EdgeCost` (`kbest.rs`) and the session API (`session-api.md`)
  signatures are **not** touched by either strategy.

## 3. Compatibility invariants (normative, unchanged by either strategy)

1. **Syllable path-set is a lower bound.** The candidate set must remain a
   superset of what the frozen `SegmentGraph` + k-best admit today
   (`segment-graph.md`, `kbest-search.md`). The only accepted exceptions are the
   already-frozen apostrophe residuals: the leading (`'ni`) and doubled
   (`ni''hao`) apostrophe path disagreements (`segment-graph.md`), and the
   apostrophe-only abort guard (`docs/findings/oracle-apostrophe-abort.md`,
   F-E-14). No new exception is introduced.
2. **Public session API unchanged.** `session-api.md` is frozen: `Candidate`
   (text, kind, `consumed_keys`, `consumed_bytes`, cost), `CandidateList`,
   `Session::{candidates, select, commit}` semantics, and `select` consuming the
   candidate's bytes. Candidate emission may change; the surface may not.
3. **Parser untouched.** `parser-spec.md` and `FullPinyinParser` are not
   modified. `F1`'s `type_pinyin` / `process_key` accept-set split
   (`f1-junk-aware-parse.md`) stands.
4. **Integer fixed-point cost throughout.** `Cost` is `i64` on the
   negative-log₂ scale of `scoring-spec.md` (`COST_PER_BIT = 1000`,
   `UNKNOWN_COST = 40000`), saturating, **no floats anywhere** (constitution
   item 6; determinism across OSes).
5. **Determinism.** Output stays a pure function of (input, user state, config).
   The **pooled candidate list** is a stable sort by `Candidate::cost`; equal-cost
   candidates keep **insertion order from the collection loops** (path × prefix
   length × `rank_phrases` order), which is what `Session::refresh` does today.
   `kbest-search.md`'s total order governs the **syllable-path** stage only, not
   the pooled list — `Candidate` carries no edge id to tie-break on. No clock,
   locale, or environment read enters.
6. **k-bound family respected.** `SEGMENTATION_K = 8`, `MAX_CANDIDATES = 64`,
   `MAX_PHRASE_KEYS = 8`, `MAX_K = 4096`. Any growth is justified against wall
   and RSS in §4 and re-pinned deliberately.

## 4. Measurement gates (runnable as written)

Any construction change is gated on all of the following. Because the five
`assert_eq!` pins in §0 are bit-exact, **they will trip by design on any ranking
change** — re-pinning them is a deliberate, reviewed step (state Δ against
6525 / 9232 / 70 / 65505–98930 in the commit that re-pins), never a silent
edit. The tolerant floors (top-1 ≥ 55%, top-5 ≥ 80%, absent ≤ 4%) are the
regression envelope that must hold regardless.

1. **Portable parity (primary metric).**
   ```bash
   cargo test --release --locked -p pinyin-oracle \
       --test real_tables_integration -- --nocapture
   ```
   Report Δ top-1, Δ top-5-set, Δ absent, Δ prefix-10 overlap against
   6525 / 9232 / 70 / 65505 of 98930. Requires the exported tables at
   `/tmp/pinyin-rs-export` (`pinyin-migrate export`); the test skips without
   them and is measured under `--release`, per
   `crates/pinyin-oracle/tests/real_tables_integration.rs`.
2. **Thread-order independence.**
   ```bash
   PARITY_SERIAL=1 cargo test --release --locked -p pinyin-oracle \
       --test real_tables_integration -- --nocapture
   ```
   Serial and parallel runs must agree bit-for-bit; a change that only moves a
   number under parallelism is a determinism bug, not a parity gain.
3. **Rank-1 recovery and bucket movement.**
   ```bash
   cargo run -p pinyin-oracle --release --bin parity-worst
   ```
   Report how often the oracle's rank-1 becomes our rank-1, and the movement of
   the rank 2–5 / 6–9 / 10+ / absent buckets in §1.5.
4. **Wall-clock and RSS** against the post-PR-#32 baseline (~146s parallel on 12
   cores, shared read-only tables). A phrase lattice (Strategy A) is the RSS risk
   — report peak RSS, not just wall.
5. **Path-set / tie-swap accounting.** Any growth in path-set size (k-bound
   family, §3.6) or in tie-swap count must be reported and justified. A larger
   path set that does not move top-1 is a cost with no benefit and is rejected.

A change is acceptable only if the tolerant floors hold, serial == parallel, and
the Δ against the pins is a net parity gain the re-pin commit records
explicitly.

## 5. Non-goals

Out of scope for this SPEC and any construction change it gates:

- Trigram / Kneser-Ney or any LM order change; new smoothing.
- Fuzzy / typo / abbreviation edges (`EdgeKind` Stage-2 variants stay
  undeclared, `segment-graph.md`).
- User-model learning, per-user adaptation, prediction candidates.
- C-ABI (`pinyin-capi`) or session-API (`session-api.md`) signature changes.
- Model-data redistribution (`model-provenance.md`).
- Parser / path-set changes (`parser-spec.md`, `parser-path-set.md`).
- Reading upstream C/C++ implementation. W2-CAND reads only the public FFI
  candidate API the harness already links (§1.6).

## 6. Process note

1. **Characterise** — this document (done).
2. **Capture** — land W2-CAND (§1.6) and analyse
   `fixtures/w4/oracle-candidate-structure.txt`: how many oracle rank-1s are
   `NORMAL` phrase candidates vs `NBEST_MATCH` sentence candidates (§1.4.3).
   Separately, from *existing* fixtures, measure how often an **absent** oracle
   rank-1 needs a segmentation our top-8 k-best starves (§1.4.1). **This is the
   gate.**
3. **Freeze** — only then extend this SPEC with the frozen construction contract
   (Strategy A or B) and its re-pinned §4 numbers.
4. **Implement** — only from the frozen document.

No agent writes lattice or two-pass code before step 3. If step 2 shows the
residual is dominated by unigram calibration rather than construction (the
§1.4.3 single-phrase regime), the correct outcome is to record that as a second
negative result alongside `f2-unigram-tiebreak-sweep.md` and
`f3-bigram-kbest.md` — not to build a lattice that cannot move the number.

STOP and report rather than inventing evidence.

## 7. W2-CAND: the captured data and the §1.4.3 answer

Date: 2026-08-13 · Status: **captured and analysed. §1.4.3 answered for the W2
corpus; the token-count sub-question stays open, unreachable from the public
API.** This section records step 2 of §6; it does not freeze a construction
contract (step 3), which remains a later PR.

### 7.1 What was captured

`fixtures/w4/oracle-candidate-structure.txt`, produced by
`cargo run -p pinyin-oracle --features oracle-ffi --bin oracle-candidate-structure`
over the pinned oracle (`Session::observe_candidate_infos`, the same
`pinyin_guess_candidates(_, 0, 0x1e)` stage as `oracle-candidates.txt`). Wall
clock: **3.6 s** for the whole corpus — the observe path is fast; the "~3h"
note on the older freshness test is not representative of a single capture pass.

Provenance matches the sister fixture exactly: `total_inputs=10465 (distinct
10312)`, `total_triples=97442 (distinct inputs with candidates 10037)`. The
portable test `structure_fixture_matches_sister_triples`
(`crates/pinyin-oracle/tests/candidate_structure.rs`) asserts the two files'
`(input, rank, candidate_text)` columns are byte-identical, so the type /
n-best columns are read against the *same* candidates the pinned metrics use.

### 7.2 A capture constraint discovered at runtime

`pinyin_get_candidate_nbest_index` **asserts** its candidate is an
`NBEST_MATCH_CANDIDATE` and `abort()`s otherwise. Observed directly, not read
from source:

```text
pinyin.cpp:2881: bool pinyin_get_candidate_nbest_index(...):
  Assertion `NBEST_MATCH_CANDIDATE == candidate->m_candidate_type' failed.
```

This is the same uncatchable-`abort()` class as
`docs/findings/oracle-apostrophe-abort.md` (F-E-14), and it is handled the same
way: a guard derived from the *observed* abort, not from upstream layout. The
capture calls the accessor **only** for `NBEST_MATCH_CANDIDATE`; for every other
type the n-best index is absent and written `-`. So `nbest_index` is present iff
the type is `NBEST_MATCH_CANDIDATE`. Refining §1.6's "record
`pinyin_get_candidate_nbest_index` → the n-best index": the field is defined only
for n-best candidates, and the frozen `type` column already tells you which
those are.

### 7.3 The distribution (worked from the fixture)

Over all 97,442 triples and all 10,037 rank-1 rows, **every candidate is
`NORMAL_CANDIDATE`**. The `type` column has exactly one distinct value across
the file, and the `nbest_index` column has exactly one distinct value (`-`).
There are **zero** `NBEST_MATCH_CANDIDATE`, and none of `ZOMBIE` / `ADDON` /
`LONGER` / any `PREDICTED_*` either.

| Slice | `NORMAL_CANDIDATE` | `NBEST_MATCH_CANDIDATE` | other types | rows with `nbest_index` ≠ `-` |
|---|---:|---:|---:|---:|
| all triples | 97,442 (100%) | 0 | 0 | 0 |
| rank-1 only | 10,037 (100%) | 0 | 0 | 0 |

Verbatim, the cross-segmentation cases §1.2–§1.3 turn on — both segmentations,
every rank, are `NORMAL`:

```text
xian	1	西安	NORMAL_CANDIDATE	-
xian	4	县	NORMAL_CANDIDATE	-
fangan	1	方案	NORMAL_CANDIDATE	-
fangan	2	反感	NORMAL_CANDIDATE	-
```

The coverage cases §1.2 cites, including the 3-syllable `中国人`:

```text
nihao	1	你好	NORMAL_CANDIDATE	-
zhongguoren	1	中国人	NORMAL_CANDIDATE	-
zhongguoren	2	中国	NORMAL_CANDIDATE	-
```

Even the longest corpus input (eight apostrophe-separated syllable groups) is
`NORMAL` at every rank:

```text
bengqiu'nangcha'gongwei'mianduan'meiban'nengna'dangdong'sheng	1	崩	NORMAL_CANDIDATE	-
```

### 7.4 The §1.4.3 answer

The §1.4.3 discriminator was `NORMAL_CANDIDATE` (a phrase-lookup candidate) vs
`NBEST_MATCH_CANDIDATE` (a sentence-decode candidate). Over the W2 corpus it
resolves **entirely to `NORMAL`**:

1. **Type across the corpus, and at rank-1.** 100% `NORMAL_CANDIDATE` (§7.3).
   The oracle's candidate list, under the frozen `0x18a` flags and the `0x1e`
   sort — which does **not** set `SORT_WITHOUT_SENTENCE_CANDIDATE`, so n-best
   candidates are *not* being suppressed — contains no sentence-decode candidate
   anywhere in this corpus.
2. **n-best index at rank-1.** No rank-1 (indeed no candidate at any rank)
   carries an n-best index; all are `-`. The question "is rank-1 systematically
   `nbest_index == 0`" is therefore **moot**: no candidate is an n-best
   candidate, so the index never exists. That absence *is* the answer.
3. **Bearing on the construction hypothesis.** The oracle's rank-1 is never a
   bigram-linked n-best *sentence* candidate over this corpus; it is always a
   phrase-lookup candidate. This is the evidence §1.4.3 was after: it removes
   the "n-best sentence candidate" explanation for the near-miss residual and
   points the residual at the ranking of phrase-lookup candidates — unigram
   phrase-cost / coverage calibration — consistent with the negative results in
   `f2-unigram-tiebreak-sweep.md` and `f3-bigram-kbest.md`. For Stage-1 parity
   against this pinned fixture no sentence-level lever is needed; whether the
   oracle's production configuration (which does run the sentence pass) would
   emit `NBEST_MATCH` candidates is a separate question this capture does not
   settle.

### 7.5 What this capture cannot determine (open sub-questions)

Per §1.6, the public API yields no segmentation, no token sequence, no offsets,
and no score, so:

- **`NORMAL` is not "exactly one dictionary phrase."** The type says a candidate
  is a phrase-lookup candidate, not that it decomposes into a single token. `中
  国人` at `zhongguoren` rank-1 is `NORMAL`, but whether it is one 3-syllable
  dictionary phrase or a shorter phrase plus a continuation is **not**
  determinable from the type; it would need the token decomposition, which
  `lookup_candidate_t` does not expose. The half of §1.4.3 that asks "single
  phrase vs bigram-linked two-phrase sentence" is answered only in the
  **negative** (not an n-best sentence candidate); the **positive** half (one
  token vs several) stays open.
- **Segmentation and consumed length remain unknowable** from this capture, so
  §1.4.1 starvation stays an *absent-bucket* question answered from existing
  fixtures (§1.6), not from W2-CAND.
- **Scope.** All of §7 is scoped to `fixtures/w4/oracle-candidate-structure.txt`
  (the W2 parity corpus, 10,465 inputs) under flags `0x18a` / sort `0x1e`. It
  does not claim the oracle never emits `NBEST_MATCH` on other inputs or
  profiles; it claims none appears here.

These open sub-questions are for the construction-contract PR (§6 step 3), not
this one. STOP and report rather than inventing evidence.
