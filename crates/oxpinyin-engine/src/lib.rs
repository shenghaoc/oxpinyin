//! Internal session state and decoding operations used by the C facades.
//!
//! Session settings and backend handles arrive as data. The engine has no
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
mod constraint;
mod cursor;
mod error;
#[cfg(test)]
#[path = "../../oxpinyin-core/tests/common/fixture_doubles.rs"]
mod fixture_doubles;
mod nbest;
mod phrase;
mod session;
mod storage;
#[cfg(feature = "tools")]
pub use session::tool;

pub use candidate::{Candidate, CandidateKind, CandidateList, rank_by_frequency};
pub use char_offset::{
    MatrixColumns, character_offset_over_keys, character_offset_with_walk_over_keys,
    key_pronounced_by, matrix_columns, remember_walk,
};
pub use cursor::{
    KeySpans, MatrixKey, left_word_offset, left_word_offset_over_spans, lookup_offset_for_cursor,
    lookup_offset_over_spans, right_word_offset, right_word_offset_over_spans,
};
pub use error::{EngineError, TrainingSpanFault};
pub use nbest::NbestShape;
pub use session::{
    CandidateRanking, KeyOutcome, Selection, Session, SessionSettings, SessionState, TrainingPlan,
    amplified_frequency, check_lookup_offset_range, normalize_lookup_offset, parse_time_keys,
};
pub use storage::StoragePaths;
