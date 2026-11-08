//! C string inputs: malformed, non-scalar and NULL text at each facade entry (#1279). Generated from the frozen pin observations.

use crate::bodies::c_string_inputs as b;
use crate::support::{Expect, run_aborting, run_j};

// c-string-input/zhuyin-character-offset-above-unicode: MATCH.
#[test]
fn c_string_input_zhuyin_character_offset_above_unicode() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_character_offset_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-character-offset-above-unicode"),
        raw!(M(&[("length", I(1)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// c-string-input/zhuyin-character-offset-invalid: MATCH.
#[test]
fn c_string_input_zhuyin_character_offset_invalid() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_character_offset_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-character-offset-invalid"),
        raw!(M(&[
            ("length", S("untouched")),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// c-string-input/zhuyin-character-offset-invalid-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn c_string_input_zhuyin_character_offset_invalid_past_matrix() {
    run_aborting(
        "c_string_inputs::c_string_input_zhuyin_character_offset_invalid_past_matrix",
        Expect::aborts(b""),
        |k| {
            b::case(
                k,
                "c-string-input/zhuyin-character-offset-invalid-past-matrix",
            );
        },
    );
}

// c-string-input/zhuyin-character-offset-null: MATCH.
#[test]
fn c_string_input_zhuyin_character_offset_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_character_offset_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-character-offset-null"),
        raw!(M(&[
            ("length", S("untouched")),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// c-string-input/zhuyin-direct-later-invalid: MATCH.
#[test]
fn c_string_input_zhuyin_direct_later_invalid() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_direct_later_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-direct-later-invalid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16780819), I(117440513)]))
        ])),
        None,
    );
}

// c-string-input/zhuyin-direct-same-invalid: MATCH.
#[test]
fn c_string_input_zhuyin_direct_same_invalid() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_direct_same_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-direct-same-invalid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[I(16780819)]))
        ])),
        None,
    );
}

// c-string-input/zhuyin-full-key-apostrophe-invalid: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn c_string_input_zhuyin_full_key_apostrophe_invalid() {
    run_aborting(
        "c_string_inputs::c_string_input_zhuyin_full_key_apostrophe_invalid",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "c-string-input/zhuyin-full-key-apostrophe-invalid");
        },
    );
}

// c-string-input/zhuyin-full-key-malformed: MATCH.
#[test]
fn c_string_input_zhuyin_full_key_malformed() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_full_key_malformed",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-full-key-malformed"),
        raw!(M(&[("key", I(0)), ("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// c-string-input/zhuyin-import-invalid: MATCH.
#[test]
fn c_string_input_zhuyin_import_invalid() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_import_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-import-invalid"),
        raw!(M(&[
            ("found", B(false)),
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[]))
        ])),
        None,
    );
}

// c-string-input/zhuyin-import-null: MATCH.
#[test]
fn c_string_input_zhuyin_import_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_import_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-import-null"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// c-string-input/zhuyin-import-six-byte: MATCH.
#[test]
fn c_string_input_zhuyin_import_six_byte() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_import_six_byte",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-import-six-byte"),
        raw!(M(&[
            ("found", B(true)),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16777217), I(117440513)]))
        ])),
        None,
    );
}

// c-string-input/zhuyin-lookup-above-unicode: MATCH.
#[test]
fn c_string_input_zhuyin_lookup_above_unicode() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_lookup_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-lookup-above-unicode"),
        raw!(M(&[
            ("found", B(true)),
            ("logs", L(&[])),
            ("ret", Nil),
            ("tokens", L(&[I(16777217)]))
        ])),
        None,
    );
}

// c-string-input/zhuyin-lookup-invalid: MATCH.
#[test]
fn c_string_input_zhuyin_lookup_invalid() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_lookup_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-lookup-invalid"),
        raw!(M(&[
            ("found", B(false)),
            ("logs", L(&[])),
            ("ret", Nil),
            ("tokens", L(&[]))
        ])),
        None,
    );
}

// c-string-input/zhuyin-lookup-state-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_zhuyin_lookup_state_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_lookup_state_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-lookup-state-null"),
        raw!(M(&[("count", I(0)), ("logs", L(&[])), ("ret", B(false))])),
        Some(&["count", "logs", "ret"]),
    );
}

// c-string-input/zhuyin-parse_chewing-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_zhuyin_parse_chewing_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_parse_chewing_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-parse_chewing-null"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("untouched key", B(true))
        ])),
        Some(&["logs", "ret", "untouched key"]),
    );
}

// c-string-input/zhuyin-parse_chewing-valid: MATCH.
#[test]
fn c_string_input_zhuyin_parse_chewing_valid() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_parse_chewing_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-parse_chewing-valid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("untouched key", B(false))
        ])),
        None,
    );
}

// c-string-input/zhuyin-parse_full_pinyin-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_zhuyin_parse_full_pinyin_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_parse_full_pinyin_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-parse_full_pinyin-null"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("untouched key", B(true))
        ])),
        Some(&["logs", "ret", "untouched key"]),
    );
}

// c-string-input/zhuyin-parse_more_chewings-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_zhuyin_parse_more_chewings_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_parse_more_chewings_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-parse_more_chewings-null"),
        raw!(M(&[("length", I(0)), ("logs", L(&[])), ("ret", I(0))])),
        Some(&["length", "logs", "ret"]),
    );
}

// c-string-input/zhuyin-parse_more_chewings-valid: MATCH.
#[test]
fn c_string_input_zhuyin_parse_more_chewings_valid() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_parse_more_chewings_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-parse_more_chewings-valid"),
        raw!(M(&[("length", I(3)), ("logs", L(&[])), ("ret", I(3))])),
        None,
    );
}

// c-string-input/zhuyin-parse_more_full_pinyins-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_zhuyin_parse_more_full_pinyins_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_parse_more_full_pinyins_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-parse_more_full_pinyins-null"),
        raw!(M(&[("length", I(0)), ("logs", L(&[])), ("ret", I(0))])),
        Some(&["length", "logs", "ret"]),
    );
}

// c-string-input/zhuyin-segment-state-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_zhuyin_segment_state_null() {
    run_j(
        "c_string_inputs::c_string_input_zhuyin_segment_state_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/zhuyin-segment-state-null"),
        raw!(M(&[
            ("count", I(0)),
            ("logs", L(&[L(&[S("GLib"), I(8)])])),
            ("ret", B(true))
        ])),
        Some(&["count", "logs", "ret"]),
    );
}
