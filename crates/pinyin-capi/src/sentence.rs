//! Sentence guessing and retrieval.

use std::os::raw::c_char;
use std::ptr;

use crate::types::{GUint, PinyinInstance};

/// Guess a sentence from saved pinyin keys.
///
/// # C signature
/// ```c
/// bool pinyin_guess_sentence(pinyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_sentence(instance: *mut PinyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }
    // STUB: T3 will implement.
    false
}

/// Guess predicted candidates with punctuations after a prefix.
///
/// # C signature
/// ```c
/// bool pinyin_guess_predicted_candidates_with_punctuations(
///     pinyin_instance_t * instance, const char * prefix);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_predicted_candidates_with_punctuations(
    instance: *mut PinyinInstance,
    _prefix: *const c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }
    // STUB: T3 will implement.
    false
}

/// Get a sentence string from the instance (n-best variant).
///
/// # C signature
/// ```c
/// bool pinyin_get_sentence(pinyin_instance_t * instance,
///                          guint8 index,
///                          char ** sentence);
/// ```
///
/// Out-param `sentence` is caller-owned (`g_free`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_sentence(
    instance: *mut PinyinInstance,
    _index: u8,
    sentence: *mut *mut c_char,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !sentence.is_null() {
        // SAFETY: Null-checked above. Write NULL to indicate no result.
        unsafe {
            *sentence = ptr::null_mut();
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get character offset from a lookup byte offset within a sentence.
///
/// # C signature
/// ```c
/// bool pinyin_get_character_offset(pinyin_instance_t * instance,
///                                  const char * phrase,
///                                  size_t offset,
///                                  size_t * length);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_character_offset(
    instance: *mut PinyinInstance,
    _phrase: *const c_char,
    _offset: usize,
    length: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !length.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *length = 0;
        }
    }
    // STUB: T3 will implement.
    false
}

/// Guess candidates at the given offset with sort option.
///
/// # C signature
/// ```c
/// bool pinyin_guess_candidates(pinyin_instance_t * instance,
///                              size_t offset,
///                              guint sort_option);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_guess_candidates(
    instance: *mut PinyinInstance,
    _offset: usize,
    _sort_option: GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }
    // STUB: T3 will implement.
    false
}
