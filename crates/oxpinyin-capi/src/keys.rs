//! Single-key parsing and the `ChewingKey` display getters.
//!
//! The single-key parsers are pure probes of (live options, one string):
//! unlike the `parse_more_*` batch entries they touch no instance parse
//! state — no matrix, no `m_parsed_len` (upstream `pinyin_parse_full_pinyin`
//! `pinyin.cpp:1476-1491`, `pinyin_parse_double_pinyin` `:1523-1534`,
//! `pinyin_parse_chewing` `:1560-1575`). The display getters read the key
//! a caller hands them through the packed ABI word
//! (`pinyin_get_zhuyin_string` and siblings, `pinyin.cpp:2700-2766`).
//!
//! Failure shapes mirror the pin byte for byte: the full-pinyin entry
//! zeroes `*onekey` before its probe (`key = ChewingKey()`,
//! `pinyin.cpp:1484`) and leaves it zeroed on failure, while the double
//! and chewing entries write `*onekey` only on success. The string
//! getters NULL the out-param before the guard — except
//! `pinyin_get_pinyin_strings`, which leaves its out-params untouched on
//! the failing guard and tolerates NULL out-params on success
//! (`pinyin.cpp:2744-2757`).

use std::os::raw::c_char;
use std::ptr;

use crate::ffi::key_text;
use crate::state::instance_ref;
use crate::types::{ChewingKey, GChar, PinyinInstance};

// ── Single-key parsing ───────────────────────────────────────────────

/// Parse one full pinyin into a key.
///
/// # C signature
/// ```c
/// bool pinyin_parse_full_pinyin(pinyin_instance_t * instance,
///                               const char * onepinyin,
///                               ChewingKey * onekey);
/// ```
///
/// Upstream zeroes `*onekey` before its probe, so a failed parse leaves
/// the zero key (`pinyin.cpp:1484`); the probe itself is
/// `FullPinyinParser2::parse_one_key` over the live option word, with
/// the `FORCE_TONE` law inside `USE_TONE`.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle, `onepinyin` is a string
/// and `onekey` is an out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_parse_full_pinyin(
    instance: *mut PinyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onepinyin.is_null() || onekey.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    let text = oxpinyin_capi_marshal::input::scalar_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(onepinyin) },
    )
    .unwrap_or_default();
    // Register row 21, `pinyin_parser2.cpp:170`: `parse_one_key` asserts
    // the text holds no `'`, and the process aborts.
    if
    // SAFETY: Strings guarantees valid, unchanged storage.
    // The borrow ends in this statement.
    unsafe { crate::ffi::borrow_c_string(onepinyin) }
        .is_some_and(|input| input.to_bytes().contains(&b'\''))
    {
        std::process::abort();
    }
    // SAFETY: Null-checked above; written through the caller's
    // storage exactly once per branch.
    unsafe {
        *onekey = ChewingKey::ZERO;
    }
    inst.core
        .parse_one_full_pinyin(&text, oxpinyin_facade::PinyinCorrections::FromOptions)
        .is_some_and(|key| {
            // SAFETY: Null-checked above.
            unsafe { *onekey = ChewingKey::from_core(key) };
            true
        })
}

/// Parse one double pinyin into a key.
///
/// # C signature
/// ```c
/// bool pinyin_parse_double_pinyin(pinyin_instance_t * instance,
///                                 const char * onepinyin,
///                                 ChewingKey * onekey);
/// ```
///
/// Unlike the full-pinyin entry, upstream never zeroes `*onekey` here —
/// a failed parse leaves the caller's key untouched. The parser is the
/// context's live double-pinyin scheme; `FORCE_TONE` demands exactly
/// three bytes, and a third `1..=5` byte is the tone under `USE_TONE`
/// (`pinyin_parser2.cpp:405-530`).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle, `onepinyin` is a string
/// and `onekey` is an out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_parse_double_pinyin(
    instance: *mut PinyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onepinyin.is_null() || onekey.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    let text = oxpinyin_capi_marshal::input::scalar_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(onepinyin) },
    )
    .unwrap_or_default();
    inst.core.parse_one_double_pinyin(&text).is_some_and(|key| {
        // SAFETY: Null-checked above.
        unsafe { *onekey = ChewingKey::from_core(key) };
        true
    })
}

/// Parse one chewing (bopomofo) keystroke string into a key.
///
/// # C signature
/// ```c
/// bool pinyin_parse_chewing(pinyin_instance_t * instance,
///                           const char * onechewing,
///                           ChewingKey * onekey);
/// ```
///
/// The context's live Zhuyin scheme parses the probe after the API's
/// `ZHUYIN_CORRECT_ALL` strip (`options &= ~ZHUYIN_CORRECT_ALL`,
/// `pinyin.cpp:1568-1569` — the caller's corrections never reach the
/// chewing parser). The key is written only on success, like the double
/// entry.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `instance` is a handle, `onechewing` is a
/// string and `onekey` is an out-parameter. The serial-calls rule applies.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_parse_chewing(
    instance: *mut PinyinInstance,
    onechewing: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onechewing.is_null() || onekey.is_null() {
        return false;
    }

    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_ref(instance) };
    let text = oxpinyin_capi_marshal::input::scalar_text(
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

// ── Display getters ──────────────────────────────────────────────────

/// Get the luoma pinyin string of a chewing key.
///
/// # C signature
/// ```c
/// bool pinyin_get_luoma_pinyin_string(pinyin_instance_t * instance,
///                                     ChewingKey * key,
///                                     gchar ** utf8_str);
/// ```
///
/// The LUOMA full-pinyin index spelling; the tone digit is appended even
/// for the first tone (`chewing_key.cpp:91-105`); caller-owned string.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key` is a borrowed object and `utf8_str` is an
/// out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_luoma_pinyin_string(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    // SAFETY: The entry point guarantees Out-parameters and Borrowed objects: a non-NULL output is
    // aligned and writable, and a non-NULL key is valid for reads. Access must not conflict
    // with live borrows.
    unsafe { display_string_getter(key, utf8_str, oxpinyin_core::ChewingKey::luoma_pinyin_text) }
}

/// Get the secondary zhuyin string of a chewing key.
///
/// # C signature
/// ```c
/// bool pinyin_get_secondary_zhuyin_string(pinyin_instance_t * instance,
///                                         ChewingKey * key,
///                                         gchar ** utf8_str);
/// ```
///
/// The `SECONDARY_ZHUYIN` index spelling; like luoma, the first tone gets
/// its digit (`chewing_key.cpp:107-121`); caller-owned string.
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key` is a borrowed object and `utf8_str` is an
/// out-parameter. `_instance` is not read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_secondary_zhuyin_string(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    // SAFETY: The entry point guarantees Out-parameters and Borrowed objects: a non-NULL output is
    // aligned and writable, and a non-NULL key is valid for reads. Access must not conflict
    // with live borrows.
    unsafe {
        display_string_getter(
            key,
            utf8_str,
            oxpinyin_core::ChewingKey::secondary_zhuyin_text,
        )
    }
}

oxpinyin_capi_marshal::display_string_getter! {
    key: ChewingKey, char: GChar
}

/// Whether a chewing key carries no middle and no final.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_is_incomplete(pinyin_instance_t * instance,
///                                      ChewingKey * key);
/// ```
///
/// `CHEWING_ZERO_MIDDLE == m_middle && CHEWING_ZERO_FINAL == m_final`
/// (`pinyin.cpp:2758-2766`). Upstream asserts the tone is zero in the
/// true branch — a toned initial-only key aborts the pin, and the process
/// aborts here too (register row 4).
///
/// # Safety
///
/// Under the [crate-level rules](crate#safety): `key` is a borrowed object. `_instance` is not
/// read.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn pinyin_get_pinyin_is_incomplete(
    _instance: *mut PinyinInstance,
    key: *mut ChewingKey,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2765-2774`).
    if key.is_null() {
        return false;
    }

    // SAFETY: Null-checked above.
    let core = unsafe { *key }.to_core();
    let incomplete = core.middle == 0 && core.final_ == 0;
    // Register row 4, `pinyin.cpp:2769`:
    // `assert(CHEWING_ZERO_TONE == key->m_tone)` on an initial-only key.
    if incomplete && core.tone != 0 {
        std::process::abort();
    }
    incomplete
}
