//! Internal session state and decoding operations used by the C facades.
//!
//! Session settings and backend handles arrive as data. The engine has no
//! frontend key dispatch, display model or platform path discovery.

mod candidate;
mod char_offset;
mod constraint;
mod cursor;
mod error;
mod nbest;
mod phrase;
mod session;
mod storage;
#[cfg(feature = "tools")]
pub use session::tool;

pub use candidate::{Candidate, CandidateKind, CandidateList, rank_by_frequency};
pub use char_offset::{
    MatrixColumns, character_offset_with_walk_over_keys, key_pronounced_by, matrix_columns,
    remember_walk,
};
pub use cursor::{
    KeySpans, MatrixKey, MatrixKeys, left_word_offset_over_spans, lookup_offset_over_spans,
    right_word_offset_over_spans,
};
pub use error::{EngineError, TrainingSpanFault};
pub use nbest::NbestShape;
pub use session::{
    BEFORE_CURSOR_ANCHOR, CandidateRanking, RowText, SORT_WITHOUT_SENTENCE_CANDIDATE, Selection,
    SelectionTarget, SentencePart, Session, SessionSettings, SessionState, TrainingPlan,
    amplified_frequency,
};
pub use storage::StoragePaths;

pub(crate) use session::{check_lookup_offset_range, normalize_lookup_offset, parse_time_keys};
