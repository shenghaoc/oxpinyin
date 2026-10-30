# Implementation Plan — Stage 3: opt-in departures from the pin

## Overview

Not started.
Begin only after [Stage 2 task 4](../bug-for-bug/tasks.md), following tasks 1 and 2.
Stage 2 task 3, internal improvement, need not be complete.
Each task below adds one opt-in cargo feature, off by default.

## Tasks

- [ ] 1. Compile the text model during data preparation.
  _Requirements: 1, 2, 3_

- [ ] 2. Move the system bigram from the backend's `bigram.db` into a compiled blob.
  _Requirements: 1, 2, 3_

- [ ] 3. Add model upgrades: trigram or Kneser-Ney, typo edges and own data.
  _Requirements: 1, 2_
