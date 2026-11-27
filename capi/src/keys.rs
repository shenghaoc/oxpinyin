//! Single-key parsing and key spellings, including Luoma and secondary Zhuyin.
//!
//! [`crate::cursor`] provides the full-pinyin and Zhuyin display getters.
//!
//! The single-key parsers are pure probes of (live options, one string):
//! unlike the `parse_more_*` batch entries they touch no instance parse
//! state — no matrix, no `m_parsed_len` (upstream `pinyin_parse_full_pinyin`
//! `pinyin.cpp:1476-1491`, `pinyin_parse_double_pinyin` `:1523-1534`,
//! `pinyin_parse_chewing` `:1566-1579`). The display getters read the key
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

use crate::ffi::{borrow_or_return, key_text};
use crate::state::instance_ref;
use crate::types::{ChewingKey, GChar, PinyinInstance};

// ── Single-key parsing ───────────────────────────────────────────────

// pinyin.cpp:1484.
/// Parse one full pinyin into a key.
///
/// # C signature
/// ```c
/// bool pinyin_parse_full_pinyin(pinyin_instance_t * instance,
///                               const char * onepinyin,
///                               ChewingKey * onekey);
/// ```
///
/// Returns true and writes the key on success. A failed parse writes a zero key. A null argument
/// returns false before writing.
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
pub unsafe extern "C" fn pinyin_parse_full_pinyin(
    instance: *mut PinyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    let inst = borrow_or_return!(instance, instance_ref, false, onepinyin, onekey);
    let text = oxpinyin::facade::scalar_text(
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
        .parse_one_full_pinyin(&text, oxpinyin::facade::PinyinCorrections::FromOptions)
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
    let inst = borrow_or_return!(instance, instance_ref, false, onepinyin, onekey);
    let text = oxpinyin::facade::scalar_text(
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
/// `pinyin.cpp:1573` — the caller's corrections never reach the
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
    let inst = borrow_or_return!(instance, instance_ref, false, onechewing, onekey);
    let text = oxpinyin::facade::scalar_text(
        // SAFETY: Strings guarantees valid, unchanged storage.
        // The borrow ends in this statement.
        unsafe { crate::ffi::borrow_c_string(onechewing) },
    )
    .unwrap_or_default();
    inst.core
        .parse_one_chewing(&text, oxpinyin::facade::ChewingCorrections::Masked)
        .is_some_and(|key| {
            // SAFETY: Null-checked above.
            unsafe { *onekey = ChewingKey::from_core(key) };
            true
        })
}

// ── Display getters ──────────────────────────────────────────────────

// chewing_key.cpp:91-105.
/// Get the luoma pinyin string of a chewing key.
///
/// # C signature
/// ```c
/// bool pinyin_get_luoma_pinyin_string(pinyin_instance_t * instance,
///                                     ChewingKey * key,
///                                     gchar ** utf8_str);
/// ```
///
/// Returns true for a key with a spelling. Free the returned string with `g_free`. An unset key
/// writes null and returns false. A null key returns false without writing. A null output
/// returns false. The spelling includes a digit for the first tone.
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
pub unsafe extern "C" fn pinyin_get_luoma_pinyin_string(
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
            oxpinyin::facade::ChewingKey::luoma_pinyin_text,
        )
    }
}

// chewing_key.cpp:107-121.
/// Get the secondary zhuyin string of a chewing key.
///
/// # C signature
/// ```c
/// bool pinyin_get_secondary_zhuyin_string(pinyin_instance_t * instance,
///                                         ChewingKey * key,
///                                         gchar ** utf8_str);
/// ```
///
/// Returns true for a key with a spelling. Free the returned string with `g_free`. An unset key
/// writes null and returns false. A null key returns false without writing. A null output
/// returns false. The spelling includes a digit for the first tone.
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
            oxpinyin::facade::ChewingKey::secondary_zhuyin_text,
        )
    }
}

/// The shared body of the two index-spelling display getters: NULL the
/// out-param, refuse the zero key on `get_table_index() == 0`, then hand
/// out the rendering made with the pin's `g_strdup`/`g_strdup_printf`
/// call (`chewing_key.cpp:97-114`), caller-owned (`g_free`).
///
/// # Safety
///
/// Under the crate-level rules, Out-parameters and Borrowed objects: a
/// non-NULL output is aligned and writable, and a non-NULL key is valid
/// for reads. Access must not conflict with live borrows.
unsafe fn display_string_getter(
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
    render: impl FnOnce(oxpinyin::facade::ChewingKey) -> oxpinyin::facade::KeyText,
) -> bool {
    // The pin never reads `instance` (`pinyin.cpp:2729-2749`); it writes
    // `*utf8_str` before any check, so a NULL out stays guarded (row 41).
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

// pinyin.cpp:2765-2774.
/// Check whether a chewing key has neither a middle nor a final.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_is_incomplete(pinyin_instance_t * instance,
///                                      ChewingKey * key);
/// ```
///
/// Returns true when both the middle and final are zero. A null key returns false.
///
/// # Aborts
///
/// A key with neither a middle nor a final and with a nonzero tone aborts (`pinyin.cpp:2769`).
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
