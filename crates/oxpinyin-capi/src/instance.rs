//! Instance lifecycle: `pinyin_alloc_instance`, `pinyin_free_instance`,
//! `pinyin_reset`.

use std::ptr;

use crate::ffi::ffi_catch;
use crate::state::{CapiInstance, box_instance, context_ref, instance_mut, instance_ref};
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
        ctx.alloc_instance(context)
            .map_or(ptr::null_mut(), box_instance)
    })
}

/// Get the pinyin context from a pinyin instance.
///
/// # C signature
/// ```c
/// pinyin_context_t * pinyin_get_context (pinyin_instance_t * instance);
/// ```
///
/// Upstream is a one-line field read (`pinyin.cpp:1358-1360`); the
/// returned handle is the caller's to keep using under the context's own
/// lifetime. NULL for a null instance.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_context(instance: *mut PinyinInstance) -> *mut PinyinContext {
    if instance.is_null() {
        return ptr::null_mut();
    }
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.context
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
///
/// The full reset: upstream's `pinyin_reset` also clears the instance's
/// constraint store (`pinyin.cpp:2697`) — the parse path's
/// [`CapiInstance::reset_parse_state`] split deliberately leaves it alive
/// across keystrokes — and the phrase result (`pinyin.cpp:2699`), which
/// this clears with it.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_reset(instance: *mut PinyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        inst.reset_parse_state();
        inst.phrase_result.clear();
        inst.session.reset();
        true
    })
}
