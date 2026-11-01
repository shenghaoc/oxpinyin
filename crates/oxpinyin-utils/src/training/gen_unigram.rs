//! `gen_unigram` (`utils/training/gen_unigram.cpp` at libpinyin 074a2219).
//!
//! For the default table set and then the addon set, every `SYSTEM_FILE`
//! and `DICTIONARY` library's resident items gain one unigram count, and
//! the set's system and dictionary chunks are saved. `--table-dir` is
//! accepted and — as in the pin, whose static `table_dir` no statement
//! reads — never used.

use std::ffi::OsString;

use oxpinyin_data::table_info::{FileType, TableConf};
use oxpinyin_input::tool;

use crate::storage::libraries::Libraries;
use crate::training::{UNIGRAM_FILE_TYPES, load_table_conf};

/// `main` (`gen_unigram.cpp:81-113`).
pub fn main(args: impl IntoIterator<Item = OsString>) {
    let _ = tool::set_locale_from_environment();
    let entries = [crate::storage::table_dir_entry()];
    // `table_dir` is only the entry's storage; the program never reads it.
    let _parsed = crate::storage::parse_options(c"- increase uni-gram", &entries, args);

    let conf = load_table_conf();
    generate_unigram(&conf, false);
    generate_unigram(&conf, true);
}

/// `generate_unigram` (`gen_unigram.cpp:35-79`) over one table array.
fn generate_unigram(conf: &TableConf, addon: bool) {
    let mut libraries = Libraries::load_rows(conf, addon, &UNIGRAM_FILE_TYPES, tool::ENOENT);
    libraries.add_unigram_to_every_item();
    // `save_phrase_index` then `save_dictionary` of the same array; each
    // save failure writes `save %s failed.` and exits `ENOENT`
    // (`utils/utils_helper.h:97-145`).
    libraries.save_rows(conf, addon, FileType::SystemFile);
    libraries.save_rows(conf, addon, FileType::Dictionary);
}
