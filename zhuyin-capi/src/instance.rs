//! Instance lifecycle: `zhuyin_alloc_instance`, `zhuyin_free_instance`,
//! `zhuyin_reset`.

use std::ptr;

use crate::ffi::borrow_or_return;
use crate::state::{CapiInstance, box_instance, context_ref, instance_mut};
use crate::types::{ZhuyinContext, ZhuyinInstance};

/// Allocate a new zhuyin instance from a context.
///
/// # C signature
/// ```c
/// zhuyin_instance_t * zhuyin_alloc_instance(zhuyin_context_t * context);
/// ```
///
/// Returns NULL on failure or null context.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_alloc_instance(context: *mut ZhuyinContext) -> *mut ZhuyinInstance {
    if context.is_null() {
        return ptr::null_mut();
    }
    // A finalised context is freed memory the pin reads
    // (`zhuyin.cpp:845-857`): class (b), answered NULL without a log.
    if !crate::live::is_live(context) {
        return ptr::null_mut();
    }

    // SAFETY: `context` is non-null and was produced by `zhuyin_init`.
    let ctx = unsafe { context_ref(context) };
    // SAFETY: the public context is live for this serialized allocation call.
    let owner = unsafe { crate::state::context_owner(context) };
    box_instance(ctx.alloc_instance(owner))
}

/// Free a zhuyin instance.
///
/// # C signature
/// ```c
/// void zhuyin_free_instance(zhuyin_instance_t * instance);
/// ```
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies. After the call `instance` is dangling, as are its candidate rows and their strings.
/// Key and key-rest getters use separate static storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_free_instance(instance: *mut ZhuyinInstance) {
    if instance.is_null() {
        return;
    }

    // SAFETY: `instance` was created by `zhuyin_alloc_instance` via
    // `box_instance` (= `Box::into_raw`). The caller transfers ownership.
    unsafe {
        drop(Box::from_raw(instance.cast::<CapiInstance>()));
    };
}

/// Clear the instance's parsing state, sentence state, and candidate list.
///
/// # C signature
/// ```c
/// bool zhuyin_reset(zhuyin_instance_t * instance);
/// ```
///
/// The reset also clears constraints and phrase results (`zhuyin.cpp:1718-1729`).
/// The function returns `false` for a null `instance` and `true` after the
/// reset.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_reset(instance: *mut ZhuyinInstance) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    inst.core.full_reset();
    inst.state.candidates.clear();
    true
}
