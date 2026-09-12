//! Instance lifecycle: `zhuyin_alloc_instance`, `zhuyin_free_instance`,
//! `zhuyin_reset`.

use std::ptr;

use crate::state::{CapiInstance, box_instance, context_ref, instance_mut, instance_ref};
use crate::types::{ZhuyinContext, ZhuyinInstance};

/// Allocate a new zhuyin instance from a context.
///
/// # C signature
/// ```c
/// zhuyin_instance_t * zhuyin_alloc_instance(zhuyin_context_t * context);
/// ```
///
/// Returns NULL on failure or null context.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_alloc_instance(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    if context.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_ref(context) };
    ctx.alloc_instance(context)
        .map_or(ptr::null_mut(), box_instance)
}

/// Free a zhuyin instance.
///
/// # C signature
/// ```c
/// void zhuyin_free_instance(zhuyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_free_instance(instance: *mut ZhuyinInstance) {
    if instance.is_null() {
        return;
    }

    // SAFETY: `instance` was created by `zhuyin_alloc_instance` via
    // `box_instance` (= `Box::into_raw`). The caller transfers ownership.
    unsafe {
        drop(Box::from_raw(instance.cast::<CapiInstance>()));
    };
}

/// Reset the zhuyin instance (clear parsing and sentence state).
///
/// # C signature
/// ```c
/// bool zhuyin_reset(zhuyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_reset(instance: *mut ZhuyinInstance) -> bool {
    if instance.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.core.full_reset();
    inst.candidates.clear();
    true
}

/// Get the zhuyin context from a zhuyin instance.
///
/// Internal helper (not an exported `libzhuyin.ver` symbol): upstream's
/// `zhuyin.h` declares it, but it is absent from the 52-symbol export list,
/// so a Rust `extern "C"` would leak it past the version-script boundary.
/// Nothing in this crate calls it yet; it is the only reader of
/// `CapiInstance::context`.
#[expect(
    dead_code,
    reason = "upstream-declared helper kept off the export list; no in-crate caller yet"
)]
pub fn zhuyin_get_context(instance: *mut ZhuyinInstance) -> *mut ZhuyinContext {
    if instance.is_null() {
        return ptr::null_mut();
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    inst.context
}
