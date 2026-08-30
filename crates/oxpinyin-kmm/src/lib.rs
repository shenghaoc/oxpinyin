//! Training-only K-mixture-model pipeline.
//!
//! Native Rust reproduction of libpinyin's KMM tools — the load-bearing
//! path of the trainer's main pipeline (segment → **generate → estimate →
//! merge → validate → prune → export → KMM→interpolation** → evaluate). See
//! `docs/findings/trainer-parity-audit.md` §6 and
//! `utils/training/*k_mixture_model*.cpp`.
//!
//! The model ([`KMixtureModel`]) is ordered maps keyed by token, so every
//! operation is deterministic and token-ascending. It carries the phrase
//! text column alongside the counts, so export needs no phrase index and
//! the crate has no dependencies. Never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod estimate;
mod generate;
mod merge;
mod model;
mod prune;
mod text;
mod validate;

pub use error::KmmError;
pub use estimate::{EPSILON, Estimate, SEED_LAMBDA, estimate};
pub use generate::{DEFAULT_MAX_INCREASE_RATE, DEFAULT_MAX_OCCURS, GenerateParams};
pub use merge::merge_into;
pub use model::{
    ArrayItem, KMixtureModel, NULL_TOKEN, Parameter, SENTENCE_START, SENTENCE_START_TEXT,
    SingleGram, compute_alpha, compute_b, compute_gamma, compute_pr_g_3, compute_pr_g_3_with_count,
};
pub use prune::{DEFAULT_CDF, DEFAULT_PRUNE_K, prune};
pub use text::{canonicalize, export, import, kmm_text_to_interpolation};
pub use validate::validate;
