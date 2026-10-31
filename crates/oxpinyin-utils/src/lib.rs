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

pub mod gen_zhuyin_table;

pub mod k_mixture_model_to_interpolation;

pub mod segment;

pub mod storage;

pub mod taglib;
