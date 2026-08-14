//! Parsing symbols: full pinyin, double pinyin, chewing.

use std::os::raw::c_char;
use std::ptr;

use crate::types::{GChar, PinyinInstance};

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
    _pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    // STUB: T3 will wire to the session's parse path.
    0
}

/// Parse multiple double pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_double_pinyins(pinyin_instance_t * instance,
///                                         const char * pinyins);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    _pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    // STUB: T3 will wire to the session's parse path.
    0
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_chewings(pinyin_instance_t * instance,
///                                    const char * chewings);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_chewings(
    instance: *mut PinyinInstance,
    _chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    // STUB: T3 will wire to the session's parse path.
    0
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
    // STUB: T3 will implement.
    false
}
