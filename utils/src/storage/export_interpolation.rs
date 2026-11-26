//! `export_interpolation` (`utils/storage/export_interpolation.cpp` at
//! libpinyin 074a2219).
//!
//! No options and no argument check (`main`, `:43-73`): the utility loads
//! `table.conf`'s system chunks and `bigram.db` from the current directory
//! and writes the interpolation model to `stdout`.

use std::ffi::OsString;
use std::path::Path;

use oxpinyin::vocab::{PHRASE_INDEX_LIBRARY_COUNT, PHRASE_MASK, phrase_index_make_token};
use oxpinyin_tool_support::data::decode_single_gram;
use oxpinyin_tool_support::input as tool;
use oxpinyin_tool_support::input::OutputStream;

use crate::storage::Hash;
use crate::storage::load_table_conf;
use crate::storage::{Libraries, Library, token_to_string};

/// `main` (`:43-73`), which reads no argument.
pub fn main(_args: impl IntoIterator<Item = OsString>) {
    let conf = load_table_conf();
    let libraries = Libraries::load(&conf);
    // `bigram.attach(bigram_filename, ATTACH_READONLY);` — the answer is
    // not checked; a failed attach leaves `m_db` without records.
    let bigram = Some(
        Hash::open_read_only(Path::new("bigram.db"))
            .unwrap_or_else(|error| oxpinyin_tool_support::input::exit_on_store_error(&error)),
    );

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
    for index in 0..PHRASE_INDEX_LIBRARY_COUNT {
        // `get_range(i, range)` (`:80-82`): a library that is not loaded
        // is `ERROR_NO_SUB_PHRASE_INDEX` and skipped.
        let Some(library) = &libraries.by_library(index) else {
            continue;
        };
        for (token, item) in walked_items(index, library) {
            let freq = item.unigram;
            if freq == 0 {
                continue;
            }
            let Some(word) = token_to_string(libraries, token) else {
                continue;
            };
            tool::storage::print_unigram(output, token as i32, &word, i64::from(freq));
        }
    }
}

/// The items `gen_unigram` reads from library `index`, as `(token, item)`
/// pairs in slot order.
///
/// The pin walks `for (token = range.m_range_begin; token <
/// range.m_range_end; token++)` (`:85-86`). `FacadePhraseIndex::get_range`
/// makes both ends tokens with `PHRASE_INDEX_MAKE_TOKEN`
/// (`src/storage/phrase_index.cpp:619-620`), which keeps only the low 24
/// bits of `SubPhraseIndex::get_range`'s end, the offset count with
/// trailing zeros trimmed (`:624-645`). So the walk covers slots 1 to
/// `range_end & PHRASE_MASK`, and nothing when that end is 0 or 1.
/// `get_phrase_item` reads the slot `token & PHRASE_MASK` (`:180-187`), and
/// an empty slot is `ERROR_NO_ITEM` and skipped (`export_interpolation.cpp:87-90`):
/// only the resident items in that range are read.
fn walked_items(
    index: u8,
    library: &Library,
) -> impl Iterator<Item = (u32, &oxpinyin::data::phrase_library::ChunkItem)> {
    let end = library.range_end & PHRASE_MASK;
    (end > 1)
        .then(|| library.items.range(1..end))
        .into_iter()
        .flatten()
        .map(move |(&slot, item)| (phrase_index_make_token(index, slot), item))
}

/// `gen_bigram` (`:106-143`).
fn gen_bigram(output: &OutputStream, libraries: &Libraries, bigram: Option<&Hash>) {
    tool::storage::print_gram_2(output);
    let Some(bigram) = bigram else {
        return;
    };
    let keys = bigram.native_keys().unwrap_or_default();
    let Some(tokens) = checked_tokens(&keys) else {
        // `assert(key.size() == sizeof(phrase_token_t))` in
        // `get_all_items` (`ngram_bdb.cpp:199`): every key is checked
        // before the first `\item` line is printed.
        tool::abort();
    };
    for token in tokens {
        let key = token.to_le_bytes();
        // `bigram->load(token, single_gram)`: a key the walk just
        // returned without a loadable value is the pin's null gram.
        let Some(value) = bigram.get(&key) else {
            tool::abort();
        };
        let Some((_total, records)) = decode_single_gram(&value) else {
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

/// The tokens of `keys`, the pin's `m_keys` after `get_all_items`' size
/// assert: every key is checked before a record is printed. `None` where a
/// key is not one `phrase_token_t`; the caller aborts there.
fn checked_tokens(keys: &[Vec<u8>]) -> Option<Vec<u32>> {
    keys.iter()
        .map(|key| {
            Some(u32::from_le_bytes(
                <[u8; 4]>::try_from(key.as_slice()).ok()?,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests;
