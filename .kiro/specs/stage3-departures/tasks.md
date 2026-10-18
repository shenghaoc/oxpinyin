# Implementation Plan — Stage 3: opt-in departures from the pin

## Overview

Not started. Nothing below begins until the Stage 2 compatibility exit
gate is complete: task 4 in `.kiro/specs/bug-for-bug/tasks.md`, after
tasks 1 and 2. Stage 2 task 3 (internal improvement) does not need to be
complete. Each task below adds one opt-in cargo feature, off by default.

## Tasks

- [ ] 1. Compile the text model at data-prep time.
  _Requirements: 1, 2, 3_

- [ ] 2. Move the system bigram out of the backend's `bigram.db` into a
  compiled blob.
  _Requirements: 1, 2, 3_

- [ ] 3. Model upgrades: trigram or Kneser-Ney, typo edges, own data.
  _Requirements: 1, 2_
