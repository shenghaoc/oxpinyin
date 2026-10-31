//! The pin's storage utilities (`utils/storage/*.cpp` at libpinyin
//! 074a2219): `gen_binary_files`, `import_interpolation` and
//! `export_interpolation`.
//!
//! Each utility reads the cwd-relative names the pin does
//! (`src/include/pinyin_internal.h:55-66`), through the pin's own calls in
//! `oxpinyin_input::tool::storage`; the table text, the DBM records and the
//! per-library chunks are the bytes `oxpinyin-data`'s layouts describe.
//!
//! - [`gen_binary_files`] turns `table.conf`'s `.table` files into the
//!   two index DBMs, the per-library chunk files and — with
//!   `--gen-punct-table` — `punct.bin`.
//! - [`import_interpolation`] applies an interpolation model from stdin to
//!   the chunk files and `bigram.db`.
//! - [`export_interpolation`] writes the model that
//!   [`import_interpolation`] reads.
//!
//! `oxpinyin-datagen compile` remains the model20 producer until the
//! pin's `gen_unigram` is ported: its output is the three-program chain
//! (`gen_binary_files`, `import_interpolation`, `gen_unigram`); this
//! member supplies the first two and `export_interpolation`, which writes
//! the model text the second one reads.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

use std::ffi::{CStr, CString, OsString};

use oxpinyin_data::table_info::{TableConf, TableConfError};
use oxpinyin_input::tool;
use oxpinyin_input::tool::{OptionArg, OptionEntry, OptionValue, ParsedOptions};

pub mod dbm;
pub mod export_interpolation;
pub mod gen_binary_files;
pub mod import_interpolation;
pub mod libraries;
pub mod pinyin;
pub mod table_text;

/// `MAX_PHRASE_LENGTH` (`src/include/novel_types.h:119`), the largest
/// index length `ChewingLargeTable2`'s switch accepts.
pub(crate) const MAX_PHRASE_LENGTH: usize = 16;

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

/// The `--table-dir` value the entry started at (`.` when unset).
pub(crate) fn table_dir(parsed: &ParsedOptions, index: usize) -> Option<CString> {
    match parsed.values.get(index) {
        Some(OptionValue::Filename(Some(bytes))) => CString::new(bytes.clone()).ok(),
        _ => None,
    }
}

/// `g_option_context_parse` with the pin's failure line already written; a
/// refused parse is `exit(EINVAL)` (`gen_binary_files.cpp:119-121`).
pub(crate) fn parse_options(
    parameter_string: &'static CStr,
    entries: &[OptionEntry],
    args: impl IntoIterator<Item = OsString>,
) -> ParsedOptions {
    match tool::parse_options(parameter_string, entries, args) {
        Ok(parsed) => parsed,
        Err(_) => tool::exit(tool::EINVAL),
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

/// `PHRASE_INDEX_LIBRARY_INDEX(token)` (`src/include/novel_types.h:41`).
pub(crate) const fn library_of(token: u32) -> u8 {
    (token >> 24) as u8
}

/// `PHRASE_INDEX_MAKE_TOKEN(index, token)`.
pub(crate) const fn make_token(library: u8, slot: u32) -> u32 {
    ((library as u32) << 24) | slot
}

/// The name bytes of one `table.conf` column.
pub(crate) fn os_bytes(name: &std::ffi::OsStr) -> Vec<u8> {
    name.as_encoded_bytes().to_vec()
}

/// The `CString` a `table.conf` name column names.
pub(crate) fn name_c_string(name: &std::ffi::OsStr) -> Option<CString> {
    CString::new(os_bytes(name)).ok()
}
