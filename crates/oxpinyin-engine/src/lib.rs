//! Internal session state and decoding operations used by the C facades.
//!
//! Configuration and backend handles arrive as data. The engine has no
//! frontend key dispatch, display model or platform path discovery.

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
mod char_offset;
mod config;
mod constraint;
mod cursor;
mod error;
mod nbest;
mod phrase;
mod session;
mod storage;

pub use candidate::{Candidate, CandidateKind, CandidateList, rank_by_frequency};
pub use char_offset::character_offset_over_keys;
pub use config::{Config, ConfigSource, ConfigValue, EmptyConfigSource, UPSTREAM_DEFAULT_COUNT};
pub use cursor::{
    MatrixKey, left_word_offset, left_word_offset_over_spans, lookup_offset_for_cursor,
    lookup_offset_over_spans, right_word_offset, right_word_offset_over_spans,
};
pub use error::{EngineError, TrainingSpanFault};
pub use nbest::NbestShape;
pub use session::{
    CandidateRanking, KeyOutcome, MAX_INPUT_BYTES, Selection, Session, amplified_frequency,
    check_lookup_offset_range, normalize_lookup_offset,
};
pub use storage::StoragePaths;
