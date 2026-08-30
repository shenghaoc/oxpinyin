# Implementation Plan — Python binding

## Overview

Shipped and documented in `docs/python.md`; no pending work is tracked
yet.

## Tasks

- [x] 1. PyO3 binding over the engine session API: `Engine`/`Candidate`
  pyclasses over the shared `oxpinyin-runtime` assembly;
  `Engine.from_fixture_dir(...).lookup(...)`.
  _Requirements: 1_

- [x] 2. Free-threaded CPython wheels: `pyo3 0.29` with `abi3-py310` +
  `abi3t-py315`; the GIL released around engine work while the session
  lock is held.
  _Requirements: 2_

- [x] 3. The user contract in `docs/python.md`: data requirements,
  selection/learning workflows, thread-safety, error mapping.
  _Requirements: 3_
