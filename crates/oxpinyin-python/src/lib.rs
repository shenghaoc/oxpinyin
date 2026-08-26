//! Python bindings for oxpinyin.
//!
//! The supported surface for embedders is [`runtime`]: a concrete engine
//! assembly over `oxpinyin-engine`'s public session API. With the
//! `bindings` feature the same runtime is re-exported through PyO3 as the
//! `oxpinyin._native` extension module; see `docs/python.md`.
//!
//! [`dump`] carries the corpus driver shared with the Python-side parity
//! tests; the `native-dump` binary renders transcripts from it.
#![warn(missing_docs)]

pub mod dump;
pub mod runtime;

#[cfg(feature = "bindings")]
mod binding;
