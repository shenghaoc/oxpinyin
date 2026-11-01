//! The pin's training utilities (`utils/training/*.cpp` at libpinyin
//! 074a2219): `gen_unigram`, `gen_ngram`, `gen_deleted_ngram`,
//! `estimate_interpolation` and `eval_correction_rate`.
//!
//! Each utility reads the cwd-relative names the pin does
//! (`src/pinyin_internal.h:55-60`) through the same boundary the storage
//! utilities use; the chunk files, the `SingleGram` bigram rows and the
//! phrase items are the bytes `oxpinyin-data`'s layouts describe, and the
//! correction-rate decode is `oxpinyin_training::eval`'s port of
//! `PhoneticLookup<1, 1>`.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]

use std::ffi::{CStr, CString};

use oxpinyin_data::row_format::encode_token_key;
use oxpinyin_data::single_gram::{decode_single_gram, encode_single_gram};
use oxpinyin_data::table_info::{FileType, TableConf, TableConfError};

use oxpinyin_input::tool;
use oxpinyin_input::tool::{OptionArg, OptionEntry, OptionValue, ParsedOptions};

use crate::storage::dbm::Hash;

pub mod estimate_interpolation;
pub mod eval_correction_rate;
pub mod gen_deleted_ngram;
pub mod gen_ngram;
pub mod gen_unigram;

/// `null_token` (`src/include/novel_types.h:121`).
pub(crate) const NULL_TOKEN: u32 = 0;
/// `sentence_start` (`src/include/novel_types.h:122`).
pub(crate) const SENTENCE_START: u32 = 1;

/// One `SingleGram` row: the stored `total_freq` and its
/// `{next_token, count}` records in stored (token-ascending) order
/// (`src/storage/ngram.cpp:31-57`).
pub(crate) struct Gram {
    /// `SingleGram::m_chunk`'s first word.
    pub total: u32,
    /// The item array after it.
    pub records: Vec<(u32, u32)>,
}

impl Gram {
    /// `SingleGram::get_freq`: the count stored for `token`, `None` where
    /// the pin's binary search finds no record.
    pub(crate) fn get_freq(&self, token: u32) -> Option<u32> {
        self.records
            .binary_search_by_key(&token, |(next, _)| *next)
            .ok()
            .map(|index| self.records[index].1)
    }

    /// `SingleGram::set_freq`: `false` where the record is absent
    /// (`check_result` is `assert` in the reference build).
    pub(crate) fn set_freq(&mut self, token: u32, freq: u32) -> bool {
        match self.records.binary_search_by_key(&token, |(next, _)| *next) {
            Ok(index) => {
                self.records[index].1 = freq;
                true
            }
            Err(_) => false,
        }
    }

    /// `SingleGram::insert_freq`: `false` where the token is already
    /// present; otherwise the record goes at its lower-bound position.
    pub(crate) fn insert_freq(&mut self, token: u32, freq: u32) -> bool {
        match self.records.binary_search_by_key(&token, |(next, _)| *next) {
            Ok(_) => false,
            Err(position) => {
                self.records.insert(position, (token, freq));
                true
            }
        }
    }

    /// `SingleGram::m_chunk`'s bytes (`encode_single_gram`), the value
    /// `Bigram::store` writes under the previous token's key.
    pub(crate) fn encode(&self) -> Vec<u8> {
        encode_single_gram(self.total, &self.records)
    }
}

/// `Bigram::load` of `token` from `bigram`: `None` where the container is
/// unopened (`m_db` NULL) or has no such key.
///
/// A stored value that is not a whole `SingleGram` would make the pin's
/// `get_total_freq`/`get_freq` read past the value (class (b)); oxpinyin
/// refuses it by the abort it already uses for a corrupt chunk.
pub(crate) fn load_gram(bigram: Option<&Hash>, token: u32) -> Option<Gram> {
    let value = bigram?.get(&encode_token_key(token))?;
    match decode_single_gram(&value) {
        Ok((total, records)) => Some(Gram { total, records }),
        Err(_) => tool::abort(),
    }
}

/// `Bigram::store` of `token`'s row. The pin's `m_db->put` answer is
/// ignored by `gen_ngram`/`gen_deleted_ngram`.
pub(crate) fn store_gram(bigram: Option<&Hash>, token: u32, gram: &Gram) {
    if let Some(bigram) = bigram {
        let _ = bigram.put(&encode_token_key(token), &gram.encode());
    }
}

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
/// `error: unknown token:%d.` line) the `strcmp` reads the NULL pointer
/// (class (b)). oxpinyin writes the same line and exits 1 instead, the
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
    let Some(first) = parts
        .first()
        .and_then(|bytes| CString::new(bytes.clone()).ok())
    else {
        tool::abort();
    };
    let Some(phrase) = parts
        .get(1)
        .and_then(|bytes| CString::new(bytes.clone()).ok())
    else {
        tool::abort();
    };
    let token = tool::atoi(&first) as u32;
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

/// `load table.conf failed.\n` and `exit(ENOENT)` after a refused
/// `SystemTableInfo2::load` (`gen_unigram.cpp:96-101`,
/// `gen_ngram.cpp:58-64`, `gen_deleted_ngram.cpp:59-65`,
/// `estimate_interpolation.cpp:91-97`, `eval_correction_rate.cpp:135-141`).
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

/// The `CString` a parsed `G_OPTION_ARG_FILENAME` holds, or `None` where
/// the value holds a NUL or another kind.
pub(crate) fn filename_value(parsed: &ParsedOptions, index: usize) -> Option<CString> {
    match parsed.values.get(index) {
        Some(OptionValue::Filename(Some(bytes))) => CString::new(bytes.clone()).ok(),
        _ => None,
    }
}

/// A C string's bytes as the path the store opens. On Unix the bytes are
/// the path; elsewhere the C runtime's narrow-name conversion is not
/// reproduced (a name that is not UTF-8 does not open).
pub(crate) fn path_of(name: &CStr) -> std::path::PathBuf {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt as _;
        std::ffi::OsStr::from_bytes(name.to_bytes()).into()
    }
    #[cfg(not(unix))]
    {
        name.to_string_lossy().into_owned().into()
    }
}
