# Implementation Plan — Oxidation record

## Overview

Tiers as in `.kiro/steering/agents-protocol.md`: `[A]` architect,
`[B]` basic, `[H]` human. Each task follows the steering's order of
work: record, design and analyse, implement, gate.

## Tasks

- [x] 1. `[A]` Survey every subsystem against the pin and write the first
  record, with the component layout, the coefficients and the open gaps
  (2026-10-16 UTC).
  _Requirements: 1, 2.1, 5.1_

- [x] 2. `[H]` Rule the method: compatibility in the first release,
  elegance, extensibility and performance afterwards; record, design
  and analyse, implement; bug for bug; coefficients from the pin;
  idiomatic Rust; never both worse; performance gaps try the pin's
  algorithm first
  (`.kiro/steering/oxidation-method.md`, 2026-10-16 UTC).
  _Requirements: 2, 3, 5, 6_

- [ ] 3. `[A]` Add the distance tie-break to the full-pinyin parse.
  Read `src/storage/pinyin_parser2.cpp:217-344` at the pin; carry each
  edge's distance and compare the per-node total third in
  `SegmentGraph::fewest_keys`. Add the Requirement 4 inputs as cases to
  the existing parser differential on the reference cell, with
  revert-and-check. Update the record's §2 and §11.
  _Requirements: 3.3, 4_

- [x] 4. `[A]` Establish whether any configuration reachable through
  the C ABI takes the `has_real_unigrams`-false branch that reads the
  `ScoringConfig` weights. None does: `RuntimeLm` forwards to
  `BigramLanguageModel`, which always reports real unigrams (§11,
  2026-10-16 UTC).
  _Requirements: 2.3_

- [ ] 4a. `[A]` After the first release: remove the k-best fallback and
  its fitted `ScoringConfig` weights, or replace them with a rule
  derived from the pin, so no fitted coefficient remains in the tree.
  _Requirements: 2.3_

- [ ] 5. `[A]` Write the equivalence arguments listed in §11 —
  matrix column dedup, candidate dedup, the constraint drop test, the
  `nstore = 1` store, `compact_libraries`, the export iterators'
  per-pronunciation order, `PinyinDirectParser2`, and double pinyin's
  empty first yunmu slot. Any argument that fails becomes its own
  defect task.
  _Requirements: 3_

- [ ] 6. `[A]` Write the arguments for the remaining "same behaviour"
  and "different" rows in §§1–8, each with time and space complexity
  against the pin.
  _Requirements: 3.1, 3.2_

- [ ] 7. `[B]` After the first release: replace the pin's `SEARCH_*` /
  `ERROR_*` integer codes in `oxpinyin-user/src/phrase_table.rs` with an
  enum or `Result`; behaviour does not depend on them (§10). Update §10.
  _Requirements: 5.2_

- [ ] 8. `[A]` Keep the record current: a change that alters a recorded
  structure or algorithm updates its row in the same PR.
  _Requirements: 1.4_

- [ ] 9. `[A]` Move the key-driven layer (`KeyInput`, `process_key`,
  `type_pinyin`, `Preedit`, `commit`) off `Session` into an optional
  adapter over the operations, and move the oracle tools and tests that
  use it onto the operations or the adapter. The C ABI already uses only
  the operations (record §0, "The operations"). This is a change to the
  supported Rust surface, so it starts with a STOP for the maintainer's
  ask.
  _Requirements: 7_
