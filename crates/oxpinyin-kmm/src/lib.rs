//! The k-mixture model's operations on the pin's flexible-bigram container
//! (`utils/training/*_k_mixture_model.cpp` at libpinyin
//! `074a2219c90feaf962d0d24f034514033ece5f99`).
//!
//! The pin keeps its working model in a DBM whose records are the raw
//! host-native structs of `utils/training/k_mixture_model.h`. Member 4's
//! [`oxpinyin_store::FlexibleBigram`] carries that container class and byte
//! layout; [`records`] names the three record types and the `KMMP` magic
//! number as [`FixedRecord`] and [`FlexibleGramSpec`] impls.
//!
//! Each operation here is the pin's C++ control flow: the same per-token
//! walks (`get_all_items` in the backend's native order, `retrieve_all`
//! token-ascending), the same ignored return values, the same `fprintf`
//! bytes through `oxpinyin-input`'s tool boundary, the same ordinary exits
//! and the same source-derived aborts. The utility `main`s live in
//! `oxpinyin-utils` under the pin's binary names; they parse the pin's
//! options and load its system tables, then call these operations.
//!
//! The crate never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see .kiro/steering/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// Inline #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

pub mod database;
pub mod estimate;
pub mod export;
pub mod generate;
pub mod import;
pub mod math;
pub mod merge;
pub mod phrase;
pub mod prune;
pub mod records;
pub mod taglib;
pub mod validate;

pub use database::{Database, open_path};
pub use math::{
    compute_alpha, compute_b, compute_gamma, compute_pr_g_3, compute_pr_g_3_with_count,
};
pub use records::{
    ArrayItemWithToken, FixedRecord, FlexibleGramSpec, KMixtureModelArrayHeader,
    KMixtureModelArrayItem, KMixtureModelMagicHeader, KmmBigram, KmmGram, KmmSpec, SingleGram,
};
