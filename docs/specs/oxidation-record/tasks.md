# Implementation Plan — Oxidation record

## Overview

Follow the [source method](../../source-policy.md#method): record, design and analyse, implement, then gate.

## Tasks

- [x] 1. Survey each subsystem and record component layout, coefficients and open gaps (2026-10-16 UTC).
  That coefficient inventory is partial; task 11 completes it.
  _Requirements: 1, 5.1_

- [x] 2. Rule the method (2026-10-16 UTC).
  Stage 2 requires bug-for-bug compatibility and permits compatible internal improvement.
  Record, design and analyse, then implement.
  Use pin coefficients and idiomatic Rust.
  Never worsen both complexity axes.
  Try the pin's algorithm first for performance gaps.
  _Requirements: 2, 3, 5, 6_

- [x] 3. Add the full-pinyin distance tie-break.
  Read `src/storage/pinyin_parser2.cpp:217-344` at the pin.
  Carry each edge's distance and compare total distance third in `SegmentGraph::fewest_keys`.
  Add Requirement 4 inputs to the existing parser differential on the reference cell, with revert-and-check.
  Update record §2 and §11.
  Done in #832 (2026-10-17 UTC): inputs and pair sweep entered the option-sweep differential with reference-cell revert-and-check.
  Record §2 and §11 were updated 2026-10-18 UTC.
  _Requirements: 3.3, 4.1, 4.2_

- [x] 4. Determine whether a C-ABI configuration reaches `has_real_unigrams` false and reads ScoringConfig weights.
  None does: RuntimeLm forwards to BigramLanguageModel, which always reports real unigrams (§11, 2026-10-16 UTC).
  _Requirements: 2.3_

- [ ] 4a. Remove the k-best fallback and fitted ScoringConfig weights, or replace them with a pin-derived rule.
  Leave no fitted coefficient in the tree.
  This is internal work, not a compatibility gate.
  The weights affect neither C ABI.
  Check in-tree Rust callers and test/tool consumers before removal.
  Rust signatures need no interface-change approval since 2026-10-18 UTC.
  _Requirements: 2.3_

- [ ] 5. Write §11's equivalence arguments.
  Cover matrix-column deduplication, candidate deduplication, constraint-drop tests and the `nstore = 1` store.
  Include compact_libraries, export per-pronunciation order, PinyinDirectParser2 and double pinyin's empty first yunmu slot.
  Make each failed argument a separate defect task.
  _Requirements: 3_

- [ ] 6. Argue every remaining "different / unverified" row in §§1–8.
  Compare time and space complexity against the pin.
  _Requirements: 3.1, 3.2_

- [ ] 7. Replace SEARCH_*/ERROR_* integer codes in `oxpinyin-user/src/phrase_table.rs` with an enum or Result.
  Behaviour does not depend on those codes (§10).
  Update §10.
  This is internal work, not a compatibility gate.
  _Requirements: 5.2_

- [ ] 8. Update recorded structures and algorithms in the same PR that changes them.
  _Requirements: 1.4_

- [ ] 9. Move KeyInput, process_key, type_pinyin, Preedit and commit off Session into an optional adapter over operations.
  Move their oracle-tool and test consumers to operations or the adapter.
  Both C ABIs already use operations only (record §0, "The operations").
  This originally required a maintainer decision on the Rust surface.
  Since 2026-10-18 UTC, no interface STOP applies to that internal surface.
  _Requirements: 7_

- [ ] 10. Score sentences as the pin does (record §11, "Sentence scoring: gfloat and libm").
  Use f32/f64 at the pin's conversion points and in its operation order.
  Verify multiply-add contraction and resolved log for pin libpinyin.so and oxpinyin's shipping build.
  None of this is verified yet.
  Investigate remaining differences instead of attributing them to the host.
  Gate on the reference-cell sentence-surface differential.
  Obtain human rulings on the class (a) register entry and compatibility-policy changes before landing.
  _Requirements: 4.3_

- [ ] 11. Complete the parity-path coefficient and limit inventory (record §9).
  Include output-shaping constants, inline literals and loop bounds.
  Give each its pin value.
  Where no pin value exists, identify the call path and whether it reaches parity output.
  _Requirements: 2.1_

- [x] 12. Investigate the constraint-revalidation expansion cap (record §11).
  Discovery used a source-built pin, not the hash-pinned oracle, with a mini fixture and user-imported phrases.
  Unconstrained guessing was the control.
  Filed as #825 (2026-10-16 UTC).
  _Requirements: 3.3_

- [x] 12a. Fix and verify against the reference.
  Done in #828: revalidation ports validate_constraint.
  Contract-diff constraint-survives-* and constraint-stale-item-* cases match the hash-pinned reference cell with full model20.
  Revert-and-check used the parent build (record §11).
  Scope is user-imported phrases.
  Six original inputs select the second phrase from its NORMAL row; the reconcile pair uses NBEST.
  _Requirements: 3.3_

- [ ] 12b. Run the original acceptance case, still unrun.
  Use a pinned-model phrase, not a user import, whose abbreviation expands past 4096.
  Force it, then revalidate through guess_sentence and a replaced selection.
  Compare with the reference-cell oracle using a contract-diff.py case.
  _Requirements: 3.3_
