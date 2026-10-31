//! Training-only correction-rate evaluator: the `eval_correction_rate`
//! counterpart.
//!
//! Reproduces `utils/training/eval_correction_rate.cpp`
//! (`docs/findings/trainer-parity-audit.md` §7): build the native evaluation
//! model, decode the evaluation corpus against the phrase index, and report
//! `correction rate = passed / tested`.
//!
//! The former `evaluate.py` driver — λ estimation, applying it to the
//! `interpolation2.text` model, and the end-to-end run — was removed with the
//! Python ports. λ estimation itself is the pin's `estimate_interpolation`,
//! now `oxpinyin_utils::training::estimate_interpolation`, and this module
//! is the decode behind `oxpinyin-utils`' `eval_correction_rate`.
//!
//! The model cost mirrors the shipping decoder's LM
//! (`oxpinyin_data::lm`), so the decode ranks exactly as the real engine
//! would; the real-model path drives the same generic core over an
//! [`oxpinyin_data::SystemDictionary`]. Never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see .kiro/steering/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// Inline #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

mod decode;
mod error;
mod model;
mod phrases;

pub use decode::{
    EvalReport, NULL_TOKEN, PhraseSource, SENTENCE_START, correction_rate, parse_eval_corpus,
};
pub use error::EvalError;
pub use model::EvalLanguageModel;
pub use phrases::SystemPhraseSource;

use crate::counter::Counts;
use oxpinyin_core::PhraseToken;
use oxpinyin_data::Lambda;

/// Parses a candidate `interpolation2.text` into its unigram/bigram counts
/// (`crate::counter::parse_interpolation_dump`), the input to the eval
/// model.
#[must_use]
pub fn parse_interpolation2(text: &str) -> Counts {
    crate::counter::parse_interpolation_dump(text)
}

/// Builds the native evaluation model from the candidate counts with λ
/// applied, floored over the phrase lexicon (`lexicon`).
///
/// The way `evaluate.py`'s `make` rebuilt the runtime model
/// (`gen_binary_files` + `import_interpolation` + `gen_unigram`).
#[must_use]
pub fn build_model(
    counts: &Counts,
    lambda: Lambda,
    lexicon: impl IntoIterator<Item = PhraseToken>,
) -> EvalLanguageModel {
    EvalLanguageModel::from_counts_with_lexicon(counts, lambda, lexicon)
}
