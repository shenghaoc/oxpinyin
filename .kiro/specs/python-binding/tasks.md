# Implementation Plan — Python binding

## Overview

Shipped and documented in `docs/python.md`. No open items.

## Tasks

- [x] 1. PyO3 binding over the engine session API: `Engine`/`Candidate`
  pyclasses over the shared `oxpinyin-runtime` assembly;
  `Engine.from_fixture_dir(...).lookup(...)`.
  _Requirements: 1_

- [x] 2. Free-threaded CPython support: `pyo3 0.29` with `abi3-py310` +
  `abi3t-py315` declared (neither stable ABI selects on the tested 3.14t
  interpreter — pyo3 emits a version-specific `cp314t` build); CI
  validates the source build on free-threaded CPython 3.14t, Linux; the
  GIL released around engine work while the session lock is held.
  _Requirements: 2_

- [x] 3. The user contract in `docs/python.md`: data requirements,
  selection/learning workflows, thread-safety, error mapping.
  _Requirements: 3_

- [x] 4. Resolve the interpreter-floor metadata gap — the crate README's
  install section says "free-threaded CPython 3.15 or newer" while
  `pyproject.toml` (`requires-python = ">=3.14"`), CI (`python-version:
  '3.14t'`) and `docs/python.md` ("Free-threaded CPython 3.14 ... the
  platform this binding is written for") all name 3.14t; align the
  README's floor or document the mismatch explicitly.
  Done: #317 narrowed the README install line to what CI tests, and
  c7665368 (#325) set it to the tested interpreter, "CPython 3.14";
  resolved by CI containerization, #322.
  _Requirements: 2_

- [x] 5. macOS and Windows wheel builds are currently untested
  (`docs/python.md`, "Supported platforms": CI exercises Linux only; the
  portable crates are covered, the wheels are not) — test them or scope
  the claim to Linux explicitly.
  Done: scoped to Linux explicitly — `docs/python.md` "Supported
  platforms" states that CI exercises Linux, that the one tested
  configuration is the container's stock `python3`, and that macOS and
  Windows wheel builds are untested (c7665368, #325); resolved by CI
  containerization, #322.
  _Requirements: 2_

- [x] 6. GIL builds are neither claimed nor tested
  (`docs/python.md`): decide whether GIL-build support stays a
  documented exclusion or gains a test lane.
  Done: the CI lane now builds and tests a GIL-enabled CPython 3.14
  (the `debian:testing` image's `python3`) and asserts the `cp310-abi3`
  wheel tag (153b9fcd, #325); `docs/python.md` claims exactly that
  configuration; resolved by CI containerization, #322.
  _Requirements: 2_
