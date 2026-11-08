//! The zhuyin facade's per-entry contracts: single-key parse, keyboard, key rests, phrase results and string getters (#563). Generated from the frozen pin observations.

use crate::bodies::zhuyin_per_entry as b;
use crate::support::{Expect, run_j};

// zhuyin-cursor-offset-range: MATCH.
#[test]
fn zhuyin_cursor_offset_range() {
    run_j(
        "zhuyin_per_entry::zhuyin_cursor_offset_range",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-cursor-offset-range"),
        raw!(M(&[
            ("0", L(&[B(true), I(0)])),
            ("1", L(&[B(true), I(0)])),
            ("2", L(&[B(true), I(0)])),
            ("3", L(&[B(true), I(3)])),
            ("6", L(&[B(true), I(6)])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// zhuyin-in-chewing-keyboard-key: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_key() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_key",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-key"),
        raw!(include!("fixtures/zhuyin_per_entry/c61728f6c7977060.rs")),
        None,
    );
}

// zhuyin-in-chewing-keyboard-no-tone: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_no_tone() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_no_tone",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-no-tone"),
        raw!(M(&[
            ("b'1'", L(&[B(true), L(&[S("ㄅ")])])),
            ("b'3'", L(&[B(false), Nil])),
            ("b's'", L(&[B(true), L(&[S("ㄋ")])])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// zhuyin-in-chewing-keyboard-scheme-2: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_scheme_2() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_scheme_2",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-scheme-2"),
        raw!(M(&[
            ("b'3'", L(&[B(false), Nil])),
            ("b'i'", L(&[B(true), L(&[S("ㄞ")])])),
            ("b's'", L(&[B(true), L(&[S("ㄙ"), S("˙")])])),
            ("b'u'", L(&[B(true), L(&[S("ㄩ")])])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// zhuyin-in-chewing-keyboard-scheme-3: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_scheme_3() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_scheme_3",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-scheme-3"),
        raw!(M(&[
            ("b'3'", L(&[B(true), L(&[S("ㄇ")])])),
            ("b'i'", L(&[B(true), L(&[S("ㄗ")])])),
            ("b's'", L(&[B(true), L(&[S("ㄨ")])])),
            ("b'u'", L(&[B(true), L(&[S("ㄖ")])])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// zhuyin-in-chewing-keyboard-scheme-4: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_scheme_4() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_scheme_4",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-scheme-4"),
        raw!(M(&[
            ("b'3'", L(&[B(true), L(&[S("ㄉ")])])),
            ("b'i'", L(&[B(true), L(&[S("ㄨ")])])),
            ("b's'", L(&[B(true), L(&[S("ㄇ")])])),
            ("b'u'", L(&[B(true), L(&[S("ㄗ")])])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// zhuyin-in-chewing-keyboard-scheme-5: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_scheme_5() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_scheme_5",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-scheme-5"),
        raw!(M(&[
            ("b'3'", L(&[B(true), L(&[S("ˇ")])])),
            ("b'i'", L(&[B(true), L(&[S("ㄞ")])])),
            ("b's'", L(&[B(true), L(&[S("ㄙ")])])),
            ("b'u'", L(&[B(true), L(&[S("ㄩ")])])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// zhuyin-in-chewing-keyboard-scheme-6: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_scheme_6() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_scheme_6",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-scheme-6"),
        raw!(M(&[
            ("b'3'", L(&[B(false), Nil])),
            ("b'i'", L(&[B(true), L(&[S("ㄞ")])])),
            ("b's'", L(&[B(true), L(&[S("ㄙ")])])),
            ("b'u'", L(&[B(true), L(&[S("ㄩ")])])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// zhuyin-in-chewing-keyboard-scheme-8: MATCH.
#[test]
fn zhuyin_in_chewing_keyboard_scheme_9() {
    run_j(
        "zhuyin_per_entry::zhuyin_in_chewing_keyboard_scheme_9",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-in-chewing-keyboard-scheme-9"),
        raw!(include!("fixtures/zhuyin_per_entry/93270f2fd47f04fe.rs")),
        None,
    );
}

// zhuyin-key-rest-positions-out: MATCH.
#[test]
fn zhuyin_key_rest_positions_out() {
    run_j(
        "zhuyin_per_entry::zhuyin_key_rest_positions_out",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-key-rest-positions-out"),
        raw!(include!("fixtures/zhuyin_per_entry/f6bb5819cf6451b3.rs")),
        None,
    );
}

// zhuyin-null-instance-defined-getters: MATCH.
#[test]
fn zhuyin_null_instance_defined_getters() {
    run_j(
        "zhuyin_per_entry::zhuyin_null_instance_defined_getters",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-null-instance-defined-getters"),
        raw!(include!("fixtures/zhuyin_per_entry/85eff09f24d85f78.rs")),
        None,
    );
}

// zhuyin-parse-chewing-key: MATCH.
#[test]
fn zhuyin_parse_chewing_key() {
    run_j(
        "zhuyin_per_entry::zhuyin_parse_chewing_key",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-parse-chewing-key"),
        raw!(include!("fixtures/zhuyin_per_entry/9d428039e6f18746.rs")),
        None,
    );
}

// zhuyin-phrase-result-tokens: MATCH.
#[test]
fn zhuyin_phrase_result_tokens() {
    run_j(
        "zhuyin_per_entry::zhuyin_phrase_result_tokens",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-phrase-result-tokens"),
        raw!(include!("fixtures/zhuyin_per_entry/234869c096071e64.rs")),
        None,
    );
}

// zhuyin-string-getters: MATCH.
#[test]
fn zhuyin_string_getters() {
    run_j(
        "zhuyin_per_entry::zhuyin_string_getters",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-string-getters"),
        raw!(include!("fixtures/zhuyin_per_entry/74130aa2df2d6db1.rs")),
        None,
    );
}

// zhuyin-token-get-phrase-optional-out: MATCH.
#[test]
fn zhuyin_token_get_phrase_optional_out() {
    run_j(
        "zhuyin_per_entry::zhuyin_token_get_phrase_optional_out",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-token-get-phrase-optional-out"),
        raw!(include!("fixtures/zhuyin_per_entry/c1cf57f557128f2e.rs")),
        None,
    );
}
