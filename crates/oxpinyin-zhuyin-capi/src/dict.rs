//! The dictionary-introspection surface: token lookups, per-token reads,
//! the unigram-frequency write.

use std::os::raw::{c_char, c_uint, c_void};
use std::ptr;

use oxpinyin_core::Dictionary;

use crate::ffi::{cstr_to_string, ffi_catch, owned_cstr};
use crate::state::instance_ref;
use crate::types::{GArray, GChar, GUint, PhraseTokenT, ZhuyinInstance};

// glib append/truncate, the same entry points oxpinyin-capi uses.
unsafe extern "C" {
    fn g_array_append_vals(array: *mut GArray, data: *const c_void, len: c_uint) -> *mut GArray;
    fn g_array_set_size(array: *mut GArray, length: c_uint) -> *mut GArray;
}

/// Look up the phrase tokens stored for an exact phrase string.
///
/// # C signature
/// ```c
/// bool zhuyin_lookup_tokens(zhuyin_instance_t * instance,
///                           const char * phrase, GArray * tokenarray);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_lookup_tokens(
    instance: *mut ZhuyinInstance,
    phrase: *const c_char,
    tokenarray: *mut GArray,
) -> bool {
    if instance.is_null() || phrase.is_null() {
        return false;
    }
    if tokenarray.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // SAFETY: Null-checked above.
        let text = unsafe { cstr_to_string(phrase) };
        let tokens: Vec<u32> = inst
            .dict
            .tokens_for_text(&text)
            .iter()
            .map(|token| token.value())
            .collect();
        // SAFETY: Null-checked above.
        unsafe {
            g_array_set_size(tokenarray, 0);
        }
        if tokens.is_empty() {
            return false;
        }
        // SAFETY: Null-checked above.
        unsafe {
            g_array_append_vals(
                tokenarray,
                tokens.as_ptr().cast::<c_void>(),
                tokens.len() as c_uint,
            );
        }
        true
    })
}

/// Get the phrase text of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_phrase(zhuyin_instance_t * instance,
///                              phrase_token_t token, guint * len,
///                              gchar ** utf8_str);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_phrase(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    len: *mut GUint,
    utf8_str: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(intro) = inst.dict.token_introspection(token) else {
            if !utf8_str.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *utf8_str = ptr::null_mut();
                }
            }
            return false;
        };
        if !len.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *len = intro.text.chars().count() as GUint;
            }
        }
        if !utf8_str.is_null() {
            let rendered = owned_cstr(&intro.text);
            // SAFETY: Null-checked above.
            unsafe {
                *utf8_str = rendered;
            }
            if rendered.is_null() {
                return false;
            }
        }
        true
    })
}

/// Get the number of pronunciations of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_n_pronunciation(zhuyin_instance_t * instance,
///                                       phrase_token_t token, guint * num);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_n_pronunciation(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    num: *mut GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        if !num.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *num = 0;
            }
        }
        let Some(intro) = inst.dict.token_introspection(token) else {
            return false;
        };
        if !num.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *num = intro.pronunciations.len() as GUint;
            }
        }
        true
    })
}

/// Get the nth pronunciation of a token as a vector of chewing keys.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_nth_pronunciation(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint nth,
///                                         ChewingKeyVector keys);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_nth_pronunciation(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    nth: GUint,
    keys: *mut GArray,
) -> bool {
    if instance.is_null() || keys.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // The pin clears the caller's array before appending
        // (`zhuyin.cpp:1793` `g_array_set_size(keys, 0)`), so a stale or
        // re-used GArray never shows concatenated results on either path.
        // SAFETY: Null-checked above; `g_array_set_size` on a real glib
        // GArray updates `len` and preserves its private metadata.
        unsafe {
            g_array_set_size(keys, 0);
        }
        let Some(intro) = inst.dict.token_introspection(token) else {
            return false;
        };
        let Some((keys_list, _count)) = intro.pronunciations.get(nth as usize) else {
            return false;
        };
        let mut packed: Vec<u16> = Vec::with_capacity(keys_list.len());
        for &key in keys_list {
            let Some(syllable) = oxpinyin_core::SyllableKey::from_index(key as usize) else {
                return false;
            };
            let Some(chewing) = oxpinyin_core::ChewingKey::from_pinyin(syllable.text()) else {
                return false;
            };
            packed.push(chewing.to_packed());
        }
        if packed.is_empty() {
            return false;
        }
        // SAFETY: Null-checked above.
        unsafe {
            g_array_append_vals(
                keys,
                packed.as_ptr().cast::<c_void>(),
                packed.len() as c_uint,
            );
        }
        true
    })
}

/// Get the unigram frequency of a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_get_unigram_frequency(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint * freq);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_get_unigram_frequency(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    freq: *mut GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(count) = inst.dict.system_unigram_count(token) else {
            return false;
        };
        if !freq.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *freq = GUint::try_from(count + 1).unwrap_or(GUint::MAX);
            }
        }
        true
    })
}

/// Add a unigram-frequency delta to a token.
///
/// # C signature
/// ```c
/// bool zhuyin_token_add_unigram_frequency(zhuyin_instance_t * instance,
///                                         phrase_token_t token, guint delta);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_token_add_unigram_frequency(
    instance: *mut ZhuyinInstance,
    token: PhraseTokenT,
    delta: GUint,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.dict.add_unigram_delta(token, delta as u64)
    })
}
