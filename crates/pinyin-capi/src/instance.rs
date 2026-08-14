//! Instance lifecycle: `pinyin_alloc_instance`, `pinyin_free_instance`,
//! `pinyin_reset`.

use std::ptr;

use crate::ffi::ffi_catch;
use crate::state::{CapiInstance, box_instance, context_ref, instance_mut};
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
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: `context` is non-null and was produced by `pinyin_init`.
        let ctx = unsafe { context_ref(context) };
        match ctx.alloc_instance() {
            Some(inst) => box_instance(inst),
            None => ptr::null_mut(),
        }
    })
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
    ffi_catch((), || {
        // SAFETY: `instance` was created by `pinyin_alloc_instance` via
        // `box_instance` (= `Box::into_raw`). The caller transfers ownership.
        unsafe {
            drop(Box::from_raw(instance.cast::<CapiInstance>()));
        }
    });
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
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        inst.session.reset();
        inst.candidates.clear();
        true
    })
}
