# Implementation Plan: Foundation

## Overview

Foundation is complete.
Task 4 closed 2026-09-08 against `docs/findings/robustness-evidence.md`.

## Tasks

- [x] 1. Record the source-built oracle reference freeze.
  Record release tags, SHAs, source/data URLs and SHA-256 values.
  Verify that the canonical recipe emits the pinned shared-object path.
  _Requirements: R0_

- [x] 2. Record provenance with the Branch A/A′/B declaration.
  `docs/findings/model-provenance.md` (2026-08-14) permits build-time fetch without redistribution.
  It selects Branch B, optional vendoring, and is not a shipping gate.
  _Requirements: R1_

- [x] 3. Capture the upstream schema verbatim with its source reference and hash.
  _Requirements: R2_

- [x] 4. Build the F-A/F-C capture harness and establish the F-E register.
  The harness in `tools/capture/` is done.
  The freeze is in `docs/testing/capture-fixtures.md`, `fixtures/foundation/f-a.txt` and `f-c.txt`.
  The consolidated register is `docs/findings/robustness-evidence.md`.
  It contains 14 cases with lanes and reproducible evidence, closed 2026-09-08.
  These are the original 13 cases plus F-E-14, the apostrophe-only oracle abort.
  - F-E-01: #566 NULL key-rest (`nih`, select valid prefix).
  - F-E-02: candidate-processing invalid access/session replay.
  - F-E-03: historical save-path configuration race.
  - F-E-04: asynchronous cloud user_data lifetime leak.
  - F-E-05: table import/export early-return resource leak.
  - F-E-06: data-tool internal leaks.
  - F-E-07: high or unbounded memory growth.
  - F-E-08: English-mode use-after-free.
  - F-E-09: i686 binary-generation invalid access.
  - F-E-10: sparc64 unaligned-access bus error.
  - F-E-11: #179 stale Berkeley DB lock hang.
  - F-E-12: #542 caller-input assertion (`zhuan`).
  - F-E-13: #518 cloud/proxy foreign-library crash.
  _Requirements: R5_

- [x] 5. Define core traits and signatures with doc comments only.
  Dictionary, UserModel, LanguageModel and InputParser reside in `crates/oxpinyin-core/src/lib.rs` under `#![forbid(unsafe_code)]`.
  _Requirements: R6_

- [x] 6. Add the static syllable table and table-driven test.
  The source is `crates/oxpinyin-core/src/syllables.rs`.
  _Requirements: R3.6_

- [x] 7. Implement longest-match parsing with backtracking, apostrophes, partial input and alternatives.
  `crates/oxpinyin-core/src/parser.rs` follows the frozen parser/path-set SPECs.
  _Requirements: R3.1, R3.2, R3.3, R3.4, R3.7_

- [x] 8. Run F-A parser acceptance and add totality properties.
  Use `crates/oxpinyin-core/tests/parser_acceptance.rs` and proptest totality/determinism properties.
  _Requirements: R3.5, R5_

- [x] 9. Add parser fuzzing and a short CI pass.
  Use `fuzz/fuzz_targets/parser.rs` and the bounded job in `.github/workflows/ci.yml`.
  _Requirements: R3.5_

- [x] 10. Establish the drop-in surface.
  #206 supplied SONAME libpinyin.so.15 and the then consumer-union scope, 58/58.
  #192 supplied libpinyin.pc and installed naming.
  Headers use libpinyin-<pin version>: 2.11.91 then, 2.11.92 from #592 under the version ruling 2026-09-26 UTC.
  _Requirements: R7_

- [x] 11. Open and measure native data on distribution backends.
  This used #228's reader, later replaced by P6, and MemoryChunk reading.
  Fedora rawhide/Kyoto Cabinet, Debian testing/tkrzw and NixOS/Kyoto Cabinet produced 1,571/1,571 rows with byte-identical sets.
  Differences were order-only and attributed to R1's defined-order rule.
  _Requirements: R8, R9_

- [x] 12. Record output compatibility policy and the E2E I/O rule in `docs/findings/compatibility-policy.md`.
  Four classes existed when this task closed.
  Consumer-scope class (d) was retired 2026-09-06.
  Class (a)–(c) remain live.
  The paired pinned-behaviour and improvement summaries replaced the compact summary on 2026-10-14.
  They now reside in `docs/pinned-behaviour.md` and `docs/oxpinyin-improvements.md`.
  _Requirements: R9_
