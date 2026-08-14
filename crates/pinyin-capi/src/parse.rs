//! Parsing symbols: full pinyin, double pinyin, chewing.

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::instance_mut;
use crate::types::{GChar, PinyinInstance};

/// Parse multiple full pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_full_pinyins(pinyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Returns number of bytes consumed, 0 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_full_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        // SAFETY: `pinyins` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(pinyins) };
        if text.is_empty() {
            return 0;
        }
        inst.session.reset();
        match inst.session.type_pinyin(&text) {
            Ok(_) => inst.session.raw_input().len(),
            Err(_) => 0,
        }
    })
}

/// Parse multiple double pinyins.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_double_pinyins(pinyin_instance_t * instance,
///                                         const char * pinyins);
/// ```
///
/// Provisional: routes through the same full-pinyin parse path until
/// the engine gains a dedicated double-pinyin parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_double_pinyins(
    instance: *mut PinyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        // SAFETY: `pinyins` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(pinyins) };
        if text.is_empty() {
            return 0;
        }
        inst.session.reset();
        match inst.session.type_pinyin(&text) {
            Ok(_) => inst.session.raw_input().len(),
            Err(_) => 0,
        }
    })
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t pinyin_parse_more_chewings(pinyin_instance_t * instance,
///                                    const char * chewings);
/// ```
///
/// Provisional: routes through the same full-pinyin parse path until
/// the engine gains a dedicated chewing parser.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_more_chewings(
    instance: *mut PinyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        // SAFETY: `chewings` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(chewings) };
        if text.is_empty() {
            return 0;
        }
        inst.session.reset();
        match inst.session.type_pinyin(&text) {
            Ok(_) => inst.session.raw_input().len(),
            Err(_) => 0,
        }
    })
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool pinyin_in_chewing_keyboard(pinyin_instance_t * instance,
///                                  const char key,
///                                  gchar *** symbols);
/// ```
///
/// `key` is a plain `char` value (not a pointer).
/// `symbols` receives a NULL-terminated string array; caller frees with
/// `g_strfreev`.
///
/// Provisional: always returns false (no chewing keyboard tables yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_in_chewing_keyboard(
    instance: *mut PinyinInstance,
    _key: c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !symbols.is_null() {
        // SAFETY: Null-checked above. Write NULL to indicate no results.
        unsafe {
            *symbols = ptr::null_mut();
        }
    }
    false
}
