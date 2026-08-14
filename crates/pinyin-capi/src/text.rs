//! Auxiliary text retrieval.
//!
//! Provisional: all three input modes return the session's preedit text
//! until the engine has dedicated formatting per scheme.

use crate::ffi::{ffi_catch, owned_cstr};
use crate::state::instance_ref;
use crate::types::{GChar, PinyinInstance};

/// Get auxiliary text for full pinyin display.
///
/// # C signature
/// ```c
/// bool pinyin_get_full_pinyin_auxiliary_text(pinyin_instance_t * instance,
///                                            size_t cursor,
///                                            gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`). The returned buffer is
/// allocated with libc `malloc`, which `g_free` releases on every platform.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_full_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    _cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let preedit = inst.session.preedit();
        if !aux_text.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(preedit.text());
            // SAFETY: Null-checked above.
            unsafe {
                *aux_text = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}

/// Get auxiliary text for double pinyin display.
///
/// # C signature
/// ```c
/// bool pinyin_get_double_pinyin_auxiliary_text(pinyin_instance_t * instance,
///                                              size_t cursor,
///                                              gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`).
///
/// Provisional: returns the same preedit text as full pinyin.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_double_pinyin_auxiliary_text(
    instance: *mut PinyinInstance,
    _cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let preedit = inst.session.preedit();
        if !aux_text.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(preedit.text());
            // SAFETY: Null-checked above.
            unsafe {
                *aux_text = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}

/// Get auxiliary text for chewing (bopomofo) display.
///
/// # C signature
/// ```c
/// bool pinyin_get_chewing_auxiliary_text(pinyin_instance_t * instance,
///                                        size_t cursor,
///                                        gchar ** aux_text);
/// ```
///
/// Out-param `aux_text` is caller-owned (`g_free`).
///
/// Provisional: returns the same preedit text as full pinyin.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_chewing_auxiliary_text(
    instance: *mut PinyinInstance,
    _cursor: usize,
    aux_text: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let preedit = inst.session.preedit();
        if !aux_text.is_null() {
            // SAFETY: Null-checked above. `owned_cstr` returns null on an
            // interior NUL or allocation failure; otherwise ownership
            // transfers to the caller, which frees it with `g_free`.
            let owned = owned_cstr(preedit.text());
            // SAFETY: Null-checked above.
            unsafe {
                *aux_text = owned;
            }
            if owned.is_null() {
                return false;
            }
        }
        true
    })
}
