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
pub(crate) use char_offset::matrix_columns;
pub use char_offset::{CharacterOffset, key_pronounced_by, remember_walk};
pub(crate) use char_offset::{MatrixColumns, character_offset_walk};
pub use cursor::MatrixKey;
pub(crate) use cursor::lookup_offset_over_spans;
pub(crate) use cursor::{
    KeySpans, MatrixKeys, left_word_offset_over_spans, right_word_offset_over_spans,
};
pub use error::EngineError;
pub(crate) use error::{TrainingStop, abort_toned_incomplete};
pub use nbest::NbestShape;
pub(crate) use session::SentencePart;
pub use session::{
    BEFORE_CURSOR_ANCHOR, Law, RowText, SORT_WITHOUT_SENTENCE_CANDIDATE, Selection,
    SelectionTarget, Session, SessionSettings, SessionState, amplified_frequency,
};

pub(crate) use session::{check_lookup_offset_range, normalize_lookup_offset, parse_time_keys};
