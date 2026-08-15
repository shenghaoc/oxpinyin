//! Training-only `interpolation2.text` emitter.
//!
//! Reproduces libpinyin `export_interpolation`
//! (`utils/storage/export_interpolation.cpp`): T2's integer
//! [`pinyin_counter::Counts`] in, the textual model format
//! [`pinyin_data::parse_interpolation2`] already reads out. Never ships
//! with the engine. See `docs/findings/emitter-port.md`.
//!
//! λ is **not** written. PR #55 settled that the shipped interpolation
//! weight lives in `table.conf` (`lambda parameter:0.312699`) and is
//! read at decode time; `export_interpolation` emits n-gram records
//! only. A `table.conf` emitter is out of scope (T3 already produces
//! the λ value).
#![deny(unsafe_code)]
#![warn(missing_docs)]

mod emit;
mod error;

pub use emit::{SENTENCE_START_TEXT, emit_interpolation2, phrase_text, write_interpolation2};
pub use error::EmitterError;
