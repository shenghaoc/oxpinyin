# Implementation Plan — Python binding

## Overview

Shipped and documented in `docs/python.md`. All items closed 2026-09-08.

## Tasks

- [x] 1. PyO3 binding over the engine session API: `Engine`/`Candidate`
  pyclasses over the shared `oxpinyin-runtime` assembly;
  `Engine.from_fixture_dir(...).lookup(...)`.
  _Requirements: 1_

- [x] 2. Free-threaded CPython support: `pyo3 0.29` with `abi3-py310` +
  `abi3t-py315` declared; the GIL released around engine work while the
  session lock is held. (Amended 2026-09-08: CI's interpreters are
  GIL-enabled 3.14 — the container's stock python3 and
  `actions/setup-python` — so the tested build is the stable-ABI
  `cp310-abi3` one; free-threaded is written-for, not tested-for, per
  `docs/python.md`, "Supported platforms".)
  _Requirements: 2_

- [x] 3. The user contract in `docs/python.md`: data requirements,
  selection/learning workflows, thread-safety, error mapping.
  _Requirements: 3_

- [x] 4. Resolve the interpreter-floor metadata gap. Closed 2026-09-08:
  the crate README's install section reads "CPython 3.14", matching
  `requires-python = ">=3.14"` and `docs/python.md`; the "3.15 or newer"
  wording is gone.
  _Requirements: 2_

- [x] 5. macOS and Windows wheel builds. Closed 2026-09-08: CI's
  `python-portable` matrix builds the wheel on both (maturin, redb
  backend, GIL-enabled 3.14), asserts the `cp310-abi3` tag, and runs the
  Rust runtime tests and the native-vs-Python parity suite there.
  _Requirements: 2_

- [x] 6. GIL builds. Decided 2026-09-08: GIL-enabled CPython 3.14 IS the
  tested configuration on all three CI platforms (the stable-ABI wheel);
  the free-threaded build is the design target and stays a documented
  written-for-not-tested-for exclusion (`docs/python.md`) until a
  free-threaded interpreter can be provisioned in CI.
  _Requirements: 2_
