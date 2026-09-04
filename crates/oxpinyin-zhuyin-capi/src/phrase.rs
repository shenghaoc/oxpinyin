//! The phrase-result surface: `zhuyin_phrase_segment` and the two getters
//! over `m_phrase_result`.

use std::os::raw::c_char;

use crate::ffi::{cstr_to_strict, ffi_catch};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GUint, PhraseTokenT, ZhuyinInstance};

/// Segment an arbitrary sentence string into phrase tokens.
///
/// # C signature
/// ```c
/// bool zhuyin_phrase_segment(zhuyin_instance_t * instance,
///                            const char * sentence);
/// ```
///
/// Upstream gates on UTF-8 validity (`g_return_val_if_fail`, `zhuyin.cpp:965`)
/// and stores the best phrase path into the instance's result array.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_phrase_segment(
    instance: *mut ZhuyinInstance,
    sentence: *const c_char,
) -> bool {
    if instance.is_null() || sentence.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
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
    })
}

/// Get the number of phrase tokens in the phrase result.
///
/// # C signature
/// ```c
/// bool zhuyin_get_n_phrase(zhuyin_instance_t * instance, guint * num);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_n_phrase(instance: *mut ZhuyinInstance, num: *mut GUint) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let count = inst.core.phrase_result.len();
        if !num.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *num = count as GUint;
            }
        }
        true
    })
}

/// Get the phrase token at an index of the phrase result.
///
/// # C signature
/// ```c
/// bool zhuyin_get_phrase_token(zhuyin_instance_t * instance,
///                              guint index, phrase_token_t * token);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_phrase_token(
    instance: *mut ZhuyinInstance,
    index: GUint,
    token: *mut PhraseTokenT,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
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
    })
}
