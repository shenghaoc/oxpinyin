//! The k-mixture model's operations on the pin's flexible-bigram container
//! (`utils/training/*_k_mixture_model.cpp` at libpinyin
//! `074a2219c90feaf962d0d24f034514033ece5f99`).
//!
//! The pin keeps its working model in a DBM whose records are the raw
//! host-native structs of `utils/training/k_mixture_model.h`. Member 4's
//! [`oxpinyin_store::FlexibleBigram`] carries that container class and byte
//! layout; [`records`] names the three record types and the `KMMP` magic
//! number as [`oxpinyin_store::FixedRecord`] and [`oxpinyin_store::FlexibleGramSpec`] impls.
//!
//! Each operation here is the pin's C++ control flow: the same per-token
//! walks (`get_all_items` in the backend's native order, `retrieve_all`
//! token-ascending), the same ignored return values, the same `fprintf`
//! bytes through `oxpinyin-input`'s tool boundary, the same ordinary exits
//! and the same source-derived aborts. The utility `main`s live in
//! the surrounding crate under the pin's binary names; they parse the pin's
//! options and load its system tables, then call these operations.
//!
//! These operations only build with the nonshipping utilities.
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
pub mod validate;

#[cfg(test)]
mod flexible_records;
