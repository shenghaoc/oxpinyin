//! Context lifecycle: `pinyin_init`, `pinyin_fini`, `pinyin_save`.

use std::os::raw::c_char;
use std::ptr;

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
    _systemdir: *const c_char,
    _userdir: *const c_char,
) -> *mut PinyinContext {
    // STUB: T2 will wire to real Session construction.
    ptr::null_mut()
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
    // SAFETY: `context` was created by `pinyin_init` via `Box::into_raw`.
    // The caller transfers ownership back; we reconstruct the Box and drop it.
    unsafe {
        drop(Box::from_raw(context));
    }
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
    // STUB: T4 will implement periodic save.
    false
}
