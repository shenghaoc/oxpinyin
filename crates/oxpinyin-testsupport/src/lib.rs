//! The model20 cache locator shared by the benches and the oracle harness.
//!
//! This crate is a workspace member so `cargo test --workspace` compiles and
//! runs its tests. No shipping crate depends on it.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod model_cache;
