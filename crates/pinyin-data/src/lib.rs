//! Loads the libpinyin-format data tables, validated by oracle cross-check.
//! Portable: no glib, no Linux-only deps. Internal crate — the supported
//! public API is `pinyin-engine`.
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod content;

pub use content::{ContentTable, LoadError, Record, TokenPair};
