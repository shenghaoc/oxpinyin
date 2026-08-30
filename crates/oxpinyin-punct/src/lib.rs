//! Training-only punctuation-table generator.
//!
//! Native Rust reproduction of the trainer's `genpunct.py`: it reads
//! word→punctuation pairs from the segmented corpus, prunes per index and
//! then globally, and emits the `puncts.table` the engine's punctuation
//! candidates consume. See `docs/findings/trainer-parity-audit.md` §9.
//!
//! Self-contained (the segmented stream carries the word text and the
//! punctuation), deterministic (ordered maps + a canonical output order),
//! and never ships with the engine.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod counts;
mod error;

pub use counts::{ALL_INDEX_THRESHOLD, PER_INDEX_THRESHOLD, PUNCT_SEARCH, PunctCounts};
pub use error::PunctError;
