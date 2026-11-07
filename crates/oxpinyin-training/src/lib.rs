//! Private, nonshipping decode behind `oxpinyin-utils`' `eval_correction_rate`.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

pub mod eval;
#[cfg(test)]
#[path = "../../oxpinyin-core/tests/common/fixture_doubles.rs"]
mod fixture_doubles;
