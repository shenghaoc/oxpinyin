# Implementation Plan: Foundation

## Overview

Phase 0 remains a hard gate: no `[B]` implementation task is `ready` until
its cited behavioural SPECs and fixtures are human-frozen. Each scoped slice
uses a merged Architect derivation PR before its separate implementation PR.

## Tasks

- [x] 1. **[H]** Record the source-built oracle reference freeze.
  - Record release tags, commit SHAs, source/data URLs and SHA-256 values.
  - Verify the canonical recipe emits the pinned shared-object path.
  _Requirements: R0_

- [ ] 2. **[H]** Write the provenance finding with Branch A/A′/B declaration.
  _Requirements: R1_

- [x] 3. **[A]** Capture the upstream schema verbatim with source ref and hash.
  _Requirements: R2_

- [ ] 4. **[A]** Build the F-A/F-C capture harness and establish the F-E cross-lane evidence register.
  - Depend on tasks 1 and 3; follow `docs/findings/spec-derivation.md`.
  - Register all 13 F-E cases now; attach each case's reproducible evidence in its applicable lane.
  - `F-E-01`: #566 NULL key-rest (`nih`, select valid prefix).
  - `F-E-02`: candidate-processing invalid access/session replay.
  - `F-E-03`: historical save-path configuration race.
  - `F-E-04`: asynchronous cloud `user_data` lifetime leak.
  - `F-E-05`: table import/export early-return resource leak.
  - `F-E-06`: libpinyin data-tool internal leaks.
  - `F-E-07`: high or unbounded memory growth.
  - `F-E-08`: English-mode use-after-free.
  - `F-E-09`: i686 binary-generation invalid access.
  - `F-E-10`: sparc64 unaligned-access bus error.
  - `F-E-11`: #179 stale Berkeley DB lock hang.
  - `F-E-12`: #542 assertion on user input (`zhuan`).
  - `F-E-13`: #518 cloud/proxy foreign-library crash.
  _Requirements: R5_

- [ ] 5. **[A]** Define core traits and signatures with doc comments only.
  _Requirements: R6_

- [ ] 6. **[B]** Add the syllable table as static data with a table-driven test.
  - Depend on task 5 and the frozen parser SPEC.
  _Requirements: R3.6_

- [ ] 7. **[B]** Implement longest-match parsing with backtracking, apostrophes,
  partial input and alternatives.
  - Depend on tasks 5 and 6 plus frozen parser/path-set SPECs.
  _Requirements: R3.1, R3.2, R3.3, R3.4, R3.7_

- [ ] 8. **[B]** Run parser acceptance against F-A and add totality properties.
  - Depend on task 7 and frozen F-A.
  _Requirements: R3.5, R5_

- [ ] 9. **[B]** Add the parser fuzz target and short CI fuzz pass.
  - Depend on task 7.
  _Requirements: R3.5_
