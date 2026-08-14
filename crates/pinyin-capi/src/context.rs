//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::{CapiContext, box_context};
use crate::types::PinyinContext;

/// Create a new pinyin context.
///
/// # C signature
/// ```c
/// pinyin_context_t * pinyin_init(const char * systemdir, const char * userdir);
/// ```
///
/// Returns NULL on failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_init(
    systemdir: *const c_char,
    userdir: *const c_char,
) -> *mut PinyinContext {
    ffi_catch(ptr::null_mut(), || {
        // SAFETY: Both pointers are C strings from the caller (null OK).
        let system_dir = unsafe { cstr_to_string(systemdir) };
        let user_dir = unsafe { cstr_to_string(userdir) };
        let ctx = CapiContext::new(&system_dir, &user_dir);
        box_context(ctx)
    })
}

/// Finalize and free a pinyin context.
///
/// # C signature
/// ```c
/// void pinyin_fini(pinyin_context_t * context);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_fini(context: *mut PinyinContext) {
    if context.is_null() {
        return;
    }
    ffi_catch((), || {
        // SAFETY: `context` was created by `pinyin_init` via `box_context`
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
/// bool pinyin_save(pinyin_context_t * context);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_save(context: *mut PinyinContext) -> bool {
    if context.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // STUB: T4 will implement periodic save.
        let _ctx = unsafe { crate::state::context_ref(context) };
        true
    })
}
