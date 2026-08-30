//! Black-box integration tests over the C ABI.
//!
//! Driven exclusively through the re-exported `pinyin_*` surface of the
//! rlib — the same symbols the shipped `.so` exports — against the
//! committed `fixtures/w3` mini tables. Suites that assert on
//! `CapiInstance` internals are deliberately *not* here: they are unit
//! tests of the ABI layer and live in `src/` (see `src/lib.rs`).

#[path = "abi/common.rs"]
mod common;
#[path = "abi/contract.rs"]
mod contract;
#[path = "abi/exact_scheme.rs"]
mod exact_scheme;
#[path = "abi/keys.rs"]
mod keys;
#[path = "abi/pipeline.rs"]
mod pipeline;
