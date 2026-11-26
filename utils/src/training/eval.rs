//! Training-only correction-rate evaluator: the `eval_correction_rate`
//! counterpart.
//!
//! Reproduces `utils/training/eval_correction_rate.cpp`
//! (`docs/findings/training-algorithm.md` §9): build the native evaluation
//! model, decode the evaluation corpus against the phrase index, and report
//! `correction rate = passed / tested`.
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

mod decode;
mod model;

#[cfg(test)]
#[path = "../../../tests/common/fixture_doubles.rs"]
mod fixture_doubles;
#[cfg(test)]
mod tests;

pub(super) use decode::{LibraryPhraseSource, correction_rate};
pub(super) use model::EvalLanguageModel;
