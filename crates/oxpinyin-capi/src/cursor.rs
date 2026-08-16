//! Pinyin key access and cursor/offset navigation.
//!
//! Provisional: cursor helpers return identity/boundary values until the
//! engine exposes per-key positional data (T4).

use std::ptr;

use crate::ffi::ffi_catch;
use crate::state::instance_ref;
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
///
/// Provisional: always returns false (no key-rest data structure yet).
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
///
/// Provisional: always returns false (no key-rest data yet).
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
///
/// Provisional: identity mapping (offset = cursor), clamped to raw input
/// length.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_offset(
    instance: *mut PinyinInstance,
    cursor: usize,
    offset: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let len = inst.session.raw_input().len();
        if !offset.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *offset = cursor.min(len);
            }
        }
        true
    })
}

/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_left_pinyin_offset(pinyin_instance_t * instance,
///                                    size_t offset,
///                                    size_t * left);
/// ```
///
/// Provisional: returns `offset.saturating_sub(1)`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_left_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    left: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        if !left.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *left = offset.saturating_sub(1);
            }
        }
        true
    })
}

/// Get the right offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_right_pinyin_offset(pinyin_instance_t * instance,
///                                     size_t offset,
///                                     size_t * right);
/// ```
///
/// Provisional: returns `min(offset.saturating_add(1), raw_input.len())`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_right_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    right: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let len = inst.session.raw_input().len();
        if !right.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *right = offset.saturating_add(1).min(len);
            }
        }
        true
    })
}
