//! The phrase-result surface: `pinyin_phrase_segment` and the two
//! getters over `m_phrase_result` (`pinyin.cpp:3269-3287`).
//!
//! The segmenter is the engine's span DP (`oxpinyin_engine::Session::
//! phrase_segment`, the `PhraseLookup::get_best_match` port); this
//! module only stores its result on the instance and hands out reads.
//! The result shape is the pin's: a character-length array with each
//! phrase's token at its span's start position and `null_token` between
//! phrases — and, on a failed match, the fully sized all-null array
//! (`PhraseLookup::final_step` sizes and null-fills before its
//! empty-last-step `false`, `phrase_lookup.cpp:382-428`). `pinyin_reset`
//! clears the array (`pinyin.cpp:2699`); nothing else removes from it.

use std::os::raw::c_char;

use crate::ffi::cstr_to_strict;
use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, PinyinInstance};

/// Segment an arbitrary sentence string into phrase tokens.
///
/// # C signature
/// ```c
/// bool pinyin_phrase_segment(pinyin_instance_t * instance,
///                            const char * sentence);
/// ```
///
/// Upstream gates on UTF-8 validity (`g_return_val_if_fail(num_of_chars
/// == ucs4_len, FALSE)`, `pinyin.cpp:1450`) and stores the best phrase
/// path into the instance's result array — including on a failed match,
/// where the array stays fully sized and all-null.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_phrase_segment(
    instance: *mut PinyinInstance,
    sentence: *const c_char,
) -> bool {
    if instance.is_null() || sentence.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // SAFETY: Null-checked above. Invalid UTF-8 refuses here, the
    // pin's `g_return_val_if_fail` gate; the lossy conversion the
    // parse paths use would paper over exactly what upstream rejects.
    // `cstr_to_strict` reads the bytes without an unsafe block; the
    // safety obligation lives in its own doc comment.
    let Some(text) = cstr_to_strict(sentence) else {
        return false;
    };
    match inst.core.session.phrase_segment(&text) {
        Ok((matched, tokens)) => {
            inst.core.phrase_result = tokens;
            matched
        }
        Err(_) => false,
    }
}

/// Get the number of phrase tokens in the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_n_phrase(pinyin_instance_t * instance, guint * num);
/// ```
///
/// Upstream answers `true` unconditionally over whatever
/// `pinyin_phrase_segment` last stored (`pinyin.cpp:3269-3274`); without
/// a prior segment call the count is 0.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_n_phrase(instance: *mut PinyinInstance, num: *mut GUint) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    let count = inst.core.phrase_result.len();
    if !num.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *num = count as GUint;
        }
    }
    true
}

/// Get the phrase token at an index of the phrase result.
///
/// # C signature
/// ```c
/// bool pinyin_get_phrase_token(pinyin_instance_t * instance,
///                              guint index,
///                              phrase_token_t * token);
/// ```
///
/// The out-param is zeroed before the bounds check — a `false` still
/// delivers `null_token` (`pinyin.cpp:3276-3287`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_phrase_token(
    instance: *mut PinyinInstance,
    index: GUint,
    token: *mut PhraseTokenT,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    if !token.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *token = crate::types::null_token;
        }
    }
    if index as usize >= inst.core.phrase_result.len() {
        return false;
    }
    if !token.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *token = inst.core.phrase_result[index as usize].value();
        }
    }
    true
}
