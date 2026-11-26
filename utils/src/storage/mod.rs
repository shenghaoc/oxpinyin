//! The pin's storage utilities (`utils/storage/*.cpp` at libpinyin
//! 074a2219): `gen_binary_files`, `import_interpolation` and
//! `export_interpolation`.
//!
//! Each utility reads the cwd-relative names the pin does
//! (`src/include/pinyin_internal.h:55-66`), through the pin's own calls in
//! `oxpinyin_tool_support::input::storage`; the table text, the DBM records and the
//! per-library chunks are the bytes `oxpinyin`'s layouts describe.
//!
//! - [`gen_binary_files`] turns `table.conf`'s `.table` files into the
//!   two index DBMs, the per-library chunk files and — with
//!   `--gen-punct-table` — `punct.bin`.
//! - [`import_interpolation`] applies an interpolation model from stdin to
//!   the chunk files and `bigram.db`.
//! - [`export_interpolation`] writes the model that
//!   [`import_interpolation`] reads.
//!
//! The model20 producer is libpinyin's own three-program chain
//! (`gen_binary_files`, `import_interpolation`, `gen_unigram`); this module
//! supplies the first two and `export_interpolation`, which writes the
//! model text the second one reads, and `training::gen_unigram` completes
//! the chain. The committed `fixtures/w3` sets are that sequence's output
//! over the mini subset of model20 (`tools/dev/w3_fixtures.py`), frozen.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

use std::ffi::CString;

use oxpinyin::data::{TableConf, TableConfError};
use oxpinyin_tool_support::input as tool;
use oxpinyin_tool_support::input::{OptionArg, OptionEntry};

mod dbm;
pub mod export_interpolation;
pub mod gen_binary_files;
pub mod import_interpolation;
mod libraries;
mod pinyin;
pub(crate) mod punctuation;
mod table_text;

/// `{"table-dir", 0, 0, G_OPTION_ARG_FILENAME, &table_dir, "table
/// directory", NULL}` (`gen_binary_files.cpp:31`,
/// `import_interpolation.cpp:38`).
pub(crate) fn table_dir_entry() -> OptionEntry {
    OptionEntry {
        long_name: c"table-dir",
        short_name: 0,
        flags: 0,
        arg: OptionArg::Filename(Some(c".")),
        description: Some(c"table directory"),
        arg_description: None,
    }
}

/// `{"gen-punct-table", 0, 0, G_OPTION_ARG_NONE, &gen_punct_table,
/// "generate punctuation table", NULL}` (`gen_binary_files.cpp:32`).
pub(crate) fn gen_punct_entry() -> OptionEntry {
    OptionEntry {
        long_name: c"gen-punct-table",
        short_name: 0,
        flags: 0,
        arg: OptionArg::Flag(false),
        description: Some(c"generate punctuation table"),
        arg_description: None,
    }
}

/// `SystemTableInfo2::load("table.conf")` followed by each utility's
/// `load table.conf failed.` line and `exit(ENOENT)`; the pin's `abort()`
/// sites are the same aborts.
pub(crate) fn load_table_conf() -> TableConf {
    match TableConf::load(c"table.conf") {
        Ok(conf) => conf,
        Err(TableConfError::Abort(_) | TableConfError::UninitialisedSourceFormat) => tool::abort(),
        Err(TableConfError::Unopenable | TableConfError::Header) => {
            tool::storage::report_table_conf_failed();
            tool::exit(tool::ENOENT);
        }
    }
}

/// The name bytes of one `table.conf` column.
pub(crate) fn os_bytes(name: &std::ffi::OsStr) -> Vec<u8> {
    oxpinyin::input::path_to_bytes(std::path::Path::new(name)).to_vec()
}

/// The `CString` a `table.conf` name column names.
pub(crate) fn name_c_string(name: &std::ffi::OsStr) -> Option<CString> {
    CString::new(os_bytes(name)).ok()
}

pub use dbm::Tree;
pub(crate) use dbm::{Gram, Hash, load_gram, store_gram};

pub(crate) use libraries::{Libraries, Library, items_of, token_to_string};
pub(crate) use table_text::{read_pinyin_record, read_punct_record};
