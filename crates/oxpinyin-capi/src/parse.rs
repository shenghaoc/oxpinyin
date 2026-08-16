//! Parsing symbols: full pinyin, double pinyin, chewing.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, PinyinInstance};

/// Shared batch-parse path: reset the instance, type `text`, store the
/// parsed prefix length, and return it.
///
/// The getter must return this snapshot, not a length recomputed from the
/// current session: the fork compares `pinyin_choose_candidate`'s cursor
/// against it (`docs/findings/abi-subset.md` W8 contract).
///
/// The stored length is the session's filtered `fewest_keys` prefix, not
/// the unfiltered graph `consumed()`.
///
/// Resets and clears the candidate snapshot even for empty input, so a
/// prior composition is discarded and the stored length returns to 0.
fn parse_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.reset_parse_state();
    if text.is_empty() {
        return 0;
    }
    let consumed = match inst.session.type_pinyin(text) {
        Ok(_) => inst.session.parsed_prefix_len(),
        Err(_) => 0,
    };
    inst.parsed_len = consumed;
    consumed
}

fn parse_c_string(instance: *mut PinyinInstance, text: *const c_char) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `text` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(text) };
        parse_more(instance, &text)
    })
}

/// Parse multiple full pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_full_pinyins(pinyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Returns number of bytes consumed, 0 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_full_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    parse_c_string(instance, pinyins)
}

/// Parse multiple double pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_double_pinyins(pinyin_instance_t * instance,
///                                         const char * pinyins);
/// ```
///
/// Provisional: routes through the same full-pinyin parse path until
/// the engine gains a dedicated double-pinyin parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    parse_c_string(instance, pinyins)
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_chewings(pinyin_instance_t * instance,
///                                    const char * chewings);
/// ```
///
/// Provisional: routes through the same full-pinyin parse path until
/// the engine gains a dedicated chewing parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_chewings(
    instance: *mut PinyinInstance,
    chewings: *const c_char,
) -> usize {
    parse_c_string(instance, chewings)
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t pinyin_get_parsed_input_length(pinyin_instance_t * instance);
/// ```
///
/// Returns the byte count of raw input consumed by the most recent parse
/// call, `0` before any parse and after [`pinyin_reset`](crate::instance::pinyin_reset),
/// matching upstream `pinyin.cpp:1611-1613` and reset `pinyin.cpp:2692`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.parsed_len
    })
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool pinyin_in_chewing_keyboard(pinyin_instance_t * instance,
///                                  const char key,
///                                  gchar *** symbols);
/// ```
///
/// `key` is a plain `char` value (not a pointer).
/// `symbols` receives a NULL-terminated string array; caller frees with
/// `g_strfreev`.
///
/// Provisional: always returns false (no chewing keyboard tables yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_in_chewing_keyboard(
    instance: *mut PinyinInstance,
    _key: c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !symbols.is_null() {
        // SAFETY: Null-checked above. Write NULL to indicate no results.
        unsafe {
            *symbols = ptr::null_mut();
        }
    }
    false
}
