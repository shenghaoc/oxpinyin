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
    // SAFETY: `context` is non-null (guarded above). `pinyin_init` currently
    // always returns NULL (T1 stub), so this branch is unreachable until T2
    // makes the constructor return `Box::into_raw(..)`. At that point the
    // caller transfers ownership back here and only here, so reconstructing
    // and dropping the Box is sound.
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
