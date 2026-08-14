//! Instance lifecycle: `pinyin_alloc_instance`, `pinyin_free_instance`,
//! `pinyin_reset`.

use std::ptr;

use crate::types::{PinyinContext, PinyinInstance};

/// Allocate a new pinyin instance from a context.
///
/// # C signature
/// ```c
/// pinyin_instance_t * pinyin_alloc_instance(pinyin_context_t * context);
/// ```
///
/// Returns NULL on failure or null context.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_alloc_instance(context: *mut PinyinContext) -> *mut PinyinInstance {
    if context.is_null() {
        return ptr::null_mut();
    }
    // STUB: T2 will wire to real instance construction.
    ptr::null_mut()
}

/// Free a pinyin instance.
///
/// # C signature
/// ```c
/// void pinyin_free_instance(pinyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_free_instance(instance: *mut PinyinInstance) {
    if instance.is_null() {
        return;
    }
    // SAFETY: `instance` was created by `pinyin_alloc_instance` via
    // `Box::into_raw`. The caller transfers ownership back.
    unsafe {
        drop(Box::from_raw(instance));
    }
}

/// Reset the pinyin instance (clear parsing and sentence state).
///
/// # C signature
/// ```c
/// bool pinyin_reset(pinyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_reset(instance: *mut PinyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }
    // STUB: T3 will implement.
    false
}
