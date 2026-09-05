//! Black-box integration tests over the C ABI.
//!
//! Driven exclusively through the re-exported `pinyin_*` surface of the
//! rlib — the same symbols the shipped `.so` exports — against the
//! committed `fixtures/w3` mini tables. Suites that assert on
//! `CapiInstance` internals are deliberately *not* here: they are unit
//! tests of the ABI layer and live in `src/` (see `src/lib.rs`).
// Placed below the `//!` block, never above it: a crate-level `#![cfg]`
// that evaluates false discards the crate attributes that FOLLOW it, so a
// gate on line 1 takes these docs with it and `missing_docs` then fires on
// every non-Linux host. Same placement as the other cfg-gated test crates.
#![cfg(target_os = "linux")]

#[path = "abi/common.rs"]
mod common;
#[path = "abi/contract.rs"]
mod contract;
#[path = "abi/exact_scheme.rs"]
mod exact_scheme;
#[path = "abi/keys.rs"]
mod keys;
#[path = "abi/phrase.rs"]
mod phrase;
#[path = "abi/pipeline.rs"]
mod pipeline;
