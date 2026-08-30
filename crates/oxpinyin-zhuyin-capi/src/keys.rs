//! Single-key parsing and the `ChewingKey` display getters.
//!
//! The single-key parsers are pure probes of (live options, one string).
//! The display getters read the key a caller hands them through the packed
//! ABI word. `zhuyin_get_pinyin_string` dispatches on the context's
//! full-pinyin scheme (Hanyu / Luoma / SecondaryZhuyin) — the zhuyin-facade
//! distinction the pinyin facade's `pinyin_get_pinyin_string` does not make
//! (`zhuyin.cpp:1743-1766`).

use std::os::raw::c_char;
use std::ptr;
use std::sync::atomic::Ordering;

use oxpinyin_core::{FullPinyinParser, ZHUYIN_CORRECT_ALL, ZhuyinParser};

use crate::ffi::{cstr_to_string, ffi_catch, owned_cstr};
use crate::parse::{full_scheme, zhuyin_scheme};
use crate::state::instance_ref;
use crate::types::{ChewingKey, GChar, ZhuyinInstance};

/// Parse one full pinyin into a key.
///
/// # C signature
/// ```c
/// bool zhuyin_parse_full_pinyin(zhuyin_instance_t * instance,
///                               const char * onepinyin,
///                               ChewingKey * onekey);
/// ```
///
/// Upstream zeroes `*onekey` before its probe (`zhuyin.cpp:1001-1014`), so a
/// failed parse leaves the zero key; the probe is `FullPinyinParser2::
/// parse_one_key` over the live option word, with `PINYIN_CORRECT_ALL`
/// masked first (`zhuyin.cpp:1013`).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_full_pinyin(
    instance: *mut ZhuyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onepinyin.is_null() || onekey.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // SAFETY: Null-checked above.
        let text = unsafe { cstr_to_string(onepinyin) };
        // SAFETY: Null-checked above; written through the caller's storage.
        unsafe {
            *onekey = ChewingKey::ZERO;
        }
        match FullPinyinParser.parse_one_key(
            inst.options().bits() & !oxpinyin_core::PINYIN_CORRECT_ALL,
            text.as_bytes(),
        ) {
            Some(key) => {
                // SAFETY: Null-checked above.
                unsafe { *onekey = ChewingKey::from_core(key) };
                true
            }
            None => false,
        }
    })
}

/// Parse one chewing (bopomofo) keystroke string into a key.
///
/// # C signature
/// ```c
/// bool zhuyin_parse_chewing(zhuyin_instance_t * instance,
///                           const char * onechewing,
///                           ChewingKey * onekey);
/// ```
///
/// The context's live Zhuyin scheme parses the probe after the API's
/// `ZHUYIN_CORRECT_ALL` strip (`zhuyin.cpp:1045-1056`). The key is written
/// only on success.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_chewing(
    instance: *mut ZhuyinInstance,
    onechewing: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onechewing.is_null() || onekey.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // SAFETY: Null-checked above.
        let text = unsafe { cstr_to_string(onechewing) };
        let scheme = zhuyin_scheme(inst.zhuyin_scheme.load(Ordering::Relaxed));
        let Some(scheme) = scheme else {
            return false;
        };
        let options = inst.options().bits() & !ZHUYIN_CORRECT_ALL;
        match ZhuyinParser::with_scheme(scheme).parse_one_key(options, text.as_bytes()) {
            Some(key) => {
                // SAFETY: Null-checked above.
                unsafe { *onekey = ChewingKey::from_core(key) };
                true
            }
            None => false,
        }
    })
}

/// `zhuyin_get_zhuyin_string` — render the key's zhuyin spelling.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_string(zhuyin_instance_t * instance,
///                               ChewingKey * key, gchar ** utf8_str);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_string(
    instance: *mut ZhuyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    display_string_getter(instance, key, utf8_str, |core| core.zhuyin_string())
}

/// `zhuyin_get_pinyin_string` — render the key's pinyin spelling, dispatching
/// on the context's full-pinyin scheme (`zhuyin.cpp:1743-1766`).
///
/// # C signature
/// ```c
/// bool zhuyin_get_pinyin_string(zhuyin_instance_t * instance,
///                               ChewingKey * key, gchar ** utf8_str);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_pinyin_string(
    instance: *mut ZhuyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    if instance.is_null() || key.is_null() || utf8_str.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = ptr::null_mut();
        }
        // SAFETY: Null-checked above.
        let slot = unsafe { &*key };
        let decoded = slot.to_core();
        if decoded.table_index() == 0 {
            return false;
        }
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let scheme = unsafe { instance_ref(instance) }
            .full_scheme
            .load(Ordering::Relaxed);
        let rendered = match full_scheme(scheme) {
            Some(oxpinyin_core::FullPinyinScheme::Luoma) => decoded.luoma_pinyin_string(),
            Some(oxpinyin_core::FullPinyinScheme::SecondaryZhuyin) => {
                decoded.secondary_zhuyin_string()
            }
            _ => decoded.pinyin_string(),
        };
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = owned_cstr(&rendered);
        }
        true
    })
}

/// The shared body of `zhuyin_get_zhuyin_string`: NULL the out-param, refuse
/// the zero key on `get_table_index() == 0`, render.
fn display_string_getter(
    instance: *mut ZhuyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
    render: impl FnOnce(oxpinyin_core::ChewingKey) -> String,
) -> bool {
    if instance.is_null() || key.is_null() || utf8_str.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = ptr::null_mut();
        }
        // SAFETY: Null-checked above.
        let core = unsafe { *key }.to_core();
        if core.table_index() == 0 {
            return false;
        }
        let rendered = render(core);
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = owned_cstr(&rendered);
        }
        !utf8_str.is_null()
    })
}
