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

use crate::ffi::{borrow_or_return, key_text};
use crate::state::instance_ref;
use crate::types::{ChewingKey, GChar, ZhuyinInstance};

// zhuyin.cpp:1001-1014, pinyin_parser2.cpp:170.
/// Parse one full pinyin into a key.
///
/// # C signature
/// ```c
/// bool zhuyin_parse_full_pinyin(zhuyin_instance_t * instance,
///                               const char * onepinyin,
///                               ChewingKey * onekey);
/// ```
///
/// Returns true and writes the key on success. A failed parse writes a zero key. A null argument
/// returns false before writing. Full-pinyin correction options are masked (`zhuyin.cpp:1013`).
///
/// # Aborts
///
/// An apostrophe in the single-key input aborts before the output is written
/// (`pinyin_parser2.cpp:168-175`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle, `onepinyin` is a string
/// and `onekey` is an out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_parse_full_pinyin(
    instance: *mut ZhuyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false, onepinyin, onekey);
    let text = oxpinyin::marshal::input::scalar_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(onepinyin) },
    )
    .unwrap_or_default();
    // Register row 21, `pinyin_parser2.cpp:170` via `zhuyin.cpp:1012`: the
    // shared `parse_one_key` asserts the text holds no `'`, before it
    // touches `*onekey`, and the process aborts.
    if
    // SAFETY: Strings guarantees valid, unchanged storage.
    // The borrow ends in this statement.
    unsafe { crate::ffi::borrow_c_string(onepinyin) }
        .is_some_and(|input| input.to_bytes().contains(&b'\''))
    {
        std::process::abort();
    }
    // SAFETY: Null-checked above; written through the caller's storage.
    unsafe {
        *onekey = ChewingKey::ZERO;
    }
    inst.core
        .parse_one_full_pinyin(&text, oxpinyin::facade::PinyinCorrections::Masked)
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
/// Uses the current keyboard. This implementation masks the chewing correction options for the
/// single-key probe. The pin passes its option word to the keyboard parser
/// (`zhuyin.cpp:1045-1056`). Only a successful parse writes the key.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle, `onechewing` is a
/// string and `onekey` is an out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_parse_chewing(
    instance: *mut ZhuyinInstance,
    onechewing: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false, onechewing, onekey);
    let text = oxpinyin::marshal::input::scalar_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(onechewing) },
    )
    .unwrap_or_default();
    inst.core.parse_one_chewing(&text).is_some_and(|key| {
        // SAFETY: Null-checked above.
        unsafe { *onekey = ChewingKey::from_core(key) };
        true
    })
}

// zhuyin.cpp:1732-1741.
/// Render a chewing key in Zhuyin.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_string(zhuyin_instance_t * instance,
///                               ChewingKey * key, gchar ** utf8_str);
/// ```
///
/// Returns true for a key with a spelling. Free the returned string with `g_free`. An unset key
/// writes null and returns false. A null key returns false without writing. A null output
/// returns false.
///
/// # Aborts
///
/// A packed key with an out-of-range component, or an invalid tone reached during rendering,
/// aborts (`chewing_key.cpp:28-42,75-108`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key` is a borrowed object and `utf8_str` is an
/// out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_zhuyin_string(
    _instance: *mut ZhuyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    // SAFETY: The entry point guarantees Out-parameters and Borrowed objects: a non-NULL output is
    // aligned and writable, and a non-NULL key is valid for reads. Access must not conflict
    // with live borrows.
    unsafe { display_string_getter(key, utf8_str, oxpinyin::ChewingKey::zhuyin_text) }
}

// zhuyin.cpp:1743-1766.
/// Render a chewing key in the context's full Pinyin scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_get_pinyin_string(zhuyin_instance_t * instance,
///                               ChewingKey * key, gchar ** utf8_str);
/// ```
///
/// Returns true for a key with a spelling. Free the returned string with `g_free`. An unset key
/// writes null and returns false. A null key returns false without writing. A null output
/// returns false. A null instance returns false without writing. The current full-pinyin scheme
/// determines the spelling.
///
/// # Aborts
///
/// A packed key with an out-of-range component, or an invalid tone reached during rendering,
/// aborts (`chewing_key.cpp:28-42,75-108`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle, `key` is a borrowed
/// object and `utf8_str` is an out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zhuyin_get_pinyin_string(
    instance: *mut ZhuyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    // The scheme lives in the context's shared live options, read through the
    // instance; pass it into the closure so the shared getter can dispatch.
    let render = |core: oxpinyin::ChewingKey| {
        // SAFETY: Handles guarantees the NULL-checked instance is live.
        // Serial calls excludes mutation; no other view is held while
        // this closure reads the scheme.
        let scheme = unsafe { instance_ref(instance) }
            .core
            .live
            .full_scheme
            .get();
        match oxpinyin::facade::full_scheme(scheme) {
            Some(oxpinyin::FullPinyinScheme::Luoma) => core.luoma_pinyin_text(),
            Some(oxpinyin::FullPinyinScheme::SecondaryZhuyin) => core.secondary_zhuyin_text(),
            Some(oxpinyin::FullPinyinScheme::Hanyu) | None => core.pinyin_text(),
        }
    };
    // SAFETY: The entry point guarantees Out-parameters and Borrowed objects: a non-NULL output is
    // aligned and writable, and a non-NULL key is valid for reads. Access must not conflict
    // with live borrows.
    unsafe { display_string_getter(key, utf8_str, render) }
}

/// Clear the output before checking the table index. A zero row leaves it
/// NULL. On success the caller frees the rendered spelling with `g_free`
/// (`zhuyin.cpp:1732-1766`).
///
/// # Safety
///
/// Under the crate-level rules, Out-parameters and Borrowed objects: a
/// non-NULL output is aligned and writable, and a non-NULL key is valid
/// for reads. Access must not conflict with live borrows.
unsafe fn display_string_getter(
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
    render: impl FnOnce(oxpinyin::ChewingKey) -> oxpinyin::KeyText,
) -> bool {
    // Guard both pointers before the first output write (row 41).
    if key.is_null() || utf8_str.is_null() {
        return false;
    }

    // SAFETY: Out-parameters guarantees aligned writable storage; NULL was checked.
    unsafe {
        *utf8_str = ptr::null_mut();
    }
    // SAFETY: Borrowed objects guarantees a readable key; NULL was checked.
    let core = unsafe { *key }.to_core();
    if !core.elements_in_dims() {
        std::process::abort();
    }
    if core.table_index() == 0 {
        return false;
    }
    if !core.tone_in_dims() {
        std::process::abort();
    }
    // SAFETY: Out-parameters guarantees aligned writable storage; NULL was checked.
    unsafe {
        *utf8_str = key_text(render(core));
    }
    true
}
