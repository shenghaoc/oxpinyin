//! The pin's training utilities (`utils/training/*.cpp` at libpinyin
//! 074a2219): `gen_unigram`, `gen_ngram`, `gen_deleted_ngram`,
//! `estimate_interpolation` and `eval_correction_rate`.
//!
//! Each utility reads the cwd-relative names the pin does
//! (`src/pinyin_internal.h:55-60`) through the same boundary the storage
//! utilities use; the chunk files, the `SingleGram` bigram rows and the
//! phrase items are the bytes `oxpinyin`'s layouts describe, and the
//! correction-rate decode is `eval`'s port of
//! `PhoneticLookup<1, 1>`.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

use oxpinyin::facade::NULL_TOKEN;
use std::ffi::{CStr, CString};

use oxpinyin::__internal::data::FileType;

use oxpinyin_tool_support::input as tool;
use oxpinyin_tool_support::input::{OptionArg, OptionEntry};

use crate::storage::Hash;

pub mod estimate_interpolation;
mod eval;
pub mod eval_correction_rate;
pub mod gen_deleted_ngram;
pub mod gen_ngram;
pub mod gen_unigram;

pub(crate) use crate::storage::{Gram, load_gram, store_gram};

/// `Bigram::get_all_items` (`src/storage/ngram.cpp:180-215`): every
/// stored previous token in the container's own walk order. A key that is
/// not one `phrase_token_t` trips the pin's
/// `assert(key.size == sizeof(phrase_token_t))`.
pub(crate) fn all_tokens(bigram: Option<&Hash>) -> Vec<u32> {
    let Some(bigram) = bigram else {
        return Vec::new();
    };
    let Ok(keys) = bigram.native_keys() else {
        return Vec::new();
    };
    keys.into_iter()
        .map(|key| match <[u8; 4]>::try_from(key.as_slice()) {
            Ok(bytes) => u32::from_le_bytes(bytes),
            Err(_) => tool::abort(),
        })
        .collect()
}

/// `TAGLIB_PARSE_SEGMENTED_LINE` (`utils/utils_helper.h:51-70`): the token
/// a segmented line names, after the pin's checks.
///
/// An empty line leaves `null_token` (`strlen` check). Otherwise
/// `g_strsplit_set(line, " \t", 2)` must yield exactly two strings —
/// otherwise the macro's `abort()` — and `atoi` reads the first. A non-null
/// token is then compared with the second string through
/// `taglib_validate_token_with_string`, whose `check_result` is `assert`
/// in the reference build: a mismatch aborts. `token_text` is
/// `taglib_token_to_string`: `None` where that helper answers NULL.
///
/// The pin's comparison is `strcmp(taglib_token_to_string(...), phrase)`;
/// where the helper answers NULL (an unknown token, after its own
/// `error: unknown token:%d.` line) the `strcmp` reads the NULL pointer. oxpinyin writes the same line and exits 1 instead, the
/// established refusal for that class.
pub(crate) fn parse_segmented_line(
    line: &CStr,
    token_text: impl Fn(u32) -> Option<CString>,
) -> u32 {
    if line.to_bytes().is_empty() {
        return NULL_TOKEN;
    }
    let parts = tool::strsplit_set(line, c" \t", 2);
    if parts.len() != 2 {
        tool::abort();
    }
    let first = &parts[0];
    let phrase = &parts[1];
    let token = tool::atoi(first) as u32;
    if token != NULL_TOKEN {
        let Some(text) = token_text(token) else {
            tool::exit(1);
        };
        if text.to_bytes() != phrase.to_bytes() {
            // `check_result(taglib_validate_token_with_string(...))`.
            tool::abort();
        }
    }
    token
}

/// The trainers' refused `SystemTableInfo2::load` is the storage tools'
/// (`gen_unigram.cpp:96-101`, `gen_ngram.cpp:58-64`,
/// `gen_deleted_ngram.cpp:59-65`, `estimate_interpolation.cpp:91-97`,
/// `eval_correction_rate.cpp:135-141`).
pub(crate) use crate::storage::load_table_conf;

/// The `SYSTEM_FILE`/`DICTIONARY` library types `gen_unigram` keeps
/// (`gen_unigram.cpp:45-47`).
pub(crate) const UNIGRAM_FILE_TYPES: [FileType; 2] = [FileType::SystemFile, FileType::Dictionary];

/// The `G_OPTION_FLAG_REVERSE`/`G_OPTION_ARG_NONE` entry the two n-gram
/// trainers share (`gen_ngram.cpp:40`, `gen_deleted_ngram.cpp:40`):
/// `--skip-pi-gram-training` clears a `gboolean` that starts TRUE.
pub(crate) fn skip_pi_gram_entry() -> OptionEntry {
    OptionEntry {
        long_name: c"skip-pi-gram-training",
        short_name: 0,
        // `G_OPTION_FLAG_REVERSE` (`glib/goption.h`).
        flags: 4,
        arg: OptionArg::Flag(true),
        description: Some(c"skip pi-gram training"),
        arg_description: None,
    }
}

/// The `--bigram-file` entry (`gen_ngram.cpp:41`).
pub(crate) fn bigram_entry() -> OptionEntry {
    filename_entry(c"bigram-file", c"bigram.db", c"bi-gram file")
}

/// The `--deleted-bigram-file` entry (`gen_deleted_ngram.cpp:41`).
pub(crate) fn deleted_bigram_entry() -> OptionEntry {
    filename_entry(
        c"deleted-bigram-file",
        c"deleted_bigram.db",
        c"deleted bi-gram file",
    )
}

/// One `G_OPTION_ARG_FILENAME` entry over a static default.
fn filename_entry(
    long_name: &'static CStr,
    default: &'static CStr,
    description: &'static CStr,
) -> OptionEntry {
    OptionEntry {
        long_name,
        short_name: 0,
        flags: 0,
        arg: OptionArg::Filename(Some(default)),
        description: Some(description),
        arg_description: None,
    }
}

/// A C string's bytes as the path the store opens. On Unix the bytes are
/// the path; Windows uses the shared lossy UTF-8 filename decoder.
pub(crate) fn path_of(name: &CStr) -> std::path::PathBuf {
    oxpinyin::__internal::input::path_from_bytes(name.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::Gram;

    /// `SingleGram::get_freq` and `set_freq` walk from `lower_bound`
    /// (`ngram.cpp:231-250`): the first of equal successor tokens is the
    /// one read and written, not whichever a binary search lands on.
    #[test]
    fn a_gram_reads_and_writes_the_first_of_equal_tokens() {
        let mut gram = Gram {
            total: 3,
            records: vec![(7, 1), (7, 2)],
        };
        assert_eq!(gram.get_freq(7), Some(1));
        assert!(gram.set_freq(7, 9));
        assert_eq!(gram.records, vec![(7, 9), (7, 2)]);
        assert!(!gram.insert_freq(7, 0), "the token is already present");
    }
}
