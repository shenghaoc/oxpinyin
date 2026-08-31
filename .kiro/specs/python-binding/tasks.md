# Implementation Plan — Python binding

## Overview

Shipped and documented in `docs/python.md`. Open items below.

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

- [ ] 4. Resolve the interpreter-floor metadata gap — the crate README's
  install section says "free-threaded CPython 3.15 or newer" while
  `pyproject.toml` (`requires-python = ">=3.14"`), CI (`python-version:
  '3.14t'`) and `docs/python.md` ("Free-threaded CPython 3.14 ... the
  platform this binding is written for") all name 3.14t; align the
  README's floor or document the mismatch explicitly.
  _Requirements: 2_

- [ ] 5. macOS and Windows wheel builds are currently untested
  (`docs/python.md`, "Supported platforms": CI exercises Linux only; the
  portable crates are covered, the wheels are not) — test them or scope
  the claim to Linux explicitly.
  _Requirements: 2_

- [ ] 6. GIL builds are neither claimed nor tested
  (`docs/python.md`): decide whether GIL-build support stays a
  documented exclusion or gains a test lane.
  _Requirements: 2_
