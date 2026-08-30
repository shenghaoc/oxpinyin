# Requirements Document — Python binding

## Introduction

`oxpinyin-python` serves the engine from Python with no libpinyin install —
the use case of [libpinyin issue #181](https://github.com/libpinyin/libpinyin/issues/181).
The binding rides the same `oxpinyin-runtime` assembly the C ABI uses, so
the two cannot silently diverge.

## Glossary

- **Free-threaded CPython** — CPython built without the GIL (PEP 703);
  PyO3's `abi3t` targets.
- **Session API** — `oxpinyin-engine`'s Rust session surface.

## Requirements

### Requirement 1: Bind the session API, not the C ABI

**User Story:** As a Python user, I want a native engine binding so that
nothing about a libpinyin install is required.

#### Acceptance Criteria

1. THE binding SHALL consume `oxpinyin-runtime`'s assembly and
   `oxpinyin-engine`'s session API directly.
2. THE binding SHALL NOT cross the C ABI: no `extern "C"` hop, no dlopen
   of the cdylib.

### Requirement 2: Free-threaded CPython support

**User Story:** As a free-threaded CPython user, I want a binding that is
safe without the GIL.

#### Acceptance Criteria

1. THE binding SHALL build free-threaded wheels (`abi3-py310` +
   `abi3t-py315`).
2. THE binding SHALL release the GIL around engine work while holding the
   session lock.
3. THE binding SHALL NOT claim GIL-build support it does not test
   (`docs/python.md`: "GIL builds are neither claimed nor tested here").

### Requirement 3: Same data contract as the engine

**User Story:** As a Python user, I want the binding to take the same data
the native engine takes.

#### Acceptance Criteria

1. THE binding SHALL follow the data contract in `docs/python.md` (data
   requirements, selection/learning workflows, thread-safety, error
   mapping).
