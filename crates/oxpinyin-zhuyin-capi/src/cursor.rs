//! Pinyin key access and cursor/offset navigation.
//!
//! The cursor → lookup-offset normalization and the word-level left/right
//! moves port the pin's matrix laws over the engine's positional data, using
//! the zhuyin parse's key spans. Where the pin's `_check_offset` aborts, these
//! answer `false` per the no-abort policy (divergence class (c)).

use std::ptr;

use oxpinyin_engine::EngineError;

use crate::ffi::ffi_catch;
use crate::state::{CapiInstance, instance_mut, instance_ref};
use crate::types::{ChewingKey, ChewingKeyRest, ZhuyinInstance};

/// Get the zhuyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest(zhuyin_instance_t * instance,
///                                 size_t offset, ChewingKeyRest ** key_rest);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key_rest(
    instance: *mut ZhuyinInstance,
    offset: usize,
    key_rest: *mut *mut ChewingKeyRest,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !key_rest.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *key_rest = ptr::null_mut();
        }
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let Some(found) = key_at(inst, offset) else {
            return false;
        };
        inst.key_rest_slot.begin = u16::try_from(found.begin).unwrap_or(u16::MAX);
        inst.key_rest_slot.end = u16::try_from(found.end).unwrap_or(u16::MAX);
        if !key_rest.is_null() {
            // SAFETY: Null-checked above; the slot lives as long as the
            // instance.
            unsafe {
                *key_rest = &raw mut inst.key_rest_slot;
            }
        }
        true
    })
}

/// Get the begin/end byte positions of a zhuyin key rest.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest_positions(zhuyin_instance_t * instance,
///                                           ChewingKeyRest * key_rest,
///                                           guint16 * begin, guint16 * end);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key_rest_positions(
    instance: *mut ZhuyinInstance,
    key_rest: *mut ChewingKeyRest,
    begin: *mut u16,
    end: *mut u16,
) -> bool {
    if instance.is_null() || key_rest.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Non-null and produced by `zhuyin_get_zhuyin_key_rest`.
        let rest = unsafe { &*key_rest };
        if !begin.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *begin = rest.begin;
            }
        }
        if !end.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *end = rest.end;
            }
        }
        true
    })
}

/// Get the raw byte length of a zhuyin key rest.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key_rest_length(zhuyin_instance_t * instance,
///                                        ChewingKeyRest * key_rest,
///                                        guint16 * length);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key_rest_length(
    instance: *mut ZhuyinInstance,
    key_rest: *mut ChewingKeyRest,
    length: *mut u16,
) -> bool {
    if instance.is_null() || key_rest.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Non-null and produced by `zhuyin_get_zhuyin_key_rest`.
        let rest = unsafe { &*key_rest };
        if !length.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *length = rest.end.saturating_sub(rest.begin);
            }
        }
        true
    })
}

/// Get the zhuyin key at an offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_key(zhuyin_instance_t * instance,
///                            size_t offset, ChewingKey ** key);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_key(
    instance: *mut ZhuyinInstance,
    offset: usize,
    key: *mut *mut ChewingKey,
) -> bool {
    if instance.is_null() {
        return false;
    }
    if !key.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *key = ptr::null_mut();
        }
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let Some(found) = key_at(inst, offset) else {
            return false;
        };
        // `found.text` comes from `mode_keys`, which reads the parsed keys /
        // the session matrix — always a syllable present in the content table —
        // so `from_spelling` cannot fail in practice. Keep the fetch-failure
        // `unwrap_or(ChewingKey::ZERO)` fallback (matching oxpinyin-capi,
        // cursor.rs) rather than propagating lookup failure: a stale matrix
        // key is not a reachable state, and the fallback keeps the ABI's
        // boolean success semantics identical to the pin.
        inst.key_slot =
            ChewingKey::from_spelling(found.text, found.tone).unwrap_or(ChewingKey::ZERO);
        if !key.is_null() {
            // SAFETY: Null-checked above; the slot lives as long as the
            // instance.
            unsafe {
                *key = &raw mut inst.key_slot;
            }
        }
        true
    })
}

/// Get the lookup offset from a user cursor position.
///
/// # C signature
/// ```c
/// bool zhuyin_get_zhuyin_offset(zhuyin_instance_t * instance,
///                               size_t cursor, size_t * offset);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_zhuyin_offset(
    instance: *mut ZhuyinInstance,
    cursor: usize,
    offset: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Ok(normalized) = lookup_offset(inst, cursor) else {
            return false;
        };
        if !offset.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *offset = normalized;
            }
        }
        true
    })
}

/// Get the left offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_left_zhuyin_offset(zhuyin_instance_t * instance,
///                                    size_t offset, size_t * left);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_left_zhuyin_offset(
    instance: *mut ZhuyinInstance,
    offset: usize,
    left: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Ok(result) = left_offset(inst, offset) else {
            return false;
        };
        if !left.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *left = result;
            }
        }
        true
    })
}

/// Get the right offset from a lookup offset.
///
/// # C signature
/// ```c
/// bool zhuyin_get_right_zhuyin_offset(zhuyin_instance_t * instance,
///                                     size_t offset, size_t * right);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn zhuyin_get_right_zhuyin_offset(
    instance: *mut ZhuyinInstance,
    offset: usize,
    right: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `zhuyin_alloc_instance`.
        let inst = unsafe { instance_ref(instance) };
        let Ok(Some(result)) = right_offset(inst, offset) else {
            return false;
        };
        if !right.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *right = result;
            }
        }
        true
    })
}

/// The active parse mode's span source: the coordinate input bytes, its
/// parsed length, the key spans, and whether `'` is a zero-key separator.
struct SpanSource<'a> {
    input: &'a [u8],
    parsed: usize,
    spans: Vec<(usize, usize)>,
    separators: bool,
}

fn span_source(inst: &CapiInstance) -> Option<SpanSource<'_>> {
    if let Some(parse) = inst.zhuyin_parse.as_ref() {
        return Some(SpanSource {
            input: inst.zhuyin_input.as_bytes(),
            parsed: parse.consumed(),
            spans: parse
                .keys()
                .iter()
                .map(|key| (key.start(), key.end()))
                .collect(),
            separators: false,
        });
    }
    inst.full_parse.as_ref().map(|parse| SpanSource {
        input: inst.full_input.as_bytes(),
        parsed: parse.consumed(),
        spans: parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect(),
        separators: true,
    })
}

fn lookup_offset(inst: &CapiInstance, cursor: usize) -> Result<usize, EngineError> {
    match span_source(inst) {
        Some(source) => oxpinyin_engine::lookup_offset_over_spans(
            source.input,
            source.parsed,
            &source.spans,
            source.separators,
            cursor,
        ),
        None => inst.session.lookup_offset_for_cursor(cursor),
    }
}

fn left_offset(inst: &CapiInstance, offset: usize) -> Result<usize, EngineError> {
    match span_source(inst) {
        Some(source) => oxpinyin_engine::left_word_offset_over_spans(
            source.input,
            source.parsed,
            &source.spans,
            source.separators,
            offset,
        ),
        None => inst.session.left_word_offset(offset),
    }
}

fn right_offset(inst: &CapiInstance, offset: usize) -> Result<Option<usize>, EngineError> {
    match span_source(inst) {
        Some(source) => oxpinyin_engine::right_word_offset_over_spans(
            source.input,
            source.parsed,
            &source.spans,
            source.separators,
            offset,
        ),
        None => inst.session.right_word_offset(offset),
    }
}

/// One matrix key at an offset.
struct KeyAt {
    text: &'static str,
    tone: u8,
    begin: usize,
    end: usize,
}

fn mode_keys(inst: &CapiInstance) -> Result<(Vec<KeyAt>, &[u8], bool), EngineError> {
    if let Some(parse) = inst.zhuyin_parse.as_ref() {
        return Ok((
            parse
                .keys()
                .iter()
                .map(|k| KeyAt {
                    text: k.key().text(),
                    tone: k.tone(),
                    begin: k.start(),
                    end: k.end(),
                })
                .collect(),
            inst.zhuyin_input.as_bytes(),
            false,
        ));
    }
    if let Some(parse) = inst.full_parse.as_ref() {
        return Ok((
            parse
                .keys()
                .iter()
                .map(|k| KeyAt {
                    text: k.canonical(),
                    tone: k.tone(),
                    begin: k.start(),
                    end: k.end(),
                })
                .collect(),
            inst.full_input.as_bytes(),
            true,
        ));
    }
    let (keys, _) = inst.session.matrix_keys()?;
    Ok((
        keys.iter()
            .map(|k| KeyAt {
                text: k.key().text(),
                tone: k.tone(),
                begin: k.syllable_start(),
                end: k.end(),
            })
            .collect(),
        inst.session.raw_input().as_bytes(),
        true,
    ))
}

fn key_at(inst: &CapiInstance, offset: usize) -> Option<KeyAt> {
    let (keys, input, separators) = mode_keys(inst).ok()?;
    if offset >= input.len() {
        return None;
    }
    let mut at = offset;
    loop {
        if let Some(found) = keys.iter().find(|k| k.begin == at) {
            return Some(KeyAt {
                text: found.text,
                tone: found.tone,
                begin: found.begin,
                end: found.end,
            });
        }
        if separators && input.get(at).copied() == Some(b'\'') && at + 1 < input.len() {
            at += 1;
            continue;
        }
        return None;
    }
}
