//! libpinyin's utilities under their own names, options, messages, inputs
//! and outputs. The utility logic lives here. `oxpinyin-tool-support` provides
//! native I/O and adapters and uses `oxpinyin::__internal` for engine access.

#![deny(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]

#[forbid(unsafe_code)]
pub mod gen_pinyin_table;

#[forbid(unsafe_code)]
pub mod estimate_k_mixture_model;

#[forbid(unsafe_code)]
pub mod export_k_mixture_model;

#[forbid(unsafe_code)]
pub mod gen_k_mixture_model;

#[forbid(unsafe_code)]
pub mod gen_zhuyin_table;

#[forbid(unsafe_code)]
pub mod import_k_mixture_model;

#[forbid(unsafe_code)]
mod k_mixture_model;

#[forbid(unsafe_code)]
pub mod k_mixture_model_to_interpolation;

pub mod segment;

pub mod storage;

#[forbid(unsafe_code)]
pub mod merge_k_mixture_model;
pub mod training;

#[forbid(unsafe_code)]
mod phrase_index;

#[forbid(unsafe_code)]
pub mod prune_k_mixture_model;

#[forbid(unsafe_code)]
pub mod validate_k_mixture_model;

#[cfg(test)]
#[path = "../../src/user/test_child.rs"]
#[forbid(unsafe_code)]
mod test_child;
#[forbid(unsafe_code)]
mod utils_helper;
