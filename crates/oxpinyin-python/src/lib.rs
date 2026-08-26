//! Python bindings for oxpinyin.
//!
//! The concrete engine assembly lives in `oxpinyin-runtime`, shared with
//! `oxpinyin-capi` so native consumers, the C ABI and this binding cannot
//! silently diverge. With the `bindings` feature this crate re-exposes that
//! assembly through PyO3 as the `oxpinyin._native` extension module; see
//! `docs/python.md`. [`dump`] carries the corpus driver shared with the
//! Python-side parity tests.
#![warn(missing_docs)]

pub mod dump;

#[cfg(feature = "bindings")]
mod binding;
