//! Parsing symbols: full pinyin, chewing (bopomofo), and the shared
//! batch-parse shell.

use std::os::raw::c_char;
use std::ptr;
use std::sync::atomic::Ordering;

use oxpinyin_core::{FullPinyinScheme, USE_TONE, ZHUYIN_INCOMPLETE, ZhuyinKey, ZhuyinScheme};

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, ZhuyinInstance};

/// The full-pinyin scheme dispatch for `zhuyin_get_pinyin_string`.
pub(crate) fn full_scheme(value: i32) -> Option<FullPinyinScheme> {
    match value {
        1 => Some(FullPinyinScheme::Hanyu),
        2 => Some(FullPinyinScheme::Luoma),
        3 => Some(FullPinyinScheme::SecondaryZhuyin),
        _ => None,
    }
}

/// The zhuyin scheme dispatch.
pub(crate) fn zhuyin_scheme(value: i32) -> Option<ZhuyinScheme> {
    match value {
        1 => Some(ZhuyinScheme::Standard),
        2 => Some(ZhuyinScheme::Hsu),
        3 => Some(ZhuyinScheme::Ibm),
        4 => Some(ZhuyinScheme::Ginyieh),
        5 => Some(ZhuyinScheme::Eten),
        6 => Some(ZhuyinScheme::Eten26),
        7 => Some(ZhuyinScheme::StandardDvorak),
        8 => Some(ZhuyinScheme::HsuDvorak),
        9 => Some(ZhuyinScheme::DachenCp26),
        _ => None,
    }
}

/// Builds the exact-decoder input for a scheme parse: the `'`-joined
/// full-pinyin text plus one [`ExactSegment`] per key over that text.
fn exact_input(keys: &[&ZhuyinKey]) -> (String, Vec<oxpinyin_core::graph::ExactSegment>) {
    let mut text = String::new();
    let mut segments = Vec::with_capacity(keys.len());
    for key in keys {
        if !text.is_empty() {
            text.push('\'');
        }
        let start = text.len();
        text.push_str(key.key().text());
        segments.push(oxpinyin_core::graph::ExactSegment::new(
            start,
            text.len(),
            key.key(),
            key.tone(),
        ));
    }
    (text, segments)
}

/// Zhuyin batch-parse path (`zhuyin_parse_more_chewings`).
///
/// Mirrors `oxpinyin-capi`'s chewing path, delegating to
/// [`oxpinyin_core::ZhuyinParser::parse`] and driving the session decoder
/// with the `'`-joined full-pinyin spelling.
fn parse_chewing_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.begin_parse(text.as_bytes());

    let Some(scheme) = zhuyin_scheme(inst.zhuyin_scheme.load(Ordering::Relaxed)) else {
        return 0;
    };
    let use_tone = inst.use_tone.load(Ordering::Relaxed);
    let allow_incomplete = inst.options().contains(ZHUYIN_INCOMPLETE);
    let parser = oxpinyin_core::ZhuyinParser::with_scheme(scheme);
    let parsed = parser.parse(text.as_bytes(), use_tone, allow_incomplete);

    if text.is_empty() {
        inst.parsed_len = 0;
        return 0;
    }

    let keys: Vec<&ZhuyinKey> = parsed.keys().iter().collect();
    let (full, segments) = exact_input(&keys);
    if !full.is_empty() && inst.session.replace_raw_exact(&full, &segments).is_err() {
        return 0;
    }

    inst.parsed_len = parsed.consumed();
    inst.zhuyin_input = text.to_owned();
    inst.zhuyin_parse = Some(parsed);
    inst.parsed_len
}

/// Full-pinyin batch-parse path (`zhuyin_parse_more_full_pinyins`).
fn parse_full_more(instance: *mut ZhuyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `zhuyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.begin_parse(text.as_bytes());
    if inst.session.set_options(inst.options()).is_err() {
        return 0;
    }
    if text.is_empty() {
        return 0;
    }
    // LUOMA / SECONDARY_ZHUYIN: parse the raw input through the scheme's
    // pinned index.
    if let Some(scheme) = full_scheme(inst.full_scheme.load(Ordering::Relaxed))
        && let Some(index) = scheme.index()
    {
        let use_tone = inst.options().contains(USE_TONE);
        let parsed = oxpinyin_core::parse_full_pinyin_index(text.as_bytes(), use_tone, index);
        let full = parsed.full_pinyin();
        if !full.is_empty() && inst.session.replace_raw(&full).is_err() {
            return 0;
        }
        inst.parsed_len = parsed.consumed();
        inst.full_input = text.to_owned();
        inst.full_parse = Some(parsed);
        return inst.parsed_len;
    }
    let consumed = match inst.session.replace_raw(text) {
        Ok(()) => inst.session.full_parsed_len(),
        Err(_) => 0,
    };
    inst.parsed_len = consumed;
    consumed
}

/// Parse multiple full pinyins.
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_full_pinyins(zhuyin_instance_t * instance,
///                                       const char * pinyins);
/// ```
///
/// Returns number of bytes consumed, 0 on failure.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_more_full_pinyins(
    instance: *mut ZhuyinInstance,
    pinyins: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `pinyins` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(pinyins) };
        parse_full_more(instance, &text)
    })
}

/// Parse multiple chewing (bopomofo) inputs.
///
/// # C signature
/// ```c
/// size_t zhuyin_parse_more_chewings(zhuyin_instance_t * instance,
///                                   const char * chewings);
/// ```
///
/// Parses through [`oxpinyin_core::ZhuyinParser`] and drives the session
/// with the apostrophe-joined full-pinyin spelling.
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_parse_more_chewings(
    instance: *mut ZhuyinInstance,
    chewings: *const c_char,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `chewings` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(chewings) };
        parse_chewing_more(instance, &text)
    })
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t zhuyin_get_parsed_input_length(zhuyin_instance_t * instance);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_parsed_input_length(instance: *mut ZhuyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.parsed_len
    })
}

/// Check whether an input key is in the current chewing keyboard scheme.
///
/// # C signature
/// ```c
/// bool zhuyin_in_chewing_keyboard(zhuyin_instance_t * instance,
///                                 const char key,
///                                 gchar *** symbols);
/// ```
///
/// `key` is a plain `char` value (not a pointer).
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_in_chewing_keyboard(
    instance: *mut ZhuyinInstance,
    key: std::os::raw::c_char,
    symbols: *mut *mut *mut GChar,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Some(scheme) = zhuyin_scheme(inst.zhuyin_scheme.load(Ordering::Relaxed)) else {
            return false;
        };
        let use_tone = inst.use_tone.load(Ordering::Relaxed);
        let parser = oxpinyin_core::ZhuyinParser::with_scheme(scheme);
        let mapped = parser.symbols_for(key as u8, use_tone);
        if mapped.is_empty() {
            if !symbols.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *symbols = std::ptr::null_mut();
                }
            }
            return false;
        }
        if !symbols.is_null() {
            let list = crate::ffi::owned_cstr_list(&mapped);
            if list.is_null() {
                // SAFETY: Null-checked above.
                unsafe {
                    *symbols = ptr::null_mut();
                }
                return false;
            }
            // SAFETY: `owned_cstr_list` is a malloc array of malloc strings;
            // the caller releases both with g_strfreev.
            unsafe {
                *symbols = list;
            }
        }
        true
    })
}
