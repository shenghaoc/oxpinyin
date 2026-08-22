//! Loads the exported data tables, validated by oracle cross-check.
//! Portable: no glib, no Linux-only deps. Internal crate — the supported
//! public API is `oxpinyin-engine`.
//!
//! The data tables are produced by `oxpinyin-migrate` from the pinned
//! oracle per `docs/findings/data-layer-export.md`; the custom-content
//! loader follows `docs/findings/data-formats.md`. This crate only
//! reads them.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod content;
pub mod dict;
pub(crate) mod initials;
pub mod interp;
pub mod lm;
pub mod punct;
pub mod table;
pub mod table_conf;

pub use content::{ContentTable, LoadError, Record, TokenPair};
pub use dict::{DictError, SystemDictionary};
pub use interp::{
    InterpolationError, UnigramTable, parse_interpolation2, parse_interpolation2_from_reader,
};
pub use lm::{BigramLanguageModel, BigramRow, LmError, merge_bigram, merge_counts};
pub use oxpinyin_core::UserCountDelta;
pub use punct::PunctTable;
pub use table::{LookupTable, TableError};
pub use table_conf::{Lambda, PINNED_LAMBDA, parse_table_conf_lambda, read_table_conf_lambda};
