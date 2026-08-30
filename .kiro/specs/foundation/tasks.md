# Implementation Plan: Foundation

## Overview

Foundation is complete except the consolidated F-E evidence register (task
4's second half). Phase 0 remains a hard gate for any new `[B]` slice: no
implementation task is `ready` until its cited behavioural SPECs and
fixtures are human-frozen.

## Tasks

- [x] 1. **[H]** Record the source-built oracle reference freeze.
  - Record release tags, commit SHAs, source/data URLs and SHA-256 values.
  - Verify the canonical recipe emits the pinned shared-object path.
  _Requirements: R0_

- [x] 2. **[H]** Write the provenance finding with Branch A/A′/B declaration.
  - `docs/findings/model-provenance.md` (2026-08-14): no redistribution,
    build-time fetch permitted; Branch B (optional vendoring route), not a
    shipping gate.
  _Requirements: R1_

- [x] 3. **[A]** Capture the upstream schema verbatim with source ref and hash.
  _Requirements: R2_

- [ ] 4. **[A]** Build the F-A/F-C capture harness and establish the F-E cross-lane evidence register.
  - Done: the harness (`tools/capture/`) and the F-A/F-C freeze
    (`docs/findings/capture-fixtures.md`, `fixtures/foundation/f-a.txt`,
    `f-c.txt`).
  - Open: the consolidated F-E register — the 13 cases' evidence is
    recorded across the findings docs, but the register itself (all 13
    cases, each with reproducible evidence in its applicable lane) is not
    yet one artifact on main.
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

- [x] 5. **[A]** Define core traits and signatures with doc comments only.
  - `Dictionary`/`UserModel`/`LanguageModel`/`InputParser` in
    `crates/oxpinyin-core/src/lib.rs`, under `#![forbid(unsafe_code)]`.
  _Requirements: R6_

- [x] 6. **[B]** Add the syllable table as static data with a table-driven test.
  - `crates/oxpinyin-core/src/syllables.rs`.
  _Requirements: R3.6_

- [x] 7. **[B]** Implement longest-match parsing with backtracking, apostrophes,
  partial input and alternatives.
  - `crates/oxpinyin-core/src/parser.rs`, per the frozen parser/path-set
    SPECs.
  _Requirements: R3.1, R3.2, R3.3, R3.4, R3.7_

- [x] 8. **[B]** Run parser acceptance against F-A and add totality properties.
  - `crates/oxpinyin-core/tests/parser_acceptance.rs`; `proptest` totality
    and determinism properties.
  _Requirements: R3.5, R5_

- [x] 9. **[B]** Add the parser fuzz target and short CI fuzz pass.
  - `fuzz/fuzz_targets/parser.rs`; the bounded fuzz job in
    `.github/workflows/ci.yml`.
  _Requirements: R3.5_

- [x] 10. **[A]** Establish the drop-in replacement surface.
  - SONAME `libpinyin.so.15` and consumer-union scope 58/58 (#206);
    header under `libpinyin-2.11.91/`, `libpinyin.pc`, installed naming
    (#192).
  _Requirements: R7_

- [x] 11. **[B]** Implement the compat read path and measure it on the distro backends.
  - `CompatLayout` detection + `MemoryChunk` reader (#228); measured
    order-only with sets byte-identical, 1,571/1,571 rows, on Fedora
    rawhide (Kyoto Cabinet), Debian testing (tkrzw) and NixOS (Kyoto
    Cabinet); the divergence attributed to R1's defined-order rule.
  _Requirements: R8, R9_

- [x] 12. **[A]** Record the output-compatibility policy.
  - `docs/findings/compatibility-policy.md`: the four exception classes
    and the E2E I/O rule; compact steering copy in
    `.kiro/steering/compatibility-policy.md`.
  _Requirements: R9_
