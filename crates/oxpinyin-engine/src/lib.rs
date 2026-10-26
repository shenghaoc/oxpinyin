//! Session state machine — the internal session layer under the C facades. Framework-neutral:
//! abstract `KeyInput`, injected config/paths, no platform types. Portable.
//!
//! The behaviour is specified in `docs/findings/session-api.md`; the Rust
//! signatures carry no stability promise. A shell supplies
//! platform facts as data — a [`KeyInput`], a [`StoragePaths`], a
//! [`ConfigSource`] — and receives platform-free results: a [`Preedit`] of
//! text plus styled spans, and a [`CandidateList`] with checked indexing.
//!
//! What is deliberately not here: keysyms, `GSettings`, path discovery,
//! `cfg(target_os)`, threading requirements, clocks. Each is a place a
//! portable API usually leaks. `IBus` keysym translation lives in
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
//! #     type PathMemo = ();
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
//!     KeyOutcome::Commit("nihao".into()),
//! );
//! # Ok::<(), oxpinyin_engine::EngineError>(())
//! ```
#![forbid(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
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
mod key;
mod nbest;
mod phrase;
mod preedit;
mod session;
mod storage;

pub use candidate::{Candidate, CandidateKind, CandidateList};
pub use char_offset::character_offset_over_keys;
pub use config::{
    Config, ConfigError, ConfigLayer, ConfigSource, ConfigValue, EmptyConfigSource,
    UPSTREAM_DEFAULT_COUNT, merge,
};
pub use cursor::{
    MatrixKey, left_word_offset, left_word_offset_over_spans, lookup_offset_for_cursor,
    lookup_offset_over_spans, right_word_offset, right_word_offset_over_spans,
};
pub use error::{EngineError, TrainingSpanFault};
pub use key::{KeyInput, LogicalKey, Modifiers};
pub use nbest::{NbestShape, glib_merge_sort};
pub use preedit::{Preedit, PreeditSpan, SpanStyle};
pub use session::{
    KeyOutcome, MAX_INPUT_BYTES, Selection, Session, check_lookup_offset_range,
    normalize_lookup_offset,
};
pub use storage::StoragePaths;
