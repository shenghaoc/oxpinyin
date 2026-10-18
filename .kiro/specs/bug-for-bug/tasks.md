# Implementation Plan — Stage 2: bug-for-bug compatibility and internal improvement

## Overview

Stage 2 work is tracked as GitHub issues (labels, milestones and
sub-issues); this file records the stage's sequence, not a copy of the
issue list.

## Tasks

- [x] 1. Close the round-2 audit phase
  (`docs/findings/bug-for-bug-audit-r2-2026-09-23.md`). The audit tracker
  #573 closed on 2026-10-17. Its execution was partial: closure of the
  phase does not mean that every finding is fixed. The guess-path axis L
  findings (#757 and its siblings) are fixed, and #553/#555 (axis L on
  tkrzw and kc) are measured. The items in the report's section 18,
  "Carried to round 3", stay open and go to task 4.
  _Requirements: 1, 2, 3, 4_

- [ ] 2. Close the init complexity tracker #870 (axis L: init
  instructions and peak heap against the pin).
  _Requirements: 4_

- [ ] 3. Internal improvement that keeps compatibility (design,
  "Internal improvement"): each item lands when it is ready, with its
  differentials identical. It does not block task 4.
  _Requirements: 3, 4, 5_

- [ ] 4. Run the third bug-for-bug sweep after tasks 1 and 2: every
  differential on all three cells, every register row re-checked against
  the pin's source, and the items carried to round 3 from task 1. This
  task is the Stage 2 compatibility exit gate.
  _Requirements: 1, 3_
