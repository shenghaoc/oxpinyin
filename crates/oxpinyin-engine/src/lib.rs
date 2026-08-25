//! Session state machine — the supported Rust surface. Framework-neutral:
//! abstract KeyInput, injected config/paths, no platform types. Portable.
//!
//! The surface is frozen in `docs/findings/session-api.md`. A shell supplies
//! platform facts as data — a [`KeyInput`], a [`StoragePaths`], a
//! [`ConfigSource`] — and receives platform-free results: a [`Preedit`] of
//! text plus styled spans, and a [`CandidateList`] with checked indexing.
//!
//! What is deliberately not here: keysyms, GSettings, path discovery,
//! `cfg(target_os)`, threading requirements, clocks. Each is a place a
//! portable API usually leaks. IBus keysym translation lives in
//! `oxpinyin-capi`.
//!
//! ```
//! use oxpinyin_engine::{EmptyConfigSource, KeyInput, KeyOutcome, LogicalKey, Session, StoragePaths};
//! # use oxpinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};
//! # struct Empty;
//! # impl Dictionary for Empty {
//! #     type Syllable = SyllableKey;
//! #     type Entry = PhraseEntry;
//! #     type Error = std::convert::Infallible;
//! #     fn lookup(&self, _: &[SyllableKey]) -> Result<Vec<PhraseEntry>, Self::Error> { Ok(Vec::new()) }
//! # }
//! # impl LanguageModel for Empty {
//! #     type Token = PhraseToken;
//! #     type Error = std::convert::Infallible;
//! #     fn score(&self, _: &[PhraseToken], _: &PhraseToken, edge: Cost) -> Result<Cost, Self::Error> { Ok(edge) }
//! # }
//! let mut session = Session::new(
//!     &EmptyConfigSource,
//!     StoragePaths::new("/tmp/oxpinyin"),
//!     Empty,
//!     Empty,
//! )?;
//!
//! for character in "nihao".chars() {
//!     session.process_key(&KeyInput::character(character))?;
//! }
//! assert_eq!(session.preedit().text(), "nihao");
//! assert_eq!(
//!     session.process_key(&KeyInput::plain(LogicalKey::Enter))?,
//!     KeyOutcome::Commit("nihao".to_owned()),
//! );
//! # Ok::<(), oxpinyin_engine::EngineError>(())
//! ```
#![warn(missing_docs)]

mod candidate;
mod config;
mod constraint;
mod cursor;
mod error;
mod key;
mod nbest;
mod preedit;
mod session;
mod storage;

pub use candidate::{Candidate, CandidateKind, CandidateList};
pub use config::{
    Config, ConfigError, ConfigLayer, ConfigSource, ConfigValue, EmptyConfigSource,
    UPSTREAM_DEFAULT_COUNT, merge,
};
pub use cursor::{
    left_word_offset, left_word_offset_over_spans, lookup_offset_for_cursor,
    lookup_offset_over_spans, right_word_offset, right_word_offset_over_spans,
};
pub use error::EngineError;
pub use key::{KeyInput, LogicalKey, Modifiers};
pub use preedit::{Preedit, PreeditSpan, SpanStyle};
pub use session::{
    KeyOutcome, MAX_INPUT_BYTES, Selection, Session, check_lookup_offset_range,
    normalize_lookup_offset,
};
pub use storage::StoragePaths;
