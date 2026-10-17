# Implementation Plan — Stage 2: bug-for-bug compatibility

## Overview

Stage 2 work is tracked as GitHub issues (labels, milestones and
sub-issues); this file records the stage's sequence, not a copy of the
issue list.

## Tasks

- [ ] 1. Close the round-2 audit issues
  (`docs/findings/bug-for-bug-audit-r2-2026-09-23.md`), each with its
  derivation cited — axis L included: the complexity findings (#757 and
  its siblings) fixed, and #553/#555 (axis L on tkrzw and kc) measured.
  _Requirements: 1, 2, 3, 4_

- [ ] 2. Run the third bug-for-bug sweep after task 1: every
  differential on all three cells, every register row re-checked against
  the pin's source.
  _Requirements: 1, 3_
