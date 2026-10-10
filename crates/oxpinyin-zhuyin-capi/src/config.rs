//! Configuration symbols: options, schemes, phrase library loading.

use std::os::raw::c_int;
use std::sync::atomic::Ordering;

use crate::state::{context_mut, context_ref};
use crate::types::{PinyinOptionT, ZhuyinContext};

/// `PHRASE_INDEX_LIBRARY_COUNT` (`phrase_index.h`): sixteen sub-indices.
const PHRASE_INDEX_LIBRARY_COUNT: u8 = 16;

/// `TSI_DICTIONARY` (`novel_types.h`): the one default library that stays.
const TSI_DICTIONARY: u8 = 1;

/// Set the zhuyin scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_set_chewing_scheme(zhuyin_context_t * context,
///                                ZhuyinScheme scheme);
/// ```
///
/// The Rust parameter is `c_int`: callers may pass any `int`.
/// Every implemented Zhuyin keyboard is table-driven; the `ZHUYIN_STANDARD_DVORAK`
/// (7) upstream abort slot, like every out-of-enum value, reports `false`
/// and one warning instead of aborting (no-abort policy, divergence class
/// (c)).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_chewing_scheme(context: *mut ZhuyinContext, scheme: c_int) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1 | 2 | 3 | 4 | 5 | 6 | 8 | 9) {
        // Class (c): the dvorak slot (7) falls through to
        // `zhuyin_parser2.cpp:295`, every out-of-enum value to
        // `zhuyin.cpp:736`, both **`abort()`**.
        crate::ffi::log_warning(&format!(
            "zhuyin_set_chewing_scheme: scheme {scheme} aborts the parser table"
        ));
        return false;
    }
    ctx.core.live.zhuyin_scheme.store(scheme, Ordering::Relaxed);
    true
}

/// Set the full pinyin scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_set_full_pinyin_scheme(zhuyin_context_t * context,
///                                    FullPinyinScheme scheme);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_full_pinyin_scheme(
    context: *mut ZhuyinContext,
    scheme: c_int,
) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    if !matches!(scheme, 1..=3) {
        // Class (c), `pinyin_parser2.cpp:398`: **`abort()`** in
        // `FullPinyinParser2::set_scheme`.
        crate::ffi::log_warning(&format!(
            "zhuyin_set_full_pinyin_scheme: scheme {scheme} aborts the parser table"
        ));
        return false;
    }
    ctx.core.live.full_scheme.store(scheme, Ordering::Relaxed);
    true
}

/// `zhuyin_set_options` — copies the caller's option word.
///
/// # C signature
/// ```c
/// bool zhuyin_set_options(zhuyin_context_t * context,
///                         pinyin_option_t options);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_set_options(context: *mut ZhuyinContext, options: PinyinOptionT) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_mut(context) };
    // The shared set_options law (word, mirrored bools, config key);
    // the pinyin facade's setter runs the same body.
    ctx.core.set_options(options);
    true
}

/// `zhuyin_mask_out`.
///
/// # C signature
/// ```c
/// bool zhuyin_mask_out(zhuyin_context_t * context,
///                      phrase_token_t mask,
///                      phrase_token_t value);
/// ```
///
/// The user table's `mask_out` walks every `user_pinyin_index.bin` record
/// and dies of SIGABRT at the `switch` whose `default` is `abort()` when a
/// key is past `MAX_PHRASE_LENGTH` syllables
/// (`chewing_large_table2_bdb.cpp:529`); this entry answers `false` and one
/// `libzhuyin` warning for that shape (class (c)).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_mask_out(context: *mut ZhuyinContext, mask: u32, value: u32) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`;
    // the unique borrow lasts only for the mask call.
    let ctx = unsafe { context_mut(context) };
    match ctx.mask_out(mask, value) {
        oxpinyin_facade::MaskOutOutcome::Done(answer) => answer,
        oxpinyin_facade::MaskOutOutcome::OverlongIndexKey => {
            // Class (c), `chewing_large_table2_bdb.cpp:529`: the user
            // pinyin index carries a key past MAX_PHRASE_LENGTH syllables
            // and the pin's `mask_out` switch falls to `default: abort()`.
            crate::ffi::log_warning(
                "zhuyin_mask_out: a user pinyin index key is longer than MAX_PHRASE_LENGTH \
                 syllables (upstream aborts, chewing_large_table2_bdb.cpp:529)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::NonTokenUserBigramKey => {
            // Class (c), `ngram_bdb.cpp:199`: the user bigram carries a key
            // that is not a phrase_token_t and the pin's `get_all_items`
            // assert dies.
            crate::ffi::log_warning(
                "zhuyin_mask_out: a user bigram key is not a phrase_token_t \
                 (upstream aborts, ngram_bdb.cpp:199)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::ShortUserBigramValue => {
            // Class (c), `memory_chunk.h:390`: the user bigram carries a
            // value shorter than a guint32 total_freq and the pin's
            // `get_total_freq` read asserts.
            crate::ffi::log_warning(
                "zhuyin_mask_out: a user bigram value is shorter than a guint32 total_freq \
                 (upstream aborts, memory_chunk.h:390)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::ResidualUserBigramGram => {
            // Class (c), `ngram.cpp:70`: masking a user bigram removed
            // every item of a gram whose total its items did not cover,
            // and the pin's `get_length` assert dies
            // (`Bigram::mask_out`, `ngram_bdb.cpp:243`).
            crate::ffi::log_warning(
                "zhuyin_mask_out: masking a user bigram leaves a residual total_freq \
                 (upstream aborts, ngram.cpp:70)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::SystemLogHeaderToken => {
            // Class (c), `phrase_index_logger.h:202`: a system library's
            // user .dbin carries a non-null-token MODIFY_HEADER and the
            // pin's `next_record` assert dies.
            crate::ffi::log_warning(
                "zhuyin_mask_out: a system library's user .dbin carries a non-null-token \
                 MODIFY_HEADER (upstream asserts, phrase_index_logger.h:202)",
            );
            false
        }
        oxpinyin_facade::MaskOutOutcome::SystemLogMultipleHeaders => {
            // Class (c), `phrase_index.cpp:745`: a system library's user
            // .dbin carries more than one MODIFY_HEADER and the pin's
            // `_peek_header` assert dies.
            crate::ffi::log_warning(
                "zhuyin_mask_out: a system library's user .dbin carries more than one \
                 MODIFY_HEADER (upstream asserts, phrase_index.cpp:745)",
            );
            false
        }
    }
}

/// Load a default phrase library by index.
///
/// # C signature
/// ```c
/// bool zhuyin_load_phrase_library(zhuyin_context_t * context,
///                                 guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_load_phrase_library(context: *mut ZhuyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_ref(context) };
    // Class (c), `zhuyin.cpp:372`: the pin **`assert`**s that the row of a
    // loaded library is a `SYSTEM_FILE` or `USER_FILE` (an index of 16 or
    // more answers `false` first, `:362`); which rows are is the
    // context's `table.conf`.
    match ctx.load_phrase_library(u32::from(index)) {
        Ok(loaded) => loaded,
        Err(_) => {
            crate::ffi::log_warning(
                "zhuyin_load_phrase_library: assertion 'SYSTEM_FILE == table_info->m_file_type \
                 || USER_FILE == table_info->m_file_type' failed",
            );
            false
        }
    }
}

/// Unload a default phrase library by index.
///
/// # C signature
/// ```c
/// bool zhuyin_unload_phrase_library(zhuyin_context_t * context,
///                                   guint8 index);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_unload_phrase_library(context: *mut ZhuyinContext, index: u8) -> bool {
    if context.is_null() {
        return false;
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_ref(context) };
    if index >= PHRASE_INDEX_LIBRARY_COUNT {
        // Class (c), `zhuyin.cpp:381`: **`assert`**.
        crate::ffi::log_warning(
            "zhuyin_unload_phrase_library: assertion 'index < PHRASE_INDEX_LIBRARY_COUNT' failed",
        );
        return false;
    }
    // `tsi.bin` can't be unloaded (`zhuyin.cpp:383-385`); any other library
    // is, and the pin ignores what `unload` returns.
    if index == TSI_DICTIONARY {
        return false;
    }
    ctx.unload_phrase_library(index)
}
