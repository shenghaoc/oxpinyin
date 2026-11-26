//! Editor instance allocation, input reset, and instance release.

use std::ptr;

use crate::ffi::borrow_or_return;
use crate::state::{CapiInstance, box_instance, context_ref, instance_mut};
use crate::types::{PinyinContext, PinyinInstance};

/// Allocate a new pinyin instance from a context.
///
/// # C signature
/// ```c
/// pinyin_instance_t * pinyin_alloc_instance(pinyin_context_t * context);
/// ```
///
/// Returns NULL on failure and on a null context. A non-null `context` must be a live context,
/// as the [crate-level handle rule](crate#safety) states.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `context` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_alloc_instance(context: *mut PinyinContext) -> *mut PinyinInstance {
    if context.is_null() {
        return ptr::null_mut();
    }
    // A context `pinyin_fini` already freed: the pin reads it anyway
    // (`pinyin.cpp:1322`) and gets away with it; here the answer is NULL
    // (register row 60, class (b)), with no log — the pin does not abort.
    if !crate::live::is_live(context) {
        return ptr::null_mut();
    }

    // SAFETY: `context` is non-null, was produced by `pinyin_init` and is
    // still live (checked above).
    let ctx = unsafe { context_ref(context) };
    // SAFETY: the public context is live for this serialized allocation call.
    let owner = unsafe { crate::state::context_owner(context) };
    box_instance(ctx.alloc_instance(context, owner))
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
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies. The returned pointer is the context handle the instance was allocated from;
/// `pinyin_fini` may already have taken it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_context(instance: *mut PinyinInstance) -> *mut PinyinContext {
    let inst = borrow_or_return!(instance, crate::state::instance_state_ref, ptr::null_mut());
    inst.context
}

/// Free a pinyin instance.
///
/// # C signature
/// ```c
/// void pinyin_free_instance(pinyin_instance_t * instance);
/// ```
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies. After the call `instance` is dangling, as are its candidate rows and their strings.
/// Key and key-rest getters use separate static storage.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_free_instance(instance: *mut PinyinInstance) {
    if instance.is_null() {
        return;
    }

    // SAFETY: `instance` was created by `pinyin_alloc_instance` via
    // `box_instance` (= `Box::into_raw`). The caller transfers ownership.
    unsafe {
        drop(Box::from_raw(instance.cast::<CapiInstance>()));
    };
}

/// Clear the instance's parsing state, sentence state, and candidate list.
///
/// # C signature
/// ```c
/// bool pinyin_reset(pinyin_instance_t * instance);
/// ```
///
/// The reset also clears constraints and phrase results (`pinyin.cpp:2693-2704`).
/// The function returns `false` for a null `instance` and `true` after the
/// reset.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle. The serial-calls rule
/// applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_reset(instance: *mut PinyinInstance) -> bool {
    let mut inst = borrow_or_return!(instance, instance_mut, false);
    inst.core.full_reset();
    inst.state.candidates.clear();
    true
}
