//! Pinyin key access and cursor/offset navigation.

use std::ptr;

use crate::types::{ChewingKeyRest, PinyinInstance};

/// Get the pinyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest(pinyin_instance_t * instance,
///                                 size_t offset,
///                                 ChewingKeyRest ** key_rest);
/// ```
///
/// Out-param `key_rest` is instance-borrowed.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest(
    instance: *mut PinyinInstance,
    _offset: usize,
    key_rest: *mut *mut ChewingKeyRest,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !key_rest.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *key_rest = ptr::null_mut();
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get the begin/end byte positions of a pinyin key rest.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest_positions(pinyin_instance_t * instance,
///                                           ChewingKeyRest * key_rest,
///                                           guint16 * begin,
///                                           guint16 * end);
/// ```
///
/// Either `begin` or `end` may be NULL to skip.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest_positions(
    instance: *mut PinyinInstance,
    key_rest: *mut ChewingKeyRest,
    begin: *mut u16,
    end: *mut u16,
) -> bool {
    if instance.is_null() || key_rest.is_null() {
        return false;
    }
    if !begin.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *begin = 0;
        }
    }
    if !end.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *end = 0;
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get the lookup offset from a user cursor position.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_offset(pinyin_instance_t * instance,
///                               size_t cursor,
///                               size_t * offset);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_offset(
    instance: *mut PinyinInstance,
    _cursor: usize,
    offset: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !offset.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *offset = 0;
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_left_pinyin_offset(pinyin_instance_t * instance,
///                                    size_t offset,
///                                    size_t * left);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_left_pinyin_offset(
    instance: *mut PinyinInstance,
    _offset: usize,
    left: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !left.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *left = 0;
        }
    }
    // STUB: T3 will implement.
    false
}

/// Get the right offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_right_pinyin_offset(pinyin_instance_t * instance,
///                                     size_t offset,
///                                     size_t * right);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_right_pinyin_offset(
    instance: *mut PinyinInstance,
    _offset: usize,
    right: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !right.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *right = 0;
        }
    }
    // STUB: T3 will implement.
    false
}
