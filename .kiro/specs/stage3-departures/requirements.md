# Requirements Document — Stage 3: opt-in departures from the pin

## Introduction

Stage 3 (maintainer ruling 2026-10-14 UTC) permits intentional
departures from the pinned libpinyin: changes to observable behaviour,
to the files on disk or to the ABI. Examples are a new language model,
different candidate behaviour and an incompatible data format. It starts
after the Stage 2 compatibility exit gate
(`.kiro/specs/bug-for-bug/tasks.md`, task 4). It is an oxpinyin improvement
(`.kiro/steering/oxpinyin-improvements.md`). The default build keeps the
pinned behaviour (`.kiro/steering/pinned-behaviour.md`).

Performance work that keeps compatibility is not Stage 3. It is Stage 2
work (`.kiro/specs/bug-for-bug/`, Requirement 4). A difference that is
only a language artefact is not Stage 3 either: it is recorded in
Stage 2 (`.kiro/specs/bug-for-bug/`, Requirement 6).

## Requirements

### Requirement 1: The default build stays bug-for-bug

**User Story:** As a distributor, I want the default oxpinyin build to
stay the bug-for-bug libpinyin binary, so that a departure never reaches
me unless I opt in at build time.

#### Acceptance Criteria

1. EACH departure SHALL sit behind an opt-in cargo feature, off by
   default.
2. IN the default build, every differential SHALL keep its Stage 2
   disposition under its policy — `MATCH`, `POLICY-ACCEPTED` for a
   recorded language artefact, or an existing `HOST-DEPENDENT` case —
   and the files on disk SHALL not change.
3. EACH departure SHALL state what it changes: observable output, files
   on disk, the ABI, or more than one of these.

### Requirement 2: Binary identity follows compatibility

**User Story:** As a distributor, I want a build that is not
compatible to say so, so that no consumer loads it as libpinyin by
mistake.

#### Acceptance Criteria

1. A build with a departure that changes the ABI or the data formats
   SHALL NOT claim libpinyin's binary identity (SONAME
   `libpinyin.so.15`, `libpinyin.pc`, the `libpinyin-<version>` header
   directory) unless a maintainer ruling says otherwise.
2. A build with a departure that changes only observable output SHALL
   state the departure in its build metadata.

### Requirement 3: Storage stays on libpinyin's backends

#### Acceptance Criteria

1. THE storage backends SHALL stay the three libpinyin is built on, one
   per build. A departure SHALL NOT add a backend.
