//! C ABI subset of libpinyin's public API (52 live symbols).
//!
//! Every `#[unsafe(no_mangle)] pub extern "C" fn` matches the signature in
//! `libpinyin/src/pinyin.h` (tag 2.11.91) symbol-for-symbol. The surface is
//! the fork's 51-symbol W8 bootstrap call set — the 50 pinned ibus-libpinyin
//! 1.16.5 symbols plus `pinyin_get_parsed_input_length` — and
//! `pinyin_clear_constraint`, a libpinyin ABI symbol that belongs in
//! oxpinyin's capi and never shimmed in a frontend.
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
// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![allow(unsafe_code)]
// The entire crate is a pointer-taking C ABI: soundness of these entry
// points rests on the documented pinyin.h contract (opaque handles,
// out-params, ownership), not on Rust-side unsafe marking. Exposing the
// fuzz_api facade makes that pointer-by-contract style lint-visible, so
// the deviation is recorded here once instead of per function.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![warn(missing_docs)]

mod ffi;

/// Rust-visible re-exports for the in-tree fuzz harness
/// (`fuzz/fuzz_targets/capi_commands.rs`) and Rust-side contract tests.
/// NOT a stable Rust API: the supported surfaces are the C ABI (pinyin.h)
/// and `oxpinyin-engine`; anything here can change or vanish.
#[allow(missing_docs)]
pub mod fuzz_api {
    pub use crate::candidates::{
        pinyin_choose_candidate, pinyin_clear_constraint, pinyin_get_candidate,
        pinyin_get_candidate_string, pinyin_get_n_candidate, pinyin_train,
    };
    pub use crate::config::pinyin_set_double_pinyin_scheme;
    pub use crate::context::{oxpinyin_init_for_fixtures, pinyin_fini};
    pub use crate::instance::{pinyin_alloc_instance, pinyin_free_instance, pinyin_reset};
    pub use crate::iterators::{
        pinyin_begin_add_phrases, pinyin_begin_get_phrases, pinyin_end_add_phrases,
        pinyin_end_get_phrases, pinyin_iterator_add_phrase, pinyin_iterator_get_next_phrase,
        pinyin_iterator_has_next_phrase,
    };
    pub use crate::parse::{pinyin_get_parsed_input_length, pinyin_parse_more_full_pinyins};
    pub use crate::sentence::{pinyin_get_sentence, pinyin_guess_sentence};
    pub use crate::types::{
        ExportIterator, GChar, ImportIterator, LookupCandidate, PinyinContext, PinyinInstance,
    };
}
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
pub use types::{GChar, LookupCandidate, PinyinInstance, lookup_candidate_type_t};
pub use types::{ImportIterator, PinyinContext};

use std::os::raw::{c_char, c_int};
use types::{GUint, PinyinOptionT};

// ── candidates ─────────────────────────────────────────────
/// In-process wrapper for the `pinyin_choose_candidate` ABI symbol (see the C header).
pub fn pinyin_choose_candidate(
    instance: *mut PinyinInstance,
    _offset: usize,
    candidate: *mut LookupCandidate,
) -> c_int {
    candidates::pinyin_choose_candidate(instance, _offset, candidate)
}
/// In-process wrapper for the `pinyin_choose_predicted_candidate` ABI symbol (see the C header).
pub fn pinyin_choose_predicted_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    candidates::pinyin_choose_predicted_candidate(instance, candidate)
}
/// In-process wrapper for the `pinyin_clear_constraint` ABI symbol (see the C header).
pub fn pinyin_clear_constraint(instance: *mut PinyinInstance, offset: usize) -> bool {
    candidates::pinyin_clear_constraint(instance, offset)
}
/// In-process wrapper for the `pinyin_get_candidate` ABI symbol (see the C header).
pub fn pinyin_get_candidate(
    instance: *mut PinyinInstance,
    index: GUint,
    candidate: *mut *mut LookupCandidate,
) -> bool {
    candidates::pinyin_get_candidate(instance, index, candidate)
}
/// In-process wrapper for the `pinyin_get_candidate_nbest_index` ABI symbol (see the C header).
pub fn pinyin_get_candidate_nbest_index(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    index: *mut u8,
) -> bool {
    candidates::pinyin_get_candidate_nbest_index(instance, candidate, index)
}
/// In-process wrapper for the `pinyin_get_candidate_string` ABI symbol (see the C header).
pub fn pinyin_get_candidate_string(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    utf8_str: *mut *const GChar,
) -> bool {
    candidates::pinyin_get_candidate_string(instance, candidate, utf8_str)
}
/// In-process wrapper for the `pinyin_get_candidate_type` ABI symbol (see the C header).
pub fn pinyin_get_candidate_type(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
    candidate_type: *mut lookup_candidate_type_t,
) -> bool {
    candidates::pinyin_get_candidate_type(instance, candidate, candidate_type)
}
/// In-process wrapper for the `pinyin_get_n_candidate` ABI symbol (see the C header).
pub fn pinyin_get_n_candidate(instance: *mut PinyinInstance, num: *mut GUint) -> bool {
    candidates::pinyin_get_n_candidate(instance, num)
}
/// In-process wrapper for the `pinyin_is_user_candidate` ABI symbol (see the C header).
pub fn pinyin_is_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    candidates::pinyin_is_user_candidate(instance, candidate)
}
/// In-process wrapper for the `pinyin_remove_user_candidate` ABI symbol (see the C header).
pub fn pinyin_remove_user_candidate(
    instance: *mut PinyinInstance,
    candidate: *mut LookupCandidate,
) -> bool {
    candidates::pinyin_remove_user_candidate(instance, candidate)
}
/// In-process wrapper for the `pinyin_train` ABI symbol (see the C header).
pub fn pinyin_train(instance: *mut PinyinInstance, _index: u8) -> bool {
    candidates::pinyin_train(instance, _index)
}

// ── config ─────────────────────────────────────────────
/// In-process wrapper for the `pinyin_load_addon_phrase_library` ABI symbol (see the C header).
pub fn pinyin_load_addon_phrase_library(context: *mut PinyinContext, index: u8) -> bool {
    config::pinyin_load_addon_phrase_library(context, index)
}
/// In-process wrapper for the `pinyin_mask_out` ABI symbol (see the C header).
pub fn pinyin_mask_out(context: *mut PinyinContext, mask: u32, value: u32) -> bool {
    config::pinyin_mask_out(context, mask, value)
}
/// In-process wrapper for the `pinyin_set_double_pinyin_scheme` ABI symbol (see the C header).
pub fn pinyin_set_double_pinyin_scheme(context: *mut PinyinContext, scheme: c_int) -> bool {
    config::pinyin_set_double_pinyin_scheme(context, scheme)
}
/// In-process wrapper for the `pinyin_set_full_pinyin_scheme` ABI symbol (see the C header).
pub fn pinyin_set_full_pinyin_scheme(context: *mut PinyinContext, scheme: c_int) -> bool {
    config::pinyin_set_full_pinyin_scheme(context, scheme)
}
/// In-process wrapper for the `pinyin_set_options` ABI symbol (see the C header).
pub fn pinyin_set_options(context: *mut PinyinContext, options: PinyinOptionT) -> bool {
    config::pinyin_set_options(context, options)
}
/// In-process wrapper for the `pinyin_set_zhuyin_scheme` ABI symbol (see the C header).
pub fn pinyin_set_zhuyin_scheme(context: *mut PinyinContext, scheme: c_int) -> bool {
    config::pinyin_set_zhuyin_scheme(context, scheme)
}

// ── context ─────────────────────────────────────────────
/// In-process wrapper for the `oxpinyin_init_for_fixtures` ABI symbol (see the C header).
#[must_use]
pub fn oxpinyin_init_for_fixtures(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    context::oxpinyin_init_for_fixtures(systemdir, userdir)
}
/// In-process wrapper for the `oxpinyin_test_set_user_bigram` ABI symbol (see the C header).
pub fn oxpinyin_test_set_user_bigram(
    context: *mut PinyinContext,
    prev: *const c_char,
    cur: *const c_char,
    count: u64,
) -> bool {
    context::oxpinyin_test_set_user_bigram(context, prev, cur, count)
}

// ── instance ─────────────────────────────────────────────
/// In-process wrapper for the `pinyin_alloc_instance` ABI symbol (see the C header).
pub fn pinyin_alloc_instance(context: *mut PinyinContext) -> *mut PinyinInstance {
    instance::pinyin_alloc_instance(context)
}
/// In-process wrapper for the `pinyin_free_instance` ABI symbol (see the C header).
pub fn pinyin_free_instance(instance: *mut PinyinInstance) {
    instance::pinyin_free_instance(instance)
}
/// In-process wrapper for the `pinyin_reset` ABI symbol (see the C header).
pub fn pinyin_reset(instance: *mut PinyinInstance) -> bool {
    instance::pinyin_reset(instance)
}

// ── parse ─────────────────────────────────────────────
/// In-process wrapper for the `pinyin_get_parsed_input_length` ABI symbol (see the C header).
pub fn pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize {
    parse::pinyin_get_parsed_input_length(instance)
}
/// In-process wrapper for the `pinyin_in_chewing_keyboard` ABI symbol (see the C header).
pub fn pinyin_in_chewing_keyboard(
    instance: *mut PinyinInstance,
    key: c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    parse::pinyin_in_chewing_keyboard(instance, key, symbols)
}
/// In-process wrapper for the `pinyin_parse_more_chewings` ABI symbol (see the C header).
pub fn pinyin_parse_more_chewings(instance: *mut PinyinInstance, chewings: *const c_char) -> usize {
    parse::pinyin_parse_more_chewings(instance, chewings)
}
/// In-process wrapper for the `pinyin_parse_more_double_pinyins` ABI symbol (see the C header).
pub fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    parse::pinyin_parse_more_double_pinyins(instance, pinyins)
}
/// In-process wrapper for the `pinyin_parse_more_full_pinyins` ABI symbol (see the C header).
pub fn pinyin_parse_more_full_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    parse::pinyin_parse_more_full_pinyins(instance, pinyins)
}

// ── sentence ─────────────────────────────────────────────
/// In-process wrapper for the `pinyin_get_sentence` ABI symbol (see the C header).
pub fn pinyin_get_sentence(
    instance: *mut PinyinInstance,
    index: u8,
    sentence: *mut *mut c_char,
) -> bool {
    sentence::pinyin_get_sentence(instance, index, sentence)
}
/// In-process wrapper for the `pinyin_guess_candidates` ABI symbol (see the C header).
pub fn pinyin_guess_candidates(
    instance: *mut PinyinInstance,
    offset: usize,
    sort_option: GUint,
) -> bool {
    sentence::pinyin_guess_candidates(instance, offset, sort_option)
}
/// In-process wrapper for the `pinyin_guess_sentence` ABI symbol (see the C header).
pub fn pinyin_guess_sentence(instance: *mut PinyinInstance) -> bool {
    sentence::pinyin_guess_sentence(instance)
}
/// In-process wrapper for the `pinyin_guess_predicted_candidates_with_punctuations` ABI symbol (see the C header).
pub fn pinyin_guess_predicted_candidates_with_punctuations(
    instance: *mut PinyinInstance,
    prefix: *const c_char,
) -> bool {
    sentence::pinyin_guess_predicted_candidates_with_punctuations(instance, prefix)
}

// ── text ─────────────────────────────────────────────
/// In-process wrapper for the `pinyin_get_chewing_auxiliary_text` ABI symbol (see the C header).
pub fn pinyin_get_chewing_auxiliary_text(
    instance: *mut PinyinInstance,
    cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    text::pinyin_get_chewing_auxiliary_text(instance, cursor, aux_text)
}
/// In-process wrapper for the `pinyin_get_double_pinyin_auxiliary_text` ABI symbol (see the C header).
pub fn pinyin_get_double_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    text::pinyin_get_double_pinyin_auxiliary_text(instance, cursor, aux_text)
}
/// In-process wrapper for the `pinyin_get_full_pinyin_auxiliary_text` ABI symbol (see the C header).
pub fn pinyin_get_full_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    text::pinyin_get_full_pinyin_auxiliary_text(instance, cursor, aux_text)
}

// ── user_data ─────────────────────────────────────────────
/// In-process wrapper for the `pinyin_remember_user_input` ABI symbol (see the C header).
pub fn pinyin_remember_user_input(
    instance: *mut PinyinInstance,
    phrase: *const c_char,
    count: c_int,
) -> bool {
    user_data::pinyin_remember_user_input(instance, phrase, count)
}

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

// The ABI-driven black-box suites live in `tests/abi.rs`. These three mods
// are white-box unit tests of the ABI layer: they drive the C symbols for
// setup and act, then assert directly on `CapiInstance` internals through
// `instance_ref`. Moving them under `tests/` would require exporting
// internals purely for test inspection or weakening the assertions, so
// they stay at the unit layer (docs/testing/upstream-test-coverage.md).
#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod guess_offset_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod union_e2e_tests;
