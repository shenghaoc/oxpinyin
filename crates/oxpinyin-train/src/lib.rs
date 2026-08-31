//! Native trainer orchestrator — the whole `libpinyin/trainer` main workflow
//! in Rust, no Python, `make`, SQLite, or libpinyin binaries.
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
//! index ([`CandidateIndex`]) — and wires the existing native tool crates
//! ([`oxpinyin_segment`], [`oxpinyin_kmm`], [`oxpinyin_lambda`],
//! [`oxpinyin_eval`]) into the full run. [`pipeline`] holds the stages as pure
//! functions; [`workspace`] adds the on-disk `try<name>` layout, status
//! files, intermediate cleanup, and resumability; [`Trainer`] is the one
//! entry point that runs the whole thing. Never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

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
pub use oxpinyin_eval::PhraseSource;
pub use pipeline::{
    CandidateModel, EvalInputs, EvalOutcome, FinalModel, ScoredCandidate, SegmentMethod,
    SegmentedDoc, SortedCandidates, TrainOutcome, evaluate_model, gather_and_sort,
    generate_candidates, merge_prune_convert, run_pipeline, score_candidates, segment_documents,
};
pub use status::{EpochState, Stage, Status};
pub use workspace::{Trainer, TrainerPaths};
