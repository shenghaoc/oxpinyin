//! Native trainer orchestrator — the whole `libpinyin/trainer` main workflow
//! in Rust, no Python, `make`, `SQLite`, or libpinyin binaries.
//!
//! The trainer's main pipeline is a chain of Python drivers over the KMM
//! tools:
//!
//! ```text
//! segment.py   raw corpus → segmented          (ngseg / spseg)
//! generate.py  segmented  → candidate models    (gen_k_mixture_model, rollover)
//! estimate.py  candidates → scored + sorted     (estimate_k_mixture_model)
//! tryprune.py  top N       → merged → pruned →   interpolation2.text
//! evaluate.py  final model → λ → correction rate (estimate_interpolation +
//!                                                  eval_correction_rate)
//! ```
//!
//! This crate reproduces every driver capability with typed structures — the
//! corpus index ([`CorpusIndex`]), the status/epoch mechanism ([`Status`],
//! [`Stage`]), the config knobs ([`TrainConfig`]), and the candidate scoring
//! index ([`CandidateIndex`]) — and wires the existing native stage modules
//! and KMM package ([`crate::segment`], [`oxpinyin_kmm`], [`crate::lambda`],
//! [`crate::eval`]) into the full run. `pipeline` holds the stages as pure
//! functions; `workspace` adds the on-disk `try<name>` layout, status
//! files, intermediate cleanup, and resumability; [`Trainer`] is the one
//! entry point that runs the whole thing. Never ships with the engine.
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

mod candidate;
mod config;
mod corpus;
mod error;
mod pipeline;
mod status;
mod workspace;

pub use candidate::{Candidate, CandidateIndex};
pub use config::{
    ESTIMATE_INDEX, EVALS_TEXT_FILE_NAME, FINAL_MODEL_FILE_NAME, FINAL_STATUS_FILE_NAME,
    INDEX_POSTFIX, MODEL_POSTFIX, REPORT_POSTFIX, SEGMENT_POSTFIX, SORTED_ESTIMATE_INDEX,
    STATUS_POSTFIX, TrainConfig, candidate_model_name,
};
pub use corpus::{CorpusIndex, IndexEntry};
pub use error::TrainError;
// Re-exported so callers name the evaluate-stage phrase source (and the
// dictionary bound) without a separate dependency edge on oxpinyin-eval.
pub use crate::eval::PhraseSource;
pub use pipeline::{
    CandidateModel, EvalInputs, EvalOutcome, FinalModel, ScoredCandidate, SegmentMethod,
    SegmentedDoc, SortedCandidates, TrainOutcome, evaluate_model, gather_and_sort,
    generate_candidates, merge_prune_convert, run_pipeline, score_candidates, segment_documents,
};
pub use status::{EpochState, Stage, Status};
pub use workspace::{Trainer, TrainerPaths};

/// Validate one held-out byte stream and build both scoring and lambda models.
///
/// The pin's `gen_k_mixture_model.cpp:72-137` and
/// `gen_deleted_ngram.cpp:78-118` consume the same segmented-line macro.
/// Parse once so ignored bytes and exact phrase validation cannot differ
/// between the two consumers.
///
/// # Errors
///
/// Returns an error for a malformed or mismatched segmented record.
pub fn prepare_held_out(
    lexicon: &crate::segment::PhraseLexicon,
    bytes: &[u8],
    params: oxpinyin_kmm::GenerateParams,
) -> Result<(oxpinyin_kmm::KMixtureModel, crate::lambda::DeletedCounts), TrainError> {
    let tokens =
        crate::counter::parse_ngseg(lexicon, bytes).map_err(|error| TrainError::Malformed {
            detail: error.to_string(),
        })?;
    let mut scoring = oxpinyin_kmm::KMixtureModel::new();
    scoring
        .add_token_document(
            tokens.iter().map(|&token| {
                let phrase = lexicon.text(token).unwrap_or_default();
                (token, phrase.split('\0').next().unwrap_or_default())
            }),
            params,
        )
        .map_err(|error| TrainError::Kmm {
            stage: "held-out",
            detail: error.to_string(),
        })?;
    let deleted = crate::lambda::count_deleted_tokens(&tokens, params.train_pi_gram);
    Ok((scoring, deleted))
}
