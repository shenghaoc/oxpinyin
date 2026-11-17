#![expect(
    clippy::undocumented_unsafe_blocks,
    reason = "each call passes handles and buffers from this test's own fixture, under the contract in the function's # Safety section"
)]

use oxpinyin_core::OptionBits;

/// The rendered text, as the final `g_strconcat` joins its parts.
fn full_aux_text(raw: &str, parsed_len: usize, cursor: usize, options: OptionBits) -> String {
    super::full_aux_text(raw, parsed_len, cursor, options)
        .expect("a non-empty key path")
        .to_string()
}

#[test]
fn full_aux_text_matches_the_oracle_for_simple_keys() {
    // Captured from the pinned C++ libpinyin 2.11.91 oracle with
    // PINYIN_INCOMPLETE set (`pinyin_get_full_pinyin_auxiliary_text`, `pinyin.cpp:3381-3438`
    // at 074a2219).
    for (cursor, expected) in [
        (0, "|ni hao "),
        (1, "n|i hao "),
        (2, "ni |hao "),
        (3, "ni h|ao "),
        (4, "ni ha|o "),
        (5, "ni hao |"),
        (6, "ni hao |"),
        (99, "ni hao |"),
    ] {
        assert_eq!(
            full_aux_text("nihao", 5, cursor, OptionBits::default()),
            expected,
            "nihao cursor {cursor}"
        );
    }
}

#[test]
fn full_aux_text_carries_the_tone_digit_under_use_tone() {
    // `ChewingKey::get_pinyin_string` (chewing_key.cpp:47-58) renders
    // "%s%d" for non-zero tones and the aux splits that rendered
    // string at the raw-relative cursor (pinyin.cpp:3411-3423) — the
    // cursor can land on the digit itself. Carrying the digit keeps
    // raw and canonical lengths equal, holding the #130 over-read
    // closed on HANYU.
    let options = OptionBits::PINYIN_INCOMPLETE | OptionBits::USE_TONE;
    for (cursor, expected) in [
        (0, "|zai4 "),
        (1, "z|ai4 "),
        (2, "za|i4 "),
        (3, "zai|4 "),
        (4, "zai4 |"),
        (99, "zai4 |"),
    ] {
        assert_eq!(
            full_aux_text("zai4", 4, cursor, options),
            expected,
            "zai4 cursor {cursor}"
        );
    }
    assert_eq!(full_aux_text("ni3hao3", 7, 7, options), "ni3 hao3 |");
    assert_eq!(full_aux_text("zai4'an", 7, 7, options), "zai4 an |");
    assert_eq!(full_aux_text("zai4Q", 4, 4, options), "zai4 |");
}

#[test]
fn full_aux_text_drops_the_digit_when_use_tone_is_clear() {
    // The invariant: with the bit clear the toned inputs render
    // exactly as the frozen toneless parser always did.
    for (raw, parsed, cursor, expected) in [
        ("zai4", 3, 0, "|zai "),
        ("zai4", 3, 3, "zai |"),
        ("ni3hao3", 3, 3, "ni |"),
    ] {
        assert_eq!(
            full_aux_text(raw, parsed, cursor, OptionBits::default()),
            expected,
            "{raw} cursor {cursor}"
        );
    }
}

#[test]
fn full_aux_text_matches_the_oracle_for_apostrophes() {
    // The apostrophe is consumed by the following edge and is never
    // rendered; cursors on the apostrophe byte (2) and on the key start
    // (3) are both the boundary between ni and hao.
    for (cursor, expected) in [
        (0, "|ni hao "),
        (2, "ni |hao "),
        (3, "ni |hao "),
        (4, "ni h|ao "),
        (6, "ni hao |"),
    ] {
        assert_eq!(
            full_aux_text("ni'hao", 6, cursor, OptionBits::default()),
            expected,
            "ni'hao cursor {cursor}"
        );
    }
}

#[test]
fn full_aux_text_matches_the_oracle_for_incomplete_tails() {
    // nih parses as ni + incomplete h with PINYIN_INCOMPLETE set.
    let options = OptionBits::PINYIN_INCOMPLETE;
    for (cursor, expected) in [(0, "|ni h "), (2, "ni |h "), (3, "ni h |"), (4, "ni h |")] {
        assert_eq!(
            full_aux_text("nih", 3, cursor, options),
            expected,
            "nih cursor {cursor}"
        );
    }

    // A bare incomplete initial renders as the initial itself.
    assert_eq!(full_aux_text("n", 1, 0, options), "|n ");
    assert_eq!(full_aux_text("n", 1, 1, options), "n |");
}

#[path = "../../../oxpinyin-core/tests/common/parser_rows.rs"]
mod parser_rows;

parser_rows::gated_parser_rows! {
    fn gated_parser_rows_publish_key_slots_and_auxiliary_text(
        #[case] input: &str,
        #[case] options: u32,
        #[case] parsed: usize,
        #[case] _keys: &[(u16, u16, u16, &str)],
        #[case] aux: &str,
    ) {
        use crate::test_support::{TempUserDir, cstr, key_slot_guard, open};
        let _slots = key_slot_guard();
        let user = TempUserDir::new("gated-row");
        let (context, instance) = open(user.path.to_str().unwrap());
        assert!(unsafe { crate::config::pinyin_set_options(context, options) });
        unsafe { crate::parse::pinyin_parse_more_full_pinyins(instance, cstr(input).as_ptr()) };
        for offset in 0..=parsed {
            let mut key = std::ptr::null_mut();
            let found = unsafe { crate::cursor::pinyin_get_pinyin_key(instance, offset, &raw mut key) };
            assert_eq!(found, !key.is_null(), "offset {offset}: key answer and out-param");
            let mut rest = std::ptr::null_mut();
            let found = unsafe { crate::cursor::pinyin_get_pinyin_key_rest(instance, offset, &raw mut rest) };
            assert_eq!(found, !rest.is_null(), "offset {offset}: rest answer and out-param");
        }
        let mut text = std::ptr::null_mut();
        assert!(unsafe { super::pinyin_get_full_pinyin_auxiliary_text(instance, parsed, &raw mut text) });
        assert_eq!(crate::ffi::take_owned_cstr(text), aux);
        unsafe { crate::instance::pinyin_free_instance(instance) };
        unsafe { crate::context::pinyin_fini(context) };
    }
}
