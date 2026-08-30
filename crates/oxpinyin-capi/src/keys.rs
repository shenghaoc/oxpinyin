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
use std::sync::atomic::Ordering;

use oxpinyin_core::{DoublePinyinParser, FullPinyinParser, ZHUYIN_CORRECT_ALL, ZhuyinParser};

use crate::ffi::{cstr_to_string, ffi_catch, owned_cstr};
use crate::parse::{double_scheme, zhuyin_scheme};
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
/// the FORCE_TONE law inside `USE_TONE`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_full_pinyin(
    instance: *mut PinyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onepinyin.is_null() || onekey.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // SAFETY: Null-checked above.
        let text = unsafe { cstr_to_string(onepinyin) };
        // SAFETY: Null-checked above; written through the caller's
        // storage exactly once per branch.
        unsafe {
            *onekey = ChewingKey::ZERO;
        }
        match FullPinyinParser.parse_one_key(inst.options().bits(), text.as_bytes()) {
            Some(key) => {
                // SAFETY: Null-checked above.
                unsafe { *onekey = ChewingKey::from_core(key) };
                true
            }
            None => false,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_double_pinyin(
    instance: *mut PinyinInstance,
    onepinyin: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onepinyin.is_null() || onekey.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        // SAFETY: Null-checked above.
        let text = unsafe { cstr_to_string(onepinyin) };
        let scheme = double_scheme(inst.double_scheme.load(Ordering::Relaxed));
        let Some(scheme) = scheme else {
            return false;
        };
        match DoublePinyinParser::with_scheme(scheme)
            .parse_one_key(inst.options().bits(), text.as_bytes())
        {
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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_parse_chewing(
    instance: *mut PinyinInstance,
    onechewing: *const c_char,
    onekey: *mut ChewingKey,
) -> bool {
    if instance.is_null() || onechewing.is_null() || onekey.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
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

// ── Display getters ──────────────────────────────────────────────────

/// Renders `text` into a freshly allocated, caller-owned string at `out`
/// (`owned_cstr`'s libc-`malloc` buffer, releasable with `g_free`).
///
/// # Safety
///
/// `out` must be non-null and writable.
///
/// Returns `false` without touching `out` when the allocation fails
/// (upstream would return `true` with NULL there — the glib OOM corner;
/// `false` is the honest answer under the no-abort policy).
unsafe fn write_string(out: *mut *mut GChar, text: &str) -> bool {
    let rendered = owned_cstr(text);
    // SAFETY: The caller guarantees `out` is writable.
    unsafe {
        *out = rendered;
    }
    !rendered.is_null()
}

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
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_luoma_pinyin_string(
    instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    display_string_getter(instance, key, utf8_str, |core| core.luoma_pinyin_string())
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
/// The SECONDARY_ZHUYIN index spelling; like luoma, the first tone gets
/// its digit (`chewing_key.cpp:107-121`); caller-owned string.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_secondary_zhuyin_string(
    instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    display_string_getter(instance, key, utf8_str, |core| {
        core.secondary_zhuyin_string()
    })
}

/// The shared body of the four single-string display getters: NULL the
/// out-param, refuse the zero key on `get_table_index() == 0`, render.
fn display_string_getter(
    instance: *mut PinyinInstance,
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
        // SAFETY: Null-checked above.
        unsafe { write_string(utf8_str, &render(core)) }
    })
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
/// true branch — a toned initial-only key aborts the pin; the no-abort
/// policy answers `true` (the divergence recorded for the toned
/// initial-only key family in `docs/findings/upstream-divergences.md`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_is_incomplete(
    instance: *mut PinyinInstance,
    key: *mut ChewingKey,
) -> bool {
    if instance.is_null() || key.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Null-checked above.
        let core = unsafe { *key }.to_core();
        core.middle == 0 && core.final_ == 0
    })
}
