# Requirements Document — Stage 3: opt-in departures from the pin

## Introduction

Stage 3 (maintainer ruling 2026-10-14 UTC) permits intentional departures in observable behaviour, files or ABI.
Examples include a new language model, different candidate behaviour or an incompatible data format.
It begins after [Stage 2 task 4](../bug-for-bug/tasks.md), the compatibility exit gate.
These are [oxpinyin improvements](../../oxpinyin-improvements.md).
The default build retains [pinned behaviour](../../pinned-behaviour.md).
Compatible performance work remains Stage 2 under Requirement 4.
Recorded language artefacts also remain Stage 2 under Requirement 6.

## Requirements

### Requirement 1: The default build stays bug-for-bug

Distributors receive departures only when they opt in at build time.

#### Acceptance Criteria

1. Put each departure behind an opt-in cargo feature, off by default.
2. Retain every default-build differential's Stage 2 disposition under its policy.
   This includes `MATCH`, `POLICY-ACCEPTED` for a recorded artefact, and existing `HOST-DEPENDENT` cases.
   Keep default-build files unchanged.
3. State whether each departure changes observable output, files, ABI or several of these.

### Requirement 2: Binary identity follows compatibility

An incompatible build must identify itself so consumers do not load it as libpinyin by mistake.

#### Acceptance Criteria

1. Do not claim libpinyin's binary identity for an ABI or data-format departure.
   A maintainer ruling can permit an exception.
   This identity includes SONAME `libpinyin.so.15`, `libpinyin.pc` and the `libpinyin-<version>` header directory.
2. State an output-only departure in build metadata.

### Requirement 3: Storage stays on libpinyin's backends

#### Acceptance Criteria

1. Retain libpinyin's three backends, with one per build.
   Do not add a backend through a departure.
