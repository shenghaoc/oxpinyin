//! Parsing symbols: full pinyin, double pinyin, chewing.

use std::os::raw::c_char;
use std::ptr;

use oxpinyin_core::graph::SegmentGraph;

use crate::ffi::{cstr_to_string, ffi_catch};
use crate::state::{instance_mut, instance_ref};
use crate::types::{GChar, PinyinInstance};

/// Shared batch-parse path: reset the instance, type `text`, store the
/// parsed prefix length, and return it.
///
/// Upstream stores the parser result in `m_parsed_len` on every
/// `pinyin_parse_more_*` call (`pinyin.cpp:1511,1554,1599`); the getter
/// `pinyin_get_parsed_input_length` (`pinyin.cpp:1611-1613`) then reads that
/// stored value. The segment graph's [`SegmentGraph::consumed`] is the
/// byte count of raw input consumed by the parse.
///
/// Resets and clears the candidate snapshot even for empty input, so a
/// prior composition is discarded and the stored length returns to 0.
fn parse_more(instance: *mut PinyinInstance, text: &str) -> usize {
    // SAFETY: `instance` is non-null and was produced by
    // `pinyin_alloc_instance`.
    let inst = unsafe { instance_mut(instance) };
    inst.session.reset();
    inst.candidates.clear();
    let consumed = if text.is_empty() {
        0
    } else {
        match inst.session.type_pinyin(text) {
            Ok(_) => SegmentGraph::build(inst.session.raw_input().as_bytes())
                .map(|graph| graph.consumed())
                .unwrap_or(0),
            Err(_) => 0,
        }
    };
    inst.parsed_len = consumed;
    consumed
}

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
        // SAFETY: `pinyins` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(pinyins) };
        parse_more(instance, &text)
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
        // SAFETY: `pinyins` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(pinyins) };
        parse_more(instance, &text)
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
        // SAFETY: `chewings` is a C string from the caller (null OK).
        let text = unsafe { cstr_to_string(chewings) };
        parse_more(instance, &text)
    })
}

/// Get the parsed length of the input.
///
/// # C signature
/// ```c
/// size_t pinyin_get_parsed_input_length(pinyin_instance_t * instance);
/// ```
///
/// Returns the byte count of raw input consumed by the most recent parse
/// call, `0` before any parse and after [`pinyin_reset`](crate::instance::pinyin_reset),
/// matching upstream `pinyin.cpp:1611-1613` and reset `pinyin.cpp:2692`.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_parsed_input_length(instance: *mut PinyinInstance) -> usize {
    if instance.is_null() {
        return 0;
    }
    ffi_catch(0, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        inst.parsed_len
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
