# Implementation Plan — Stage 3: performance

## Overview

Not started as a stage: nothing below begins until Stage 2's tasks are
done (`.kiro/specs/bug-for-bug/tasks.md`). The checked items landed
early, before the restaging.

## Tasks

- [x] 1. Measurement harness: Criterion groups on the C-ABI surface, the
  `profiling` cargo profile, `tools/profile/run-w8-cycle.sh`
  (2026-08-19).
  _Requirements: 3_

- [x] 2. P1–P6 data-layer inversion: the runtime handles each backend's
  files natively (2026-09-02).
  _Requirements: 2_

- [x] 3. Release profile: fat LTO, one codegen unit
  (`docs/perf/perf-so-size-2026-09.md`).
  _Requirements: 2_

- [x] 4. Per-instance key-cost table eliminated (`6886dc1f`, 2026-09-07).
  _Requirements: 2_

- [ ] 5. Compile the text model at data-prep time.
  _Requirements: 1, 2_

- [ ] 6. Move the system bigram out of the KV store into a compiled blob.
  _Requirements: 1, 2_

- [ ] 7. Dictionary cursor walk for lattice building.
  _Requirements: 1, 2_

- [ ] 8. Incremental per-keystroke lattice and per-session arena.
  _Requirements: 1, 2_

- [ ] 9. Ordered atomics off the steady cycle.
  _Requirements: 1, 2_

- [ ] 10. Host-independent cost derivation.
  _Requirements: 1, 2_
