//! Training-only correction-rate evaluator.
//!
//! Native Rust reproduction of the trainer's `evaluate.py` +
//! `eval_correction_rate` (`docs/findings/trainer-parity-audit.md` §7),
//! with **no Python, libpinyin, `make`, or external evaluator binary**:
//!
//! ```text
//! interpolation2.text
//!     ↓  parse to counts, build the native EvalLanguageModel
//! estimate λ  (oxpinyin-lambda deleted-interpolation EM, over a held-out slice)
//!     ↓  apply λ (the exact table.conf rational, {:.6} round-trip)
//! decode the evaluation corpus  (reuse the engine's sentence Viterbi / Scorer)
//!     ↓
//! correction rate = passed / tested
//! ```
//!
//! The model cost mirrors the shipping decoder's LM
//! (`oxpinyin_data::lm`), so the decode ranks exactly as the real engine
//! would; the real-model path drives the same generic core over an
//! [`oxpinyin_data::SystemDictionary`]. Never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

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

use oxpinyin_core::PhraseToken;
use oxpinyin_counter::Counts;
use oxpinyin_data::Lambda;
use oxpinyin_lambda::DeletedCounts;

/// Parses a candidate `interpolation2.text` into its unigram/bigram counts
/// (`oxpinyin_counter::parse_interpolation_dump`), the input to the eval
/// model and the λ estimation.
#[must_use]
pub fn parse_interpolation2(text: &str) -> Counts {
    oxpinyin_counter::parse_interpolation_dump(text)
}

/// Estimates λ over the candidate's counts against a held-out deleted model
/// and returns it as the exact `table.conf` rational, matching `evaluate.py`
/// estimating with `estimate_interpolation` and writing `{:.6}` into
/// `table.conf`.
///
/// # Errors
///
/// Returns [`EvalError::Lambda`] when no held-out context is scorable or the
/// estimate falls outside `[0, 1]`.
pub fn estimate_lambda(system: &Counts, deleted: &DeletedCounts) -> Result<Lambda, EvalError> {
    let lambda =
        oxpinyin_lambda::estimate_lambda(system, deleted).map_err(|error| EvalError::Lambda {
            detail: error.to_string(),
        })?;
    lambda_from_f64(lambda.average)
}

/// Applies an estimated λ (an `f64` weight) as the exact `table.conf`
/// rational, via the `{:.6}` round-trip `make modify LAMBDA_PARAMETER=λ`
/// performs.
///
/// # Errors
///
/// Returns [`EvalError::Lambda`] when λ is not finite or is outside
/// `[0, 1]` — checked on the original value, before the six-decimal
/// rounding, so `1.0000004` is rejected rather than rounded to `1.000000`.
pub fn lambda_from_f64(value: f64) -> Result<Lambda, EvalError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(EvalError::Lambda {
            detail: format!("lambda {value} is not in [0, 1]"),
        });
    }
    Lambda::from_decimal(&format!("{value:.6}")).ok_or_else(|| EvalError::Lambda {
        detail: format!("lambda {value:.6} is not in [0, 1]"),
    })
}

/// Builds the native evaluation model from the candidate counts with λ
/// applied, floored over the phrase lexicon (`lexicon`) the way
/// `evaluate.py`'s `make` rebuilds the runtime model (`gen_binary_files` +
/// `import_interpolation` + `gen_unigram`).
#[must_use]
pub fn build_model(
    counts: &Counts,
    lambda: Lambda,
    lexicon: impl IntoIterator<Item = PhraseToken>,
) -> EvalLanguageModel {
    EvalLanguageModel::from_counts_with_lexicon(counts, lambda, lexicon)
}

#[cfg(test)]
mod tests {
    use super::lambda_from_f64;

    #[test]
    fn lambda_is_range_checked_before_rounding() {
        assert_eq!(
            lambda_from_f64(0.5).expect("in range"),
            oxpinyin_data::Lambda::from_decimal("0.500000").expect("half")
        );
        assert!(lambda_from_f64(1.0).is_ok());
        assert!(lambda_from_f64(0.0).is_ok());
        for out in [1.000_000_4, -0.000_000_4, 1.5, f64::NAN, f64::INFINITY] {
            assert!(lambda_from_f64(out).is_err(), "{out}");
        }
    }
}
