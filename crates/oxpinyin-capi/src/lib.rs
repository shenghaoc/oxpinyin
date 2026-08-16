//! C ABI subset of libpinyin's public API (51 live symbols).
//!
//! Every `#[unsafe(no_mangle)] pub extern "C" fn` matches the signature in
//! `libpinyin/src/pinyin.h` (tag 2.11.91) symbol-for-symbol. The surface is
//! the fork's 51-symbol W8 bootstrap call set: the 50 pinned ibus-libpinyin
//! 1.16.5 symbols plus `pinyin_get_parsed_input_length`.
//!
//! ## Panic discipline
//!
//! An unwind across `extern "C"` is undefined behaviour. Every entry
//! point wraps its body in [`ffi::ffi_catch`] which calls
//! [`std::panic::catch_unwind`], returning the sentinel value (false /
//! null / 0) on panic. The engine layer returns `Result` everywhere and
//! should never panic, but `catch_unwind` is the belt-and-suspenders
//! safety net at the ABI boundary.
//!
//! Opaque handles cross the boundary as `*mut T` via `Box::into_raw` /
//! `Box::from_raw`. Every incoming pointer is null-checked before deref.
//! `// SAFETY:` documents each `unsafe` block.
#![allow(unsafe_code)]
#![warn(missing_docs)]

mod ffi;
mod state;
mod types;

mod candidates;
mod config;
mod context;
mod cursor;
mod instance;
mod iterators;
mod parse;
mod predict;
mod sentence;
mod text;
mod user_data;

use oxpinyin_core::graph::FewestKeys;

pub use context::{pinyin_fini, pinyin_init, pinyin_save};
pub use iterators::{pinyin_begin_add_phrases, pinyin_end_add_phrases, pinyin_iterator_add_phrase};
pub use oxpinyin_user::{
    DEFAULT_PHRASE_COUNT, ExportedPhrase, NETWORK_DICTIONARY, USER_DICTIONARY,
};
pub use state::ExportedBigramRow;
pub use types::{ImportIterator, PinyinContext};

/// One successful import-pinyin parse.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportPinyin {
    /// Complete keys the ABI would write.
    pub key_count: usize,
    /// `'`-joined syllable spellings, matching [`ExportedPhrase::pinyin`].
    pub canonical: String,
}

/// Open a user-store-only [`PinyinContext`] for standalone migration tools.
///
/// This is the Rust-side constructor behind `oxpinyin-dictool import`: it owns
/// exactly the state the import/export/save trio needs and no decode model,
/// so a vocabulary conversion tool can run without system tables installed.
/// The C ABI `pinyin_init` contract is unchanged — it still requires a
/// system directory. Release the returned handle with [`pinyin_fini`].
///
/// Returns null for an empty or unopenable `user_dir`.
#[must_use]
pub fn open_user_import_context(user_dir: &std::path::Path) -> *mut PinyinContext {
    let Some(user_dir) = user_dir.to_str() else {
        return std::ptr::null_mut();
    };
    ffi::ffi_catch(
        std::ptr::null_mut(),
        || match state::CapiContext::new_user_only(user_dir) {
            Some(ctx) => state::box_context(ctx),
            None => std::ptr::null_mut(),
        },
    )
}

/// Snapshot the user-store phrase rows a migration tool needs for its
/// desired-count import math.
///
/// This drives the W6-T7 C ABI export iterator
/// (`pinyin_begin_get_phrases` / `pinyin_iterator_has_next_phrase` /
/// `pinyin_iterator_get_next_phrase` / `pinyin_end_get_phrases`) and returns
/// the same §9 rows. `None` for a null context, a context without a user
/// store, or an iterator failure.
#[must_use]
pub fn user_phrase_rows(context: *mut PinyinContext) -> Option<Vec<ExportedPhrase>> {
    let iter = iterators::pinyin_begin_get_phrases(context, u32::from(USER_DICTIONARY));
    if iter.is_null() {
        return None;
    }
    let mut rows = Vec::new();
    while iterators::pinyin_iterator_has_next_phrase(iter) {
        let mut phrase = std::ptr::null_mut();
        let mut pinyin = std::ptr::null_mut();
        let mut count = 0;
        if !iterators::pinyin_iterator_get_next_phrase(iter, &mut phrase, &mut pinyin, &mut count) {
            iterators::pinyin_end_get_phrases(iter);
            return None;
        }
        if phrase.is_null() || pinyin.is_null() {
            let _ = ffi::take_owned_cstr(phrase);
            let _ = ffi::take_owned_cstr(pinyin);
            iterators::pinyin_end_get_phrases(iter);
            return None;
        }
        rows.push(ExportedPhrase {
            text: ffi::take_owned_cstr(phrase),
            pinyin: ffi::take_owned_cstr(pinyin),
            count: count.max(0) as u64,
        });
    }
    iterators::pinyin_end_get_phrases(iter);
    Some(rows)
}

/// Snapshot the rendered user-bigram rows the frontend Export button writes.
///
/// Drives the W6-T7 C ABI bigram export quartet and returns the same rows
/// `pinyin_begin_get_bigram_phrases` materializes. `None` mirrors
/// [`user_phrase_rows`].
#[must_use]
pub fn user_bigram_rows(context: *mut PinyinContext) -> Option<Vec<ExportedBigramRow>> {
    let iter = iterators::pinyin_begin_get_bigram_phrases(context);
    if iter.is_null() {
        return None;
    }
    let mut rows = Vec::new();
    while iterators::pinyin_bigram_iterator_has_next_phrase(iter) {
        let mut phrase = std::ptr::null_mut();
        let mut pinyin = std::ptr::null_mut();
        let mut count = 0;
        if !iterators::pinyin_bigram_iterator_get_next_phrase(
            iter,
            &mut phrase,
            &mut pinyin,
            &mut count,
        ) {
            iterators::pinyin_end_get_bigram_phrases(iter);
            return None;
        }
        if phrase.is_null() || pinyin.is_null() {
            let _ = ffi::take_owned_cstr(phrase);
            let _ = ffi::take_owned_cstr(pinyin);
            iterators::pinyin_end_get_bigram_phrases(iter);
            return None;
        }
        rows.push(ExportedBigramRow {
            phrase: ffi::take_owned_cstr(phrase),
            pinyin: ffi::take_owned_cstr(pinyin),
            count: i64::from(count),
        });
    }
    iterators::pinyin_end_get_bigram_phrases(iter);
    Some(rows)
}

/// Parse `pinyin` the way `pinyin_iterator_add_phrase` does.
///
/// Longest parsed prefix, complete keys only, trailing unparsed bytes
/// ignored. `None` when no path can be built. `canonical` is the §9
/// `'`-joined spelling, so a file line `nihao` and a stored row `ni'hao`
/// compare as the same pronunciation.
#[must_use]
pub fn import_pinyin(pinyin: &str) -> Option<ImportPinyin> {
    let parsed = FewestKeys::parse(pinyin)?;
    Some(ImportPinyin {
        key_count: parsed.keys().len(),
        canonical: parsed.canonical(),
    })
}

/// Number of keys [`import_pinyin`] parses out of `pinyin`.
#[must_use]
pub fn import_pinyin_key_count(pinyin: &str) -> Option<usize> {
    import_pinyin(pinyin).map(|parsed| parsed.key_count)
}

/// Begin a [`USER_DICTIONARY`] import batch on a user-import context.
///
/// Pair with [`end_user_import`].
#[must_use]
pub fn begin_user_import(context: *mut PinyinContext) -> *mut ImportIterator {
    iterators::pinyin_begin_add_phrases(context, USER_DICTIONARY)
}

/// Add one phrase/reading to the batch started by [`begin_user_import`].
///
/// `count` follows the ABI: `-1` means the pinned default count. Returns
/// `false` for a null iterator, a NUL-containing string field, an index other
/// than [`USER_DICTIONARY`], an unparseable pinyin, or a store failure.
pub fn add_user_import_phrase(
    iter: *mut ImportIterator,
    phrase: &str,
    pinyin: &str,
    count: std::os::raw::c_int,
) -> bool {
    if iter.is_null() {
        return false;
    }
    let Ok(phrase) = std::ffi::CString::new(phrase) else {
        return false;
    };
    let Ok(pinyin) = std::ffi::CString::new(pinyin) else {
        return false;
    };
    iterators::pinyin_iterator_add_phrase(iter, phrase.as_ptr(), pinyin.as_ptr(), count)
}

/// End the batch, arm `m_modified`, and release the iterator handle.
pub fn end_user_import(iter: *mut ImportIterator) {
    iterators::pinyin_end_add_phrases(iter);
}

/// `pinyin_save` for a user-import context.
///
/// Returns `false` for a null context, an absent user store, an unmodified
/// store, or a compaction failure.
pub fn save_user_import_context(context: *mut PinyinContext) -> bool {
    context::pinyin_save(context)
}

/// `pinyin_fini` for a context returned by [`open_user_import_context`].
pub fn close_user_import_context(context: *mut PinyinContext) {
    context::pinyin_fini(context);
}

#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod test_support;
