//! Single-key parsing and the `ChewingKey` display getters.
//!
//! The single-key parsers are pure probes of (live options, one string).
//! The display getters read the key a caller hands them through the packed
//! ABI word. `zhuyin_get_pinyin_string` dispatches on the context's
//! full-pinyin scheme (Hanyu / Luoma / `SecondaryZhuyin`) — the zhuyin-facade
//! distinction the pinyin facade's `pinyin_get_pinyin_string` does not make
//! (`zhuyin.cpp:1743-1766`).

use std::os::raw::c_char;
use std::ptr;
use std::sync::atomic::Ordering;

use crate::ffi::owned_cstr;
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
/// masked first (`zhuyin.cpp:1013`). A string holding `'` aborts the
/// process, as that probe's assert does (`pinyin_parser2.cpp:170`, register
/// row 21).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_full_pinyin(
    instance: *mut ZhuyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onepinyin.is_null() || onekey.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // SAFETY: Null-checked above.
    let text = oxpinyin_capi_marshal::input::scalar_text(oxpinyin_capi_marshal::borrow_c_string!(
        onepinyin
    ))
    .unwrap_or_default();
    // Register row 21, `pinyin_parser2.cpp:170` via `zhuyin.cpp:1012`: the
    // shared `parse_one_key` asserts the text holds no `'`, before it
    // touches `*onekey`, and the process aborts.
    if oxpinyin_capi_marshal::borrow_c_string!(onepinyin)
        .is_some_and(|input| input.to_bytes().contains(&b'\''))
    {
        std::process::abort();
    }
    // SAFETY: Null-checked above; written through the caller's storage.
    unsafe {
        *onekey = ChewingKey::ZERO;
    }
    inst.core
        .parse_one_full_pinyin(&text, true)
        .is_some_and(|key| {
            // SAFETY: Null-checked above.
            unsafe { *onekey = ChewingKey::from_core(key) };
            true
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

    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    // SAFETY: Null-checked above.
    let text = oxpinyin_capi_marshal::input::scalar_text(oxpinyin_capi_marshal::borrow_c_string!(
        onechewing
    ))
    .unwrap_or_default();
    inst.core.parse_one_chewing(&text).is_some_and(|key| {
        // SAFETY: Null-checked above.
        unsafe { *onekey = ChewingKey::from_core(key) };
        true
    })
}

/// `zhuyin_get_zhuyin_string` — render the key's zhuyin spelling.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_string(zhuyin_instance_t * instance,
///                               ChewingKey * key, gchar ** utf8_str);
/// ```
///
/// The pin's body never reads the instance (`zhuyin.cpp:1732-1741`): a NULL
/// instance is a defined call there, so only the key and the out-param are
/// touched and the instance parameter is unused (register row 41's class (b)
/// guard does not apply to a parameter the pin never dereferences).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_string(
    _instance: *mut ZhuyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    display_string_getter(key, utf8_str, oxpinyin_core::ChewingKey::zhuyin_string)
}

/// `zhuyin_get_pinyin_string` — render the key's pinyin spelling, dispatching
/// on the context's full-pinyin scheme (`zhuyin.cpp:1743-1766`).
///
/// # C signature
/// ```c
/// bool zhuyin_get_pinyin_string(zhuyin_instance_t * instance,
///                               ChewingKey * key, gchar ** utf8_str);
/// ```
///
/// Unlike the zhuyin getter this one reads the instance's scheme, so the pin
/// dereferences it and a NULL one is register row 41's class (b) site; the
/// guard answers `false`.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_pinyin_string(
    instance: *mut ZhuyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    // The scheme lives on the shared context atomic, read through the
    // instance; pass it into the closure so the shared getter can dispatch.
    display_string_getter(key, utf8_str, |core| {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let scheme = unsafe { instance_ref(instance) }
            .core
            .live
            .full_scheme
            .load(Ordering::Relaxed);
        match oxpinyin_facade::full_scheme(scheme) {
            Some(oxpinyin_core::FullPinyinScheme::Luoma) => core.luoma_pinyin_string(),
            Some(oxpinyin_core::FullPinyinScheme::SecondaryZhuyin) => {
                core.secondary_zhuyin_string()
            }
            _ => core.pinyin_string(),
        }
    })
}

/// The shared body of the two string getters: NULL the out-param, refuse the
/// zero key on `get_table_index() == 0`, render. The instance is not read
/// here; `zhuyin_get_pinyin_string` guards it before the call.
fn display_string_getter(
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
    render: impl FnOnce(oxpinyin_core::ChewingKey) -> String,
) -> bool {
    if key.is_null() || utf8_str.is_null() {
        return false;
    }

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
    // Return success only when the allocation succeeded; `owned_cstr`
    // returns null on an interior NUL or OOM.
    let owned = owned_cstr(&rendered);
    // SAFETY: Null-checked above.
    unsafe {
        *utf8_str = owned;
    }
    !owned.is_null()
}
