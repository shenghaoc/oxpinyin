# Implementation Plan — Oxidation record

## Overview

Tiers as in `.kiro/steering/agents-protocol.md`: `[A]` architect,
`[B]` basic, `[H]` human. Each task follows the steering's order of
work: record, design and analyse, implement, gate.

## Tasks

- [x] 1. `[A]` Survey every subsystem against the pin and write the first
  record, with the component layout, the coefficients and the open gaps
  (2026-10-16 UTC). The coefficient inventory it took is partial; task
  11 completes it.
  _Requirements: 1, 5.1_

- [x] 2. `[H]` Rule the method: bug-for-bug compatibility as Stage 2's
  acceptance requirement, with internal improvement permitted alongside
  it when compatibility is preserved; record, design
  and analyse, implement; bug for bug; coefficients from the pin;
  idiomatic Rust; never both worse; performance gaps try the pin's
  algorithm first
  (`.kiro/steering/oxidation-method.md`, 2026-10-16 UTC).
  _Requirements: 2, 3, 5, 6_

- [x] 3. `[A]` Add the distance tie-break to the full-pinyin parse.
  Read `src/storage/pinyin_parser2.cpp:217-344` at the pin; carry each
  edge's distance and compare the per-node total third in
  `SegmentGraph::fewest_keys`. Add the Requirement 4 inputs as cases to
  the existing parser differential on the reference cell, with
  revert-and-check. Update the record's §2 and §11. Done in #832
  (2026-10-17 UTC): the inputs and a pair sweep were added to the
  option-sweep differential, with revert-and-check on the reference
  cell; the record's §2 and §11 were updated 2026-10-18 UTC.
  _Requirements: 3.3, 4.1, 4.2_

- [x] 4. `[A]` Establish whether any configuration reachable through
  the C ABI takes the `has_real_unigrams`-false branch that reads the
  `ScoringConfig` weights. None does: `RuntimeLm` forwards to
  `BigramLanguageModel`, which always reports real unigrams (§11,
  2026-10-16 UTC).
  _Requirements: 2.3_

- [ ] 4a. `[A]` Internal work, not a compatibility gate: remove the
  k-best fallback and its fitted `ScoringConfig` weights, or replace
  them with a rule derived from the pin, so no fitted coefficient
  remains in the tree. The weights do not affect either C ABI; before
  removal, check supported Rust callers and test and tool consumers,
  and get any interface-change approval the removal needs.
  _Requirements: 2.3_

- [ ] 5. `[A]` Write the equivalence arguments listed in §11 —
  matrix column dedup, candidate dedup, the constraint drop test, the
  `nstore = 1` store, `compact_libraries`, the export iterators'
  per-pronunciation order, `PinyinDirectParser2`, and double pinyin's
  empty first yunmu slot. Any argument that fails becomes its own
  defect task.
  _Requirements: 3_

- [ ] 6. `[A]` Write the arguments for the remaining rows marked
  "different / unverified" in §§1–8, each with time and space complexity
  against the pin.
  _Requirements: 3.1, 3.2_

- [ ] 7. `[B]` Internal work, not a compatibility gate: replace the
  pin's `SEARCH_*` / `ERROR_*` integer codes in
  `oxpinyin-user/src/phrase_table.rs` with an enum or `Result`;
  behaviour does not depend on them (§10). Update §10.
  _Requirements: 5.2_

- [ ] 8. `[A]` Keep the record current: a change that alters a recorded
  structure or algorithm updates its row in the same PR.
  _Requirements: 1.4_

- [ ] 9. `[A]` Move the key-driven layer (`KeyInput`, `process_key`,
  `type_pinyin`, `Preedit`, `commit`) off `Session` into an optional
  adapter over the operations, and move the oracle tools and tests that
  use it onto the operations or the adapter. Both C ABIs already use only
  the operations (record §0, "The operations"). This is a change to the
  supported Rust surface, so it starts with a STOP for the maintainer's
  ask.
  _Requirements: 7_

- [ ] 10. `[A]` Score sentences as the pin does (record §11, "Sentence
  scoring: gfloat and libm"): `f32` and `f64` at the pin's conversion
  points and its order of operations. For the identified artifacts —
  the pin's `libpinyin.so` and oxpinyin's shipping build — verify any
  multiply-add contraction and which `log` implementation each resolves;
  none of this is verified yet. Investigate any difference that remains
  rather than attributing it to the host. Gate on the sentence-surface
  differential on the reference cell. The class (a) register entry and
  the compatibility policy change with it: `[H]` rules on both before
  the task lands.
  _Requirements: 4.3_

- [ ] 11. `[A]` Complete the coefficient and limit inventory: every
  output-shaping constant and limit on the parity path, including those
  written inline (literals in expressions, loop bounds), each with its
  pin value or, when it has none, its call path and whether it reaches
  the parity path (record §9).
  _Requirements: 2.1_

- [x] 12. `[A]` Investigate the constraint-revalidation expansion cap
  (record §11): discovery. Reproduced against a source build of the pin
  (not the hash-pinned oracle) on the mini fixture with user-imported
  phrases, unconstrained guessing as the control, and filed as #825
  (2026-10-16 UTC).
  _Requirements: 3.3_

- [x] 12a. `[A]` Fix and reference verification. Done in #828: the
  revalidation ports `validate_constraint`, and the
  `constraint-survives-*` and `constraint-stale-item-*` cases in
  `contract-diff.py` match the hash-pinned oracle on the reference cell
  with full model20, with revert-and-check against the parent build
  (record §11). Scope: user-imported phrases; the six original inputs
  choose the second phrase from its NORMAL row, the reconcile pair from
  its NBEST row.
  _Requirements: 3.3_

- [ ] 12b. `[A]` The original acceptance case, not yet run: a phrase of
  the pinned model (not a user import) whose abbreviation expands past
  4096, forced and then revalidated through `guess_sentence` and a
  replaced selection, compared with the oracle on the reference cell as
  a case in `contract-diff.py`.
  _Requirements: 3.3_
