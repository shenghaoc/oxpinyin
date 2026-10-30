# Implementation Plan — Stage 2: bug-for-bug compatibility and internal improvement

## Overview

GitHub issues, labels, milestones and sub-issues track Stage 2 work.
This file records the sequence, not a duplicate issue list.

## Tasks

- [x] 1. Close the [round-2 audit phase](../../findings/bug-for-bug-audit-r2-2026-09-23.md).
  Tracker #573 closed on 2026-10-17.
  Execution was partial; phase closure does not mean every finding is fixed.
  The guess-path axis L findings, #757 and its siblings, are fixed.
  The tkrzw and kc axis L items #553/#555 are measured.
  Section 18 items, "Carried to round 3", remain open for task 4.
  _Requirements: 1, 2, 3, 4_

- [ ] 2. Close init complexity tracker #870.
  It covers axis L init instructions and peak heap against the pin.
  _Requirements: 4_

- [ ] 3. Land compatible internal improvements individually when ready.
  See the design's Internal improvement section.
  Keep differentials identical, or `POLICY-ACCEPTED` for a recorded language artefact.
  This task does not block task 4.
  _Requirements: 3, 4, 5_

- [ ] 4. Run the third bug-for-bug sweep after tasks 1 and 2.
  Run every differential on all three cells.
  Check every register row against the pin's source.
  Include task 1's items carried to round 3.
  Record language artefacts instead of reproducing them.
  This is the Stage 2 compatibility exit gate.
  _Requirements: 1, 3, 6_

- [ ] 5. Remove reproduced language artefacts under the 2026-10-26 ruling, #1303.
  Give each its record and `POLICY-ACCEPTED` cases.
  The classification is #1297 (#1306).
  Examples are the 8-bit span key (#1275), tail-comparator truncation, libstdc++ heap tie order, glibc `%f` extensions and amd64 out-of-range conversions.
  This task does not block task 4.
  _Requirements: 6_
