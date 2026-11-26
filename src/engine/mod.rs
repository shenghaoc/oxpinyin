//! Session state, candidate selection, constraints, training plans, and decoding.
//!
//! [`Session`] operates on dictionary and language-model views from [`crate::model`].
//! [`SessionState`] retains parse and result state; [`SessionSettings`] supplies
//! the option word, schemes and candidate ranking.
//!
//! Session settings and backend handles arrive as data. The engine has no
//! frontend key dispatch, display model or platform path discovery.

mod candidate;
mod char_offset;
mod constraint;
mod cursor;
mod error;
pub(crate) mod nbest;
pub(crate) mod phrase;
pub(crate) mod session;

pub use candidate::{Candidate, CandidateKind, CandidateList, rank_by_frequency};
pub use char_offset::{
    CharacterOffset, MatrixColumns, character_offset_walk, key_pronounced_by, matrix_columns,
    remember_walk,
};
pub use cursor::{
    KeySpans, MatrixKey, MatrixKeys, left_word_offset_over_spans, lookup_offset_over_spans,
    right_word_offset_over_spans,
};
pub use error::EngineError;
pub(crate) use error::{TrainingStop, abort_toned_incomplete};
pub use nbest::NbestShape;
pub use session::{
    BEFORE_CURSOR_ANCHOR, CandidateRanking, RowText, SORT_WITHOUT_SENTENCE_CANDIDATE, Selection,
    SelectionTarget, SentencePart, Session, SessionSettings, SessionState, amplified_frequency,
};

pub(crate) use session::{check_lookup_offset_range, normalize_lookup_offset, parse_time_keys};
