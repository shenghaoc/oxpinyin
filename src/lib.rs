//! Provides shared parsing, decoding, and storage for the libpinyin and libzhuyin C libraries.
//!
//! Native Rust interfaces are unsupported internal details.
//! Each build selects exactly one database backend. The default is `tkrzw`.
//!
//! ## Aborts
//!
//! Malformed or corrupt stored data and failed backend operations can abort the process,
//! as in libpinyin. Individual items omit these aborts. Their `Aborts` sections describe
//! termination caused by caller arguments or call order, or by an operation that exits
//! the process.
//!
//! ## Further reading
//!
//! - [Supported external interfaces](https://github.com/shenghaoc/oxpinyin/blob/main/docs/constitution.md#external-interfaces)
//! - [Backend selection](https://github.com/shenghaoc/oxpinyin/blob/main/docs/runbooks/backends.md#selecting-a-backend)
//! - [Upstream pin](https://github.com/shenghaoc/oxpinyin/blob/main/tools/oracle/oracle-pin.txt)
#![deny(unsafe_code)]
#![warn(missing_docs)]
// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]

// The fixture doubles name this crate by its package name.
#[cfg(test)]
extern crate self as oxpinyin;

#[forbid(unsafe_code)]
#[doc(hidden)]
pub mod __internal;
#[forbid(unsafe_code)]
mod chewing_key;
#[forbid(unsafe_code)]
mod chewing_key_data;
#[forbid(unsafe_code)]
pub mod data;
#[forbid(unsafe_code)]
mod direct;
#[forbid(unsafe_code)]
pub mod engine;
#[forbid(unsafe_code)]
pub mod facade;
#[cfg(test)]
#[path = "../tests/common/fixture_doubles.rs"]
#[forbid(unsafe_code)]
mod fixture_doubles;
#[forbid(unsafe_code)]
mod full_pinyin_index;
#[forbid(unsafe_code)]
pub mod graph;
pub mod input;
#[forbid(unsafe_code)]
pub mod marshal;
#[forbid(unsafe_code)]
pub mod model;
#[forbid(unsafe_code)]
mod options;
#[forbid(unsafe_code)]
mod packed_records;
#[forbid(unsafe_code)]
pub mod parser;
#[cfg(test)]
#[path = "../tests/common/parser_rows.rs"]
#[forbid(unsafe_code)]
mod parser_rows;
#[forbid(unsafe_code)]
pub mod runtime;
#[forbid(unsafe_code)]
mod scheme;
#[forbid(unsafe_code)]
mod sentence_facts;
#[forbid(unsafe_code)]
pub mod single_gram;
#[forbid(unsafe_code)]
mod static_strings;
pub mod store;
#[forbid(unsafe_code)]
mod ucs4;
#[forbid(unsafe_code)]
pub mod user;
#[forbid(unsafe_code)]
mod string_tables {
    use crate::options::table_row::{
        ZHUYIN_CORRECT_ETEN26, ZHUYIN_CORRECT_HSU, ZHUYIN_CORRECT_SHUFFLE, ZHUYIN_INCOMPLETE,
    };
    include!(concat!(env!("OUT_DIR"), "/string_tables.rs"));
}
#[forbid(unsafe_code)]
mod syllables;
#[forbid(unsafe_code)]
pub mod vocab;
#[forbid(unsafe_code)]
mod zhuyin_map;

pub use options::OptionBits;
