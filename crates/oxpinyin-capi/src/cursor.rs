//! Pinyin key access and cursor/offset navigation.
//!
//! The cursor → lookup-offset normalization (D1) and the word-level
//! left/right moves (D2) port the pin's matrix laws
//! (`pinyin.cpp:3008-3095` at the pin) over the engine's positional
//! data — `oxpinyin_engine::lookup_offset_for_cursor` and the
//! `*_word_offset` pair. Where the pin's `_check_offset` aborts
//! (`pinyin.cpp:2175`), these answer `false` per the no-abort policy
//! (`docs/findings/upstream-divergences.md`).
//!
//! Parse-mode dispatch mirrors `CapiInstance::validate_lookup_offset`:
//! plain full pinyin runs the law over the session's own buffer; LUOMA /
//! SECONDARY_ZHUYIN run it over the stored original input with the index
//! parse's key spans (the pinned index parse consumes `'` as the same
//! separator); double pinyin and the zhuyin keyboards hold no zero-key
//! columns, so the law steps their parse's key spans only.

use std::ptr;

use oxpinyin_core::phonetic_initial;
use oxpinyin_engine::EngineError;

use crate::ffi::{ffi_catch, owned_cstr};
use crate::state::{CapiInstance, instance_mut, instance_ref};
use crate::types::{ChewingKey, ChewingKeyRest, GChar, PinyinInstance};

/// Get the pinyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest(pinyin_instance_t * instance,
///                                 size_t offset,
///                                 ChewingKeyRest ** key_rest);
/// ```
///
/// Out-param `key_rest` borrows a per-instance slot, valid until the next
/// call on the same instance. Answers at exactly the offsets
/// [`pinyin_get_pinyin_key`] does.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest(
    instance: *mut PinyinInstance,
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
        // `pinyin_alloc_instance`.
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

/// Get the begin/end byte positions of a pinyin key rest.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest_positions(pinyin_instance_t * instance,
///                                           ChewingKeyRest * key_rest,
///                                           guint16 * begin,
///                                           guint16 * end);
/// ```
///
/// Either `begin` or `end` may be NULL to skip. The pin answers `true`
/// unconditionally and dereferences `key_rest` without a null check
/// (`pinyin.cpp`); a null one answers `false` here rather than crashing.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest_positions(
    instance: *mut PinyinInstance,
    key_rest: *mut ChewingKeyRest,
    begin: *mut u16,
    end: *mut u16,
) -> bool {
    if instance.is_null() || key_rest.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Non-null and produced by `pinyin_get_pinyin_key_rest`.
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

/// Get the raw byte length of a pinyin key rest.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest_length(pinyin_instance_t * instance,
///                                        ChewingKeyRest * key_rest,
///                                        guint16 * length);
/// ```
///
/// The pin's `key_rest->length()` — `m_raw_end - m_raw_begin`
/// (`chewing_key.h:111-113`) — and, like the pin, `true` whenever it can
/// answer. fcitx branches on this being 2 to pick its shuangpin rendering
/// (`eim.cpp:473`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest_length(
    instance: *mut PinyinInstance,
    key_rest: *mut ChewingKeyRest,
    length: *mut u16,
) -> bool {
    if instance.is_null() || key_rest.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Non-null and produced by `pinyin_get_pinyin_key_rest`.
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

/// Render a pinyin key as its full spelling.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_string(pinyin_instance_t * instance,
///                               ChewingKey * key,
///                               gchar ** utf8_str);
/// ```
///
/// Out-param `utf8_str` is caller-owned (`g_free`). The pin answers
/// `false` with a NULL out-param for a key whose table index is 0 — the
/// unset key — which an unpopulated slot reproduces.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_string(
    instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    render_key(instance, key, utf8_str, |text, _| Some(text.to_owned()))
}

/// Render a pinyin key as its Zhuyin spelling.
///
/// # C signature
/// ```c
/// bool pinyin_get_zhuyin_string(pinyin_instance_t * instance,
///                               ChewingKey * key,
///                               gchar ** utf8_str);
/// ```
///
/// Out-param `utf8_str` is caller-owned (`g_free`). fcitx treats a NULL
/// out-param as its "something like xi'" break condition
/// (`eim.cpp:512-515`), which the `false` return preserves.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_zhuyin_string(
    instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
) -> bool {
    render_key(
        instance,
        key,
        utf8_str,
        oxpinyin_core::zhuyin_display_for_pinyin,
    )
}

/// Render a pinyin key as its shengmu / yunmu pair.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_strings(pinyin_instance_t * instance,
///                                ChewingKey * key,
///                                gchar ** shengmu,
///                                gchar ** yunmu);
/// ```
///
/// Either out-param may be NULL to skip; both are caller-owned
/// (`g_free`). A syllable with no initial answers an empty shengmu, which
/// is the case fcitx substitutes an apostrophe for (`eim.cpp:478-479`).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_strings(
    instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    shengmu: *mut *mut GChar,
    yunmu: *mut *mut GChar,
) -> bool {
    if instance.is_null() || key.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: Non-null and produced by `pinyin_get_pinyin_key`.
        let slot = unsafe { &*key };
        let Some(text) = slot.key else {
            return false;
        };
        let initial = phonetic_initial(text).unwrap_or("");
        if !shengmu.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *shengmu = owned_cstr(initial);
            }
        }
        if !yunmu.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *yunmu = owned_cstr(&text[initial.len()..]);
            }
        }
        true
    })
}

/// The shared body of the single-string renderers.
fn render_key(
    instance: *mut PinyinInstance,
    key: *mut ChewingKey,
    utf8_str: *mut *mut GChar,
    render: impl Fn(&'static str, u8) -> Option<String>,
) -> bool {
    if instance.is_null() || key.is_null() {
        return false;
    }
    if !utf8_str.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *utf8_str = ptr::null_mut();
        }
    }
    ffi_catch(false, || {
        // SAFETY: Non-null and produced by `pinyin_get_pinyin_key`.
        let slot = unsafe { &*key };
        let Some(text) = slot.key else {
            return false;
        };
        let Some(rendered) = render(text, slot.tone) else {
            return false;
        };
        if !utf8_str.is_null() {
            // SAFETY: Null-checked above.
            unsafe {
                *utf8_str = owned_cstr(&rendered);
            }
        }
        true
    })
}

/// The active parse mode's span source/// The active parse mode's span source: the coordinate input bytes, its
/// parsed length, the key spans (start, end), and whether `'` is a zero-key
/// separator in that mode. `None` for plain full pinyin, whose law runs
/// over the session's own buffer.
struct SpanSource<'a> {
    input: &'a [u8],
    parsed: usize,
    spans: Vec<(usize, usize)>,
    separators: bool,
}

/// The mode dispatch shared by [`lookup_offset`], [`left_offset`] and
/// [`right_offset`]: zhuyin, then double pinyin, then the LUOMA /
/// SECONDARY_ZHUYIN full-pinyin index — the same precedence as
/// `CapiInstance::validate_lookup_offset`. Zhuyin and double pinyin hold
/// no zero-key columns (`separators` false); the index parse consumes `'`
/// as a separator (`separators` true). Plain full pinyin answers `None`.
fn span_source(inst: &CapiInstance) -> Option<SpanSource<'_>> {
    if let Some(parse) = inst.zhuyin_parse.as_ref() {
        Some(SpanSource {
            input: inst.zhuyin_input.as_bytes(),
            parsed: parse.consumed(),
            spans: parse
                .keys()
                .iter()
                .map(|key| (key.start(), key.end()))
                .collect(),
            separators: false,
        })
    } else if let Some(parse) = inst.double_parse.as_ref() {
        Some(SpanSource {
            input: inst.double_input.as_bytes(),
            parsed: parse.consumed(),
            spans: parse
                .keys()
                .iter()
                .map(|key| (key.start(), key.end()))
                .collect(),
            separators: false,
        })
    } else {
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
}

/// The cursor → lookup-offset law in the instance's active parse mode.
///
/// Plain full pinyin walks the session's own scan matrix; the index-parsed
/// schemes walk the index parse's key spans over the stored original
/// input; double pinyin and zhuyin hold no zero-key columns and step the
/// parse's key spans in original coordinates.
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

/// The word-level left-move law in the instance's active parse mode —
/// [lookup_offset]'s mode dispatch applied to the engine's
/// `left_word_offset` law.
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

/// The word-level right-move law in the instance's active parse mode —
/// [lookup_offset]'s mode dispatch applied to the engine's
/// `right_word_offset` law. `Ok(None)` is the pin's one graceful
/// false (`pinyin.cpp:3085-3086`): no key starts at the
/// (zero-run-skipped) position.
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

/// Get the lookup offset from a user cursor position.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_offset(pinyin_instance_t * instance,
///                               size_t cursor,
///                               size_t * offset);
/// ```
///
/// The pin clamps the cursor to the parsed length, walks back to the
/// nearest non-empty matrix column, extends back over the zero-key run
/// before it, and validates (`pinyin.cpp:3008-3027`). A mid-syllable
/// cursor normalizes to the syllable start; the validation abort is
/// answered as `false` (the no-abort policy).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_offset(
    instance: *mut PinyinInstance,
    cursor: usize,
    offset: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
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
/// bool pinyin_get_left_pinyin_offset(pinyin_instance_t * instance,
///                                    size_t offset,
///                                    size_t * left);
/// ```
///
/// Steps syllable-to-syllable: the start of the key ending at `offset`
/// (0 when no key ends there), zero-run-normalized and validated
/// (`pinyin.cpp:3029-3059`). The pin's validation aborts are answered
/// as `false` (the no-abort policy).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_left_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    left: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
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
/// bool pinyin_get_right_pinyin_offset(pinyin_instance_t * instance,
///                                     size_t offset,
///                                     size_t * right);
/// ```
///
/// Steps syllable-to-syllable: the raw end of the first key starting at
/// `offset`, skipping a leading lone-zero-key run first
/// (`pinyin.cpp:3061-3094`). Answers `false` when no key starts there
/// (the pin's own graceful false, `pinyin.cpp:3085-3086`), and where
/// the pin's validation aborts (the no-abort policy).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_right_pinyin_offset(
    instance: *mut PinyinInstance,
    offset: usize,
    right: *mut usize,
) -> bool {
    if instance.is_null() {
        return false;
    }
    ffi_catch(false, || {
        // SAFETY: `instance` is non-null and was produced by
        // `pinyin_alloc_instance`.
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

#[cfg(test)]
mod tests {
    use crate::config::pinyin_set_options;
    use crate::parse::pinyin_parse_more_full_pinyins;
    use crate::test_support::{TempUserDir, cstr, open};

    /// The parity word: `PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
    /// USE_RESPLIT_TABLE` plus the harness's `0x2` bit — the
    /// profile the pin tables were measured under.
    const PARITY: u32 = 0x18a;

    fn parity_instance(
        tag: &str,
    ) -> (
        *mut crate::types::PinyinContext,
        *mut crate::types::PinyinInstance,
        TempUserDir,
    ) {
        let user_dir = TempUserDir::new(tag);
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(pinyin_set_options(context, PARITY));
        (context, instance, user_dir)
    }

    fn parse(instance: *mut crate::types::PinyinInstance, text: &str) -> usize {
        let input = cstr(text);
        pinyin_parse_more_full_pinyins(instance, input.as_ptr())
    }

    #[test]
    fn cursor_table_matches_the_pinned_oracle() {
        // Measured first-hand on the rebuilt pin (fork-per-probe driver,
        // parity word 0x18a): mid-syllable cursors normalize to the
        // syllable start, and the word moves step syllable-to-syllable.
        let (context, instance, _user_dir) = parity_instance("cursor-table");
        assert_eq!(parse(instance, "nihaoshijie"), 11);

        let mut offset = usize::MAX;
        for (cursor, expected) in [
            (0, 0),
            (1, 0),
            (2, 2),
            (3, 2),
            (4, 2),
            (5, 5),
            (6, 5),
            (7, 5),
            (8, 8),
            (9, 8),
            (10, 10),
            (11, 11),
        ] {
            assert!(super::pinyin_get_pinyin_offset(
                instance,
                cursor,
                &mut offset
            ));
            assert_eq!(offset, expected, "cursor {cursor}");
        }

        let (mut left, mut right) = (usize::MAX, usize::MAX);
        for (cursor, expected_left, expected_right) in [(0, 0, 2), (2, 0, 5), (5, 2, 8)] {
            assert!(super::pinyin_get_pinyin_offset(
                instance,
                cursor,
                &mut offset
            ));
            assert!(super::pinyin_get_left_pinyin_offset(
                instance, offset, &mut left
            ));
            assert!(super::pinyin_get_right_pinyin_offset(
                instance, offset, &mut right
            ));
            assert_eq!(
                (left, right),
                (expected_left, expected_right),
                "cursor {cursor}"
            );
        }

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn word_moves_answer_false_where_the_pin_aborts_or_has_no_key() {
        // The pin aborts at offset 11 of nihaoshijie (the second
        // _check_offset on the trailing zero key, pinyin.cpp:3090) and
        // answers false at mid-syllable offsets (no key starts there);
        // the no-abort policy answers false for both shapes.
        let (context, instance, _user_dir) = parity_instance("cursor-aborts");
        assert_eq!(parse(instance, "nihaoshijie"), 11);

        let mut right = usize::MAX;
        assert!(!super::pinyin_get_right_pinyin_offset(
            instance, 11, &mut right
        ));
        for offset in [1, 3, 4, 6, 7, 9] {
            assert!(!super::pinyin_get_right_pinyin_offset(
                instance, offset, &mut right
            ));
        }
        // The pin answers left=10 at offset 11 (its walk halts at column
        // 10 before the trailing zero) — not an abort shape.
        let mut left = usize::MAX;
        assert!(super::pinyin_get_left_pinyin_offset(
            instance, 11, &mut left
        ));
        assert_eq!(left, 10);
        // Offsets past one-past-end: the range refusal.
        assert!(!super::pinyin_get_left_pinyin_offset(
            instance, 12, &mut left
        ));

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }

    #[test]
    fn separator_inputs_match_the_pinned_oracle() {
        // ni'hao: cursor 3 normalizes back over the apostrophe-run zero
        // key to 2; the left move at offset 3 sits one past that zero
        // (the abort shape — answered false); the right move at offset 2
        // skips the zero run to the end of hao.
        let (context, instance, _user_dir) = parity_instance("cursor-separator");
        assert_eq!(parse(instance, "ni'hao"), 6);

        let mut offset = usize::MAX;
        assert!(super::pinyin_get_pinyin_offset(instance, 3, &mut offset));
        assert_eq!(offset, 2);
        let mut left = usize::MAX;
        assert!(!super::pinyin_get_left_pinyin_offset(
            instance, 3, &mut left
        ));
        let mut right = usize::MAX;
        assert!(super::pinyin_get_right_pinyin_offset(
            instance, 2, &mut right
        ));
        assert_eq!(right, 6);

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}

// ── The preedit key family ───────────────────────────────────────────────
//
// fcitx-libpinyin's preedit renderer (`eim.cpp:419-520`) walks the parsed
// input calling `pinyin_get_pinyin_key` at each offset, uses a `false`
// return as its loop terminator, and renders every key through the string
// functions below. Its loop advances with `pinyin_get_right_pinyin_offset`,
// so the offsets it asks about are always syllable-aligned; the
// empty-column `false` path exists for callers that walk byte by byte.
//
// `ChewingKey` and `ChewingKeyRest` are opaque to consumers — the shipped
// `pinyin.h` forward-declares both and `chewing_key.h` is not installed —
// so what must match the pin is the observable output: the boolean, the
// two `guint16`s, and the rendered strings.

/// One matrix key at an offset: its canonical pinyin spelling, its tone,
/// and its raw span.
///
/// The spelling rather than a `SyllableKey` because all three renderers
/// want text, and because the LUOMA / SECONDARY_ZHUYIN index parse carries
/// a canonical spelling rather than a vocabulary key.
struct KeyAt {
    text: &'static str,
    tone: u8,
    begin: usize,
    end: usize,
}

/// The parse's keys as `(text, tone, syllable start, raw end)`, the active
/// mode's own input buffer, and whether `'` is a zero-key separator in that
/// mode — the same `(input, separators)` dispatch [`span_source`] and
/// [`CapiInstance::validate_lookup_offset`] make. The key spans are in the
/// active input's coordinates, so [`key_at`] must walk that same buffer, not
/// the session's `'`-joined canonical spelling. Zhuyin and double pinyin
/// carry `'` as content or not at all (`separators` false); plain full
/// pinyin and the LUOMA / SECONDARY_ZHUYIN index parse consume it as a
/// separator (`separators` true).
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
    if let Some(parse) = inst.double_parse.as_ref() {
        return Ok((
            parse
                .keys()
                .iter()
                .map(|k| KeyAt {
                    text: k.key().text(),
                    tone: 0,
                    begin: k.start(),
                    end: k.end(),
                })
                .collect(),
            inst.double_input.as_bytes(),
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

/// The key the pin's `pinyin_get_pinyin_key` answers at `offset`.
///
/// The pin's three steps (`pinyin.cpp`): refuse `offset >= matrix.size() - 1`
/// (the reserved slot), refuse an empty column, then `_compute_pinyin_start`
/// skips forward over columns holding one lone zero key — a consumed `'`
/// separator — and the answer is that column's first item.
fn key_at(inst: &CapiInstance, offset: usize) -> Option<KeyAt> {
    let (keys, input, separators) = mode_keys(inst).ok()?;
    // matrix.size() is input.len() + 1; the last column is the reserved slot.
    // `input` and the key spans share one coordinate space — the active
    // mode's own buffer — so the separator walk reads it, not the session's
    // `'`-joined canonical spelling.
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
        // A lone zero-key column is a consumed separator; the pin walks past
        // the run. Only the separator modes hold one — zhuyin and double
        // pinyin carry `'` as content or not at all, so their empty columns
        // end the walk. Anything else is an empty mid-syllable column.
        if separators && input.get(at).copied() == Some(b'\'') && at + 1 < input.len() {
            at += 1;
            continue;
        }
        return None;
    }
}

/// Get the pinyin key at an offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key(pinyin_instance_t * instance,
///                            size_t offset,
///                            ChewingKey ** key);
/// ```
///
/// Out-param `key` borrows a per-instance slot, valid until the next call
/// on the same instance. The pin hands out a function-local `static`
/// instead — one process-wide slot — which is observably identical for the
/// documented use and unsound for any other.
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key(
    instance: *mut PinyinInstance,
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
        // `pinyin_alloc_instance`.
        let inst = unsafe { instance_mut(instance) };
        let Some(found) = key_at(inst, offset) else {
            return false;
        };
        inst.key_slot.key = Some(found.text);
        inst.key_slot.tone = found.tone;
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

#[cfg(test)]
mod preedit_key_tests {
    use std::ptr;

    use crate::config::pinyin_set_options;
    use crate::parse::pinyin_parse_more_full_pinyins;
    use crate::test_support::{TempUserDir, cstr, open};
    use crate::types::{ChewingKey, ChewingKeyRest};

    const PARITY: u32 = 0x18a;

    /// The renderers allocate with `owned_cstr`; `take_owned_cstr` is the
    /// matching deallocator, so a leak or a mismatched free would surface
    /// here rather than in a consumer.
    fn take(p: *mut crate::types::GChar) -> String {
        assert!(!p.is_null(), "renderer answered true with a NULL out-param");
        crate::ffi::take_owned_cstr(p)
    }

    /// The expectation table established from the pin in
    /// `docs/findings/preedit-key-accessor-phase1.md` §d: `nihao` parses as
    /// `ni|hao`, so keys start at byte 0 and byte 2 and every other offset
    /// answers `false` — offsets 1/3/4 are empty mid-syllable columns and 5
    /// is the reserved slot (`offset >= matrix.size() - 1`).
    #[test]
    fn nihao_preedit_family_matches_the_pin_expectation_table() {
        let user_dir = TempUserDir::new("preedit-nihao");
        let (context, instance) = open(user_dir.path.to_str().expect("UTF-8 path"));
        assert!(pinyin_set_options(context, PARITY));
        let input = cstr("nihao");
        assert_eq!(pinyin_parse_more_full_pinyins(instance, input.as_ptr()), 5);

        let expected: [(usize, &str, u16, u16, &str, &str, &str); 2] = [
            (0, "ni", 0, 2, "n", "i", "ㄋㄧ"),
            (2, "hao", 2, 5, "h", "ao", "ㄏㄠ"),
        ];

        for (offset, pinyin, begin, end, shengmu, yunmu, zhuyin) in expected {
            let mut key: *mut ChewingKey = ptr::null_mut();
            assert!(
                super::pinyin_get_pinyin_key(instance, offset, &raw mut key),
                "offset {offset} starts a key"
            );
            assert!(!key.is_null());

            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            assert!(super::pinyin_get_pinyin_key_rest(
                instance,
                offset,
                &raw mut rest
            ));
            let (mut b, mut e) = (0_u16, 0_u16);
            assert!(super::pinyin_get_pinyin_key_rest_positions(
                instance, rest, &raw mut b, &raw mut e
            ));
            assert_eq!((b, e), (begin, end), "raw span at offset {offset}");

            let mut len = 0_u16;
            assert!(super::pinyin_get_pinyin_key_rest_length(
                instance,
                rest,
                &raw mut len
            ));
            assert_eq!(len, end - begin, "rest length at offset {offset}");

            let mut s: *mut crate::types::GChar = ptr::null_mut();
            assert!(super::pinyin_get_pinyin_string(instance, key, &raw mut s));
            assert_eq!(take(s), pinyin);

            let (mut sm, mut ym): (*mut crate::types::GChar, *mut crate::types::GChar) =
                (ptr::null_mut(), ptr::null_mut());
            assert!(super::pinyin_get_pinyin_strings(
                instance,
                key,
                &raw mut sm,
                &raw mut ym
            ));
            assert_eq!((take(sm), take(ym)), (shengmu.to_owned(), yunmu.to_owned()));

            let mut z: *mut crate::types::GChar = ptr::null_mut();
            assert!(super::pinyin_get_zhuyin_string(instance, key, &raw mut z));
            assert_eq!(take(z), zhuyin);
        }

        for offset in [1_usize, 3, 4, 5] {
            let mut key: *mut ChewingKey = ptr::null_mut();
            assert!(
                !super::pinyin_get_pinyin_key(instance, offset, &raw mut key),
                "offset {offset} starts no key"
            );
            assert!(key.is_null(), "a false answer nulls the out-param");
            let mut rest: *mut ChewingKeyRest = ptr::null_mut();
            assert!(!super::pinyin_get_pinyin_key_rest(
                instance,
                offset,
                &raw mut rest
            ));
        }

        crate::instance::pinyin_free_instance(instance);
        crate::context::pinyin_fini(context);
    }
}
