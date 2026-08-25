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

use oxpinyin_engine::EngineError;

use crate::ffi::ffi_catch;
use crate::state::{CapiInstance, instance_ref};
use crate::types::{ChewingKeyRest, PinyinInstance};

/// Get the pinyin key rest at an offset.
///
/// # C signature
/// ```c
/// bool pinyin_get_pinyin_key_rest(pinyin_instance_t * instance,
///                                 size_t offset,
///                                 ChewingKeyRest ** key_rest);
/// ```
///
/// Out-param `key_rest` is instance-borrowed.
///
/// Provisional: always returns false (no key-rest data structure yet).
#[unsafe(no_mangle)]
pub extern "C" fn pinyin_get_pinyin_key_rest(
    instance: *mut PinyinInstance,
    _offset: usize,
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
    false
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
/// Either `begin` or `end` may be NULL to skip.
///
/// Provisional: always returns false (no key-rest data yet).
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
    if !begin.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *begin = 0;
        }
    }
    if !end.is_null() {
        // SAFETY: Null-checked above.
        unsafe {
            *end = 0;
        }
    }
    false
}

/// The cursor → lookup-offset law in the instance's active parse mode.
///
/// Plain full pinyin walks the session's own scan matrix; the index-parsed
/// schemes walk the index parse's key spans over the stored original
/// input; double pinyin and zhuyin hold no zero-key columns and step the
/// parse's key spans in original coordinates.
fn lookup_offset(inst: &CapiInstance, cursor: usize) -> Result<usize, EngineError> {
    if let Some(parse) = inst.zhuyin_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::lookup_offset_over_spans(
            inst.zhuyin_input.as_bytes(),
            parse.consumed(),
            &spans,
            false,
            cursor,
        )
    } else if let Some(parse) = inst.double_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::lookup_offset_over_spans(
            inst.double_input.as_bytes(),
            parse.consumed(),
            &spans,
            false,
            cursor,
        )
    } else if let Some(parse) = inst.full_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::lookup_offset_over_spans(
            inst.full_input.as_bytes(),
            parse.consumed(),
            &spans,
            true,
            cursor,
        )
    } else {
        inst.session.lookup_offset_for_cursor(cursor)
    }
}

/// The word-level left-move law in the instance's active parse mode —
/// [lookup_offset]'s mode dispatch applied to the engine's
/// `left_word_offset` law.
fn left_offset(inst: &CapiInstance, offset: usize) -> Result<usize, EngineError> {
    if let Some(parse) = inst.zhuyin_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::left_word_offset_over_spans(
            inst.zhuyin_input.as_bytes(),
            parse.consumed(),
            &spans,
            false,
            offset,
        )
    } else if let Some(parse) = inst.double_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::left_word_offset_over_spans(
            inst.double_input.as_bytes(),
            parse.consumed(),
            &spans,
            false,
            offset,
        )
    } else if let Some(parse) = inst.full_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::left_word_offset_over_spans(
            inst.full_input.as_bytes(),
            parse.consumed(),
            &spans,
            true,
            offset,
        )
    } else {
        inst.session.left_word_offset(offset)
    }
}

/// The word-level right-move law in the instance's active parse mode —
/// [lookup_offset]'s mode dispatch applied to the engine's
/// `right_word_offset` law. `Ok(None)` is the pin's one graceful
/// false (`pinyin.cpp:3085-3086`): no key starts at the
/// (zero-run-skipped) position.
fn right_offset(inst: &CapiInstance, offset: usize) -> Result<Option<usize>, EngineError> {
    if let Some(parse) = inst.zhuyin_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::right_word_offset_over_spans(
            inst.zhuyin_input.as_bytes(),
            parse.consumed(),
            &spans,
            false,
            offset,
        )
    } else if let Some(parse) = inst.double_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::right_word_offset_over_spans(
            inst.double_input.as_bytes(),
            parse.consumed(),
            &spans,
            false,
            offset,
        )
    } else if let Some(parse) = inst.full_parse.as_ref() {
        let spans: Vec<(usize, usize)> = parse
            .keys()
            .iter()
            .map(|key| (key.start(), key.end()))
            .collect();
        oxpinyin_engine::right_word_offset_over_spans(
            inst.full_input.as_bytes(),
            parse.consumed(),
            &spans,
            true,
            offset,
        )
    } else {
        inst.session.right_word_offset(offset)
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
