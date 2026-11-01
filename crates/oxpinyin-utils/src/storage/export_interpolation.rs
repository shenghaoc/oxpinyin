//! `export_interpolation` (`utils/storage/export_interpolation.cpp` at
//! libpinyin 074a2219).
//!
//! No options and no argument check (`main`, `:43-73`): the utility loads
//! `table.conf`'s system chunks and `bigram.db` from the current directory
//! and writes the interpolation model to `stdout`.

use core::ffi::c_long;
use std::ffi::OsString;
use std::path::Path;

use oxpinyin_data::single_gram::decode_single_gram;
use oxpinyin_input::tool;
use oxpinyin_input::tool::OutputStream;

use crate::storage::dbm::Hash;
use crate::storage::libraries::{Libraries, token_to_string};
use crate::storage::{load_table_conf, make_token};

/// `main` (`:43-73`), which reads no argument.
pub fn main(_args: impl IntoIterator<Item = OsString>) {
    let conf = load_table_conf();
    let libraries = Libraries::load(&conf);
    // `bigram.attach(bigram_filename, ATTACH_READONLY);` — the answer is
    // not checked; a failed attach leaves `m_db` without records.
    let bigram = Hash::open_read_only(Path::new("bigram.db")).ok();

    let Some(output) = OutputStream::stdout() else {
        // The C runtime's `stdout` is never NULL in a started process.
        tool::abort();
    };

    // `begin_data`.
    tool::storage::print_header(&output);
    gen_unigram(&output, &libraries);
    gen_bigram(&output, &libraries, bigram.as_ref());
    // `end_data`.
    tool::storage::print_end(&output);
}

/// `gen_unigram` (`:75-104`).
fn gen_unigram(output: &OutputStream, libraries: &Libraries) {
    tool::storage::print_gram_1(output);
    for index in 0..16_u8 {
        let Some(library) = &libraries.by_library(index) else {
            continue;
        };
        // `get_range`: every resident slot, ascending; an absent slot is
        // `ERROR_NO_ITEM` and skipped.
        for (slot, item) in &library.items {
            if *slot == 0 {
                continue;
            }
            let freq = item.unigram;
            if freq == 0 {
                continue;
            }
            let token = make_token(index, *slot);
            let Some(word) = token_to_string(libraries, token) else {
                continue;
            };
            tool::storage::print_unigram(output, token as i32, &word, c_long::from(freq));
        }
    }
}

/// `gen_bigram` (`:106-143`).
fn gen_bigram(output: &OutputStream, libraries: &Libraries, bigram: Option<&Hash>) {
    tool::storage::print_gram_2(output);
    let Some(bigram) = bigram else {
        return;
    };
    let keys = bigram.native_keys().unwrap_or_default();
    for key in keys {
        // `assert(key.size() == sizeof(phrase_token_t))`.
        if key.len() != 4 {
            tool::abort();
        }
        let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        // `bigram->load(token, single_gram)`: a key the walk just
        // returned without a loadable value is the pin's null gram.
        let Some(value) = bigram.get(&key) else {
            tool::abort();
        };
        let Ok((_total, records)) = decode_single_gram(&value) else {
            // `Bigram::load` handed a malformed value.
            tool::abort();
        };
        for (next, count) in records {
            let Some(word1) = token_to_string(libraries, token) else {
                continue;
            };
            let Some(word2) = token_to_string(libraries, next) else {
                continue;
            };
            tool::storage::print_bigram(
                output,
                token as i32,
                &word1,
                next as i32,
                &word2,
                count as i32,
            );
        }
    }
}
