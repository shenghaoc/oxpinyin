//! libpinyin's utilities under their own names, options, messages, inputs
//! and outputs. Each utility's edge calls are
//! the pin's, made through `oxpinyin`'s `tools` boundary; its own
//! logic is here.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]

pub mod gen_pinyin_table;

pub mod estimate_k_mixture_model;

pub mod export_k_mixture_model;

pub mod gen_k_mixture_model;

pub mod gen_zhuyin_table;

pub mod import_k_mixture_model;

mod k_mixture_model;

pub mod k_mixture_model_to_interpolation;

pub mod segment;

pub mod storage;

pub mod merge_k_mixture_model;
pub mod training;

mod phrase_index;

pub mod prune_k_mixture_model;

pub mod validate_k_mixture_model;

#[cfg(test)]
#[path = "../../src/user/test_child.rs"]
mod test_child;
mod utils_helper;
