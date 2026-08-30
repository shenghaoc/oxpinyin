# Design — Python binding

## Overview

The rlib route: PyO3 types wrap `oxpinyin-runtime` handles directly. There
is no `extern "C"` crossing and no dlopen of the cdylib — the binding and
the C ABI are two consumers of one assembly, so they cannot silently
diverge.

## Components and Interfaces

- `crates/oxpinyin-python/src/binding.rs` — `#[pyclass(frozen)] Engine`
  and `Candidate`; `from_fixture_dir` constructs through the runtime;
  `lookup` snapshots candidates.
- GIL handling — engine work runs with the session locked and the GIL
  released (`docs/python.md` documents the thread-safety contract).
- Dependency — `pyo3 0.29` behind the optional `bindings` feature
  (`macros`, `abi3-py310`, `abi3t-py315`); maturin builds the wheel.
- User contract — `docs/python.md`: data requirements, selection/learning
  workflows, thread-safety, error mapping.
