//! Loads the exported data tables, validated by oracle cross-check.
//! Portable: no glib, no Linux-only deps. Internal crate — the supported
//! public API is `oxpinyin-engine`.
//!
//! The data tables are committed under `fixtures/w3/` (frozen; no longer
//! regenerated in-tree) per `docs/findings/data-layer-export.md`; the
//! custom-content loader follows `docs/findings/data-formats.md`.  This
//! crate only reads them.
#![deny(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

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
// The compiled-in backend's native-table extension and filename helper,
// re-exported so consumers above this crate (runtime, capi, tests) name
// data files consistently with the backend the build selected —
// oxpinyin-runtime has no direct store edge.
pub use oxpinyin_store::{DEFAULT_STORE_EXT, default_store_file};
pub use punct::PunctTable;
pub use table::{GenericLookupTable, LookupTable, TableError};
pub use table_conf::{Lambda, PINNED_LAMBDA, parse_table_conf_lambda, read_table_conf_lambda};
