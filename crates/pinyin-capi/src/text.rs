//! Auxiliary text retrieval.

use std::ptr;

use crate::types::{GChar, PinyinInstance};

/// Get auxiliary text for full pinyin display.
///
/// # C signature
/// ```c
/// bool pinyin_get_full_pinyin_auxiliary_text(pinyin_instance_t * instance,
///                                            size_t cursor,
///                                            gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_full_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    _cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !aux_text.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *aux_text = ptr::null_mut();
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get auxiliary text for double pinyin display.
///
/// # C signature
/// ```c
/// bool pinyin_get_double_pinyin_auxiliary_text(pinyin_instance_t * instance,
///                                              size_t cursor,
///                                              gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_double_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    _cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !aux_text.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *aux_text = ptr::null_mut();
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get auxiliary text for chewing (bopomofo) display.
///
/// # C signature
/// ```c
/// bool pinyin_get_chewing_auxiliary_text(pinyin_instance_t * instance,
///                                        size_t cursor,
///                                        gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_chewing_auxiliary_text(
    instance: *mut PinyinInstance,
    _cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !aux_text.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *aux_text = ptr::null_mut();
        }
    }
    // STUB: T3 will implement.
    false
}
