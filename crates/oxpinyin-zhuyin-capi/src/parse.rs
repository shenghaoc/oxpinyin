//! Parsing symbols: full pinyin, chewing (bopomofo), and the shared
//! batch-parse shell.

use std::os::raw::c_char;

use crate::ffi::cstr_to_string;
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, ZhuyinInstance};

fn parse_chewing_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // The parse path clears the candidate snapshot before anything else —
    // main's `begin_parse` did this through `reset_parse_state`; the core
    // seam cannot see this layer's snapshot, so the clear lives here.
    inst.candidates.clear();
    inst.core
        .parse_chewing_more(text, oxpinyin_facade::ToneForwarding::ZhuyinFacade)
}

fn parse_full_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    // Parse-path snapshot clear (main's begin_parse law).
    inst.candidates.clear();
    inst.core.parse_full_more(text)
}

/// Parse multiple full pinyins.
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_full_pinyins(zhuyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Returns number of bytes consumed, 0 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_more_full_pinyins(
    instance: *mut ZhuyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `pinyins` is a C string from the caller (null OK).
    let text = unsafe { cstr_to_string(pinyins) };
    parse_full_more(instance, &text)
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_chewings(zhuyin_instance_t * instance,
///                                   const char * chewings);
/// ```
///
/// Parses through [`oxpinyin_core::ZhuyinParser`] and drives the session
/// with the apostrophe-joined full-pinyin spelling.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_more_chewings(
    instance: *mut ZhuyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `chewings` is a C string from the caller (null OK).
    let text = unsafe { cstr_to_string(chewings) };
    parse_chewing_more(instance, &text)
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t zhuyin_get_parsed_input_length(zhuyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_parsed_input_length(instance: *mut ZhuyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    inst.core.parsed_len
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_in_chewing_keyboard(zhuyin_instance_t * instance,
///                                 const char key,
///                                 gchar *** symbols);
/// ```
///
/// `key` is a plain `char` value (not a pointer).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_in_chewing_keyboard(
    instance: *mut ZhuyinInstance,
    key: std::os::raw::c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // `c_char` is `i8` on some targets and `u8` on others (aarch64
    // Linux among them); `as u8` is a lossless reinterpret on both,
    // and the cast is not "unnecessary" on the targets where it is
    // `i8`.
    #[allow(clippy::unnecessary_cast)]
    let mapped = inst.core.in_keyboard(key as u8);
    if mapped.is_empty() {
        if !symbols.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *symbols = std::ptr::null_mut();
            }
        }
        return false;
    }
    if !symbols.is_null() {
        let list = crate::ffi::owned_cstr_list(&mapped);
        if list.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *symbols = std::ptr::null_mut();
            }
            return false;
        }
        // SAFETY: `owned_cstr_list` is a malloc array of malloc strings;
        // the caller releases both with g_strfreev.
        unsafe {
            *symbols = list;
        }
    }
    true
}
