//! `eval_correction_rate` (`utils/training/eval_correction_rate.cpp` at
//! libpinyin 074a2219).
//!
//! Reads `evals2.text` from the current directory, tokenises it at
//! `null_token` boundaries, and for each sentence lays out one key per cell
//! for the highest-frequency pronunciation of each token, then decodes the
//! key chain with `PhoneticLookup<1, 1>`'s single best path and compares the
//! two sentences. The final `correction rate:%f` is
//! `passed_count / tested_count`, a C double division.
//!
//! The decode in `eval` ports `PhoneticLookup<1, 1>`. Its model uses the
//! data directory's own chunks and `bigram.db`. Phrase lookup resolves the
//! pinyin index (`FacadeChewingTable2`) through the loaded phrase libraries.
//! The pin's failing `user_bigram.attach(NULL, ATTACH_CREATE|ATTACH_READWRITE)`
//! (`:156-157`) leaves the user bigram `NULL`, so `merge_single_gram`
//! always sees the system row alone and no user row is read.

use std::borrow::Cow;
use std::ffi::OsString;
use std::path::Path;

use oxpinyin::__internal::data::phrase_library::{LibraryError, RowFault};
use oxpinyin::__internal::data::{DictError, PhraseLibraries};
use oxpinyin::__internal::model::PhraseList;
use oxpinyin::__internal::vocab::{PhraseEntry, phrase_index_library_index};
use oxpinyin::facade::{ChewingKey, NULL_TOKEN, PhraseToken};
use oxpinyin_tool_support::data::PinyinIndexTable;
use oxpinyin_tool_support::input as tool;
use oxpinyin_tool_support::input::LineReader;

use super::eval::{EvalLanguageModel, LibraryPhraseSource, correction_rate};
use crate::training::{load_table_conf, parse_segmented_line};
use crate::utils_helper::{SystemLibraryError, load_system_libraries, tag_token_text};

/// The evaluation corpus name (`eval_correction_rate.cpp:133`).
const EVALS_TEXT: &std::ffi::CStr = c"evals2.text";

/// `main` (`eval_correction_rate.cpp:132-208`).
pub fn main(_args: impl IntoIterator<Item = OsString>) {
    let conf = load_table_conf();

    // `FacadeChewingTable2 largetable; largetable.load(SYSTEM_PINYIN_INDEX,
    // NULL);` — the answer is ignored: a failed load leaves an empty table,
    // whose searches find nothing and reach the result assertion below.
    let pinyin = PinyinIndexTable::open(Path::new("pinyin_index.bin")).ok();

    // `load_phrase_index(phrase_files, &phrase_index)`: every default
    // `SYSTEM_FILE` row; a refused chunk writes `load %s failed!` and ends
    // the program at `ENOENT`.
    let libraries = match load_system_libraries(&conf) {
        Ok(libraries) => libraries,
        Err(SystemLibraryError::MissingName) => tool::abort(),
        // `MemoryChunk::load`'s header assertion on a directory
        // (`memory_chunk.h:429`).
        Err(SystemLibraryError::Chunk(_, LibraryError::Row(RowFault::ChunkHeaderRead))) => {
            tool::abort()
        }
        Err(SystemLibraryError::Chunk(
            name,
            LibraryError::Io(_)
            | LibraryError::Format(_)
            | LibraryError::Layout(_)
            | LibraryError::Unmappable { .. }
            | LibraryError::Row(RowFault::NullChunkName),
        )) => {
            let c_name = oxpinyin_tool_support::input::c_path(Path::new(name.as_ref()));
            tool::storage::report_load_failed(c_name.as_deref());
            tool::exit(tool::ENOENT);
        }
    };

    // `system_bigram.attach(SYSTEM_BIGRAM, ATTACH_READONLY)`: ignored.
    let bigram = Some(
        crate::storage::Hash::open_read_only(Path::new("bigram.db"))
            .unwrap_or_else(|error| oxpinyin_tool_support::input::exit_on_store_error(&error)),
    );

    // `system_table_info.get_lambda()`, the `gfloat` the pin passes to
    // `PhoneticLookup<1, 1>`.
    let lambda = conf.lambda_f32();
    let model = EvalLanguageModel::new(&libraries, bigram.as_ref(), lambda);
    let source = LibraryPhraseSource {
        libraries: &libraries,
    };
    let dictionary = EvalDictionary {
        pinyin: pinyin.as_ref(),
        libraries: &libraries,
    };

    // `std::ifstream evals_file(evals_text); if (!evals_file)`.
    let mut evals = LineReader::open(EVALS_TEXT);
    if !evals.is_open() {
        tool::training::report_cant_open_file(EVALS_TEXT);
        tool::exit(tool::ENOENT);
    }

    let mut tested_count = 0_usize;
    let mut passed_count = 0_usize;
    let mut tokens: Vec<u32> = Vec::new();

    while evals.getline() {
        let token = parse_segmented_line(evals.c_str(), |token| token_text(&libraries, token));
        if token == NULL_TOKEN {
            // `/* one test. */`
            if !tokens.is_empty() {
                if do_one_test(&dictionary, &model, &source, &tokens) {
                    passed_count += 1;
                }
                tested_count += 1;
                tokens.clear();
            }
        } else {
            tokens.push(token);
        }
    }

    if !tokens.is_empty() {
        if do_one_test(&dictionary, &model, &source, &tokens) {
            passed_count += 1;
        }
        tested_count += 1;
    }

    // `parameter_t rate = passed_count / (parameter_t) tested_count;`
    let rate = passed_count as f64 / tested_count as f64;
    tool::training::print_correction_rate(rate);
}

/// One test (`do_one_test`, `eval_correction_rate.cpp:84-130`): `true` when
/// the decoded sentence equals the source sentence.
///
/// Failure to build or read a single decode aborts after the preceding
/// work. A completed guess that differs from the source prints both
/// sentences and counts as a mismatch.
fn do_one_test(
    dictionary: &EvalDictionary<'_>,
    model: &EvalLanguageModel,
    source: &LibraryPhraseSource<'_>,
    tokens: &[u32],
) -> bool {
    let sentence: Vec<PhraseToken> = tokens.to_vec();
    let Some(report) = correction_rate(dictionary, model, source, std::slice::from_ref(&sentence))
    else {
        tool::abort()
    };
    match report.mismatches.first() {
        None => true,
        Some((expected, guessed)) => {
            tool::training::report_test_sentence(&c_string(expected));
            tool::training::report_guessed_sentence(&c_string(guessed));
            tool::training::report_mismatch();
            false
        }
    }
}

/// The bytes of `text` up to its first NUL, as `%s` prints a C string.
fn c_string(text: &str) -> std::ffi::CString {
    crate::segment::c_string(text.as_bytes()).unwrap_or_default()
}

/// `taglib_token_to_string` (`src/storage/tag_utility.cpp:384-405`) over
/// the data layer's loaded libraries: `"<start>"` for token 1, the item's
/// text otherwise, and the pin's `error: unknown token:%d.` line before
/// `None`.
fn token_text(libraries: &PhraseLibraries, token: u32) -> Option<std::ffi::CString> {
    let text = tag_token_text(
        token,
        usize::from(phrase_index_library_index(token)) == 0,
        |token| libraries.library(token)?.item(token),
        |item| item.phrase_codes().collect::<Option<Vec<_>>>(),
        |token| tool::storage::report_unknown_token(token as core::ffi::c_int),
    )?;
    crate::segment::c_string(&text)
}

/// The pinyin index resolved through the loaded phrase libraries: the
/// `Dictionary` the decode's `search_matrix` and `SEARCH_CONTINUED` need.
struct EvalDictionary<'a> {
    pinyin: Option<&'a PinyinIndexTable>,
    libraries: &'a PhraseLibraries,
}

impl PhraseList for EvalDictionary<'_> {
    type Syllable = ChewingKey;
    type Error = DictError;

    fn lookup(&self, keys: &[ChewingKey]) -> Result<Vec<PhraseEntry>, DictError> {
        let Some(pinyin) = self.pinyin else {
            return Ok(Vec::new());
        };
        let keys = matrix_keys(keys);
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let tokens = pinyin.search(&keys)?;
        let mut entries = Vec::new();
        for token in tokens {
            // `resolve_item`: a token whose library is unloaded or whose
            // item cannot be read is dropped, upstream's `NULL == head`
            // skip.
            let Some(library) = self.libraries.library(token) else {
                continue;
            };
            let Some(item) = library.item(token) else {
                continue;
            };
            let Some(text) = item.phrase_text() else {
                continue;
            };
            let mut entry = PhraseEntry::new(token, text);
            if let Some((matched, total)) = self.libraries.pronunciation_possibility(token, &keys) {
                entry = entry.with_pronunciation_possibility(matched, total);
            }
            entries.push(entry);
        }
        Ok(entries)
    }

    /// `SEARCH_CONTINUED`: the query's index key exists in the DBM.
    fn phrase_prefix_exists(&self, keys: &[ChewingKey]) -> Result<bool, DictError> {
        let Some(pinyin) = self.pinyin else {
            return Ok(false);
        };
        let keys = matrix_keys(keys);
        if keys.is_empty() {
            return Ok(true);
        }
        Ok(pinyin.key_exists(&keys))
    }
}

// fill_matrix retains zero cells; search_matrix and pronunciation pricing
// skip them (`phonetic_key_matrix.cpp:389-393,567-574` at 074a2219).
fn matrix_keys(keys: &[ChewingKey]) -> Cow<'_, [ChewingKey]> {
    if keys.contains(&ChewingKey::default()) {
        Cow::Owned(
            keys.iter()
                .copied()
                .filter(|key| *key != ChewingKey::default())
                .collect(),
        )
    } else {
        Cow::Borrowed(keys)
    }
}
