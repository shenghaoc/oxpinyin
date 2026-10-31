//! libpinyin's utilities under their own names, options, messages, inputs
//! and outputs (maintainer ruling 2026-10-28 UTC; stage 5 of
//! `docs/findings/external-io-boundary.md`). Each utility's edge calls are
//! the pin's, made through `oxpinyin-input`'s `tools` boundary; its own
//! logic is here.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

pub mod gen_pinyin_table;

pub mod estimate_k_mixture_model;

pub mod export_k_mixture_model;

pub mod gen_k_mixture_model;

pub mod gen_zhuyin_table;

pub mod import_k_mixture_model;

pub mod k_mixture_model_to_interpolation;

pub mod segment;

pub mod storage;

pub mod training;
pub mod merge_k_mixture_model;

pub mod phrase_index;

pub mod prune_k_mixture_model;

pub mod validate_k_mixture_model;

/// The pin's line tagger, shared with the KMM operations that read its text
/// models. Its edge calls are `oxpinyin-input`'s.
pub use oxpinyin_kmm::taglib;
