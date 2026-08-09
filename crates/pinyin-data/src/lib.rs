//! Loads the libpinyin-format data tables, validated by oracle cross-check.
//! Portable: no glib, no Linux-only deps. Internal crate — the supported
//! public API is `pinyin-engine`.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod content;
pub mod dict;
/// SyllableKey → TableKey encoder (blocked).
pub mod encoder;
pub mod lm;
pub mod table;
pub mod types;

pub use content::{ContentTable, LoadError, Record, TokenPair};
pub use dict::{DictError, SystemDictionary};
pub use encoder::{EncoderError, encode as encode_syllable};
pub use lm::{BigramLanguageModel, LmError};
pub use table::LookupTable;
pub use types::{BigramEntry, BigramKey, PhraseEntry, PhraseToken, SyllableKey, TableKey};
