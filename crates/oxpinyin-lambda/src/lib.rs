//! Training-only deleted-interpolation λ estimator.
//!
//! Reproduces two libpinyin trainer stages, in order:
//!
//! 1. [`held_out`] — `gen_deleted_ngram`
//!    (`utils/training/gen_deleted_ngram.cpp`): held-out bigram counting
//!    into `DELETED_BIGRAM`, no unigram, no freq-1 floor.
//! 2. [`interpolation`] — `estimate_interpolation`
//!    (`utils/training/estimate_interpolation.cpp`): the deleted-
//!    interpolation EM (seed λ = 0.6, ε = 0.001, per-context update) and
//!    the cross-context arithmetic mean that yields the single system λ.
//!
//! It consumes W9-T2's integer [`oxpinyin_counter::Counts`] as the system
//! model (`SYSTEM_BIGRAM` + phrase-index unigram, floor included) and the
//! held-out counts it derives here, and produces λ — the value the decode
//! path currently hardcodes as an authored constant
//! (`oxpinyin-data/src/lm.rs` `LAMBDA_NUMERATOR/LAMBDA_DENOMINATOR = 1/2`).
//! This crate makes λ *derived*; it does **not** change the decode path.
//! See `docs/findings/lambda-port.md`.
//!
//! Float boundary: counts are integers throughout ([`held_out`],
//! [`oxpinyin_counter`]); floating point (`parameter_t = double`) appears
//! only inside the EM ([`interpolation`]). Never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod error;
mod held_out;
mod interpolation;

pub use error::LambdaError;
pub use held_out::{DeletedCounts, count_deleted, count_deleted_tokens};
pub use interpolation::{EPSILON, Lambda, SEED_LAMBDA, estimate_lambda};
