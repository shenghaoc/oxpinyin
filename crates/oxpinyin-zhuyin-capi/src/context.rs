//! Context lifecycle: `zhuyin_init`, `zhuyin_fini`, `zhuyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::{cstr_to_owned_lossy, ffi_catch};
use crate::state::{CapiContext, box_context, context_mut};
use crate::types::ZhuyinContext;

/// Create a new zhuyin context.
///
/// # C signature
/// ```c
/// zhuyin_context_t * zhuyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Opens the system dictionary and language model tables from `systemdir`.
/// Returns NULL when `systemdir` is empty, any table fails to open, or the
/// system dir has no parsable `interpolation2.text` real-unigram model.
///
/// **Divergence note (the pin seeds `USE_TONE | FORCE_TONE`).** The zhuyin
/// facade's context defaults to `m_options = USE_TONE | FORCE_TONE`
/// (`zhuyin.cpp:272` at the pin 0c5e80e1), unlike `pinyin_init`, which seeds
/// only `PINYIN_INCOMPLETE`. The zhuyin parser honours `FORCE_TONE` nested
/// inside `USE_TONE` for the Simple and CP26 keyboards
/// (`zhuyin_parser2.cpp:178,602`) and unconditionally for Discrete
/// (`:373,:387`) — the same law the shared [`oxpinyin_core::ZhuyinParser`]
/// implements. If that law cannot be reproduced exactly for a keyboard, the
/// gap is registered under the existing `FORCE_TONE on double-pinyin/zhuyin
/// schemes` divergence class rather than silently absorbed.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut ZhuyinContext {
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: Both pointers are C strings from the caller (null OK).
        let system_dir = cstr_to_owned_lossy(systemdir);
        let user_dir = cstr_to_owned_lossy(userdir);
        match CapiContext::open(&system_dir, &user_dir) {
            Some(ctx) => box_context(ctx),
            None => ptr::null_mut(),
        }
    })
}

/// Finalize and free a zhuyin context.
///
/// # C signature
/// ```c
/// void zhuyin_fini(zhuyin_context_t * context);
/// ```
///
/// Deliberately does **not** save — upstream's teardown has no flush.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_fini(context: *mut ZhuyinContext) {
    if context.is_null() {
        return;
    }
    ffi_catch((), || {
        // SAFETY: `context` was created by `zhuyin_init` via `box_context`
        // (= `Box::into_raw`). The caller transfers ownership back.
        unsafe {
            drop(Box::from_raw(context.cast::<CapiContext>()));
        }
    });
}

/// Save user data.
///
/// # C signature
/// ```c
/// bool zhuyin_save(zhuyin_context_t * context);
/// ```
///
/// The §4 semantics: `false` when there is no user directory or nothing
/// changed since the last save; `true` after a dirty save.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_save(context: *mut ZhuyinContext) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `context` is non-null and was produced by `zhuyin_init`;
        // the unique borrow lasts only for the save call.
        let ctx = unsafe { context_mut(context) };
        ctx.save_user()
    })
}
