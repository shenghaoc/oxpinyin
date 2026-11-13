//! Training-only correction-rate evaluator: the `eval_correction_rate`
//! counterpart.
//!
//! Reproduces `utils/training/eval_correction_rate.cpp`
//! (`docs/findings/training-algorithm.md` §9): build the native evaluation
//! model, decode the evaluation corpus against the phrase index, and report
//! `correction rate = passed / tested`.
//!
//! The former `evaluate.py` driver — λ estimation, applying it to the
//! `interpolation2.text` model, and the end-to-end run — was removed with the
//! Python ports. λ estimation itself is the pin's `estimate_interpolation`,
//! now `oxpinyin_utils::training::estimate_interpolation`, and this module
//! is the decode behind `oxpinyin-utils`' `eval_correction_rate`.
//!
//! Never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// Inline #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

#[cfg(test)]
mod counts;
mod decode;
mod error;
mod model;

#[cfg(test)]
#[path = "../../../oxpinyin-core/tests/common/fixture_doubles.rs"]
mod fixture_doubles;
#[cfg(test)]
mod tests;

pub(super) use decode::{PhraseSource, correction_rate};
pub(super) use model::EvalLanguageModel;
