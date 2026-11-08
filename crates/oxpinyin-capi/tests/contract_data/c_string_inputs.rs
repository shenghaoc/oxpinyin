//! C string inputs: malformed, non-scalar and NULL text at each facade entry (#1279). Generated from the frozen pin observations.

use crate::bodies::c_string_inputs as b;
use crate::support::{Expect, run_aborting, run_j};

// c-string-input/pinyin-character-offset-invalid: MATCH.
#[test]
fn c_string_input_pinyin_character_offset_invalid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_character_offset_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-character-offset-invalid"),
        raw!(M(&[
            ("length", S("untouched")),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// c-string-input/pinyin-character-offset-invalid-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn c_string_input_pinyin_character_offset_invalid_past_matrix() {
    run_aborting(
        "c_string_inputs::c_string_input_pinyin_character_offset_invalid_past_matrix",
        Expect::aborts(b""),
        |k| {
            b::case(
                k,
                "c-string-input/pinyin-character-offset-invalid-past-matrix",
            );
        },
    );
}

// c-string-input/pinyin-character-offset-null: MATCH.
#[test]
fn c_string_input_pinyin_character_offset_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_character_offset_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-character-offset-null"),
        raw!(M(&[
            ("length", S("untouched")),
            ("logs", L(&[])),
            ("ret", B(false))
        ])),
        None,
    );
}

// c-string-input/pinyin-character-offset-surrogate: MATCH.
#[test]
fn c_string_input_pinyin_character_offset_surrogate() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_character_offset_surrogate",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-character-offset-surrogate"),
        raw!(M(&[("length", I(1)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// c-string-input/pinyin-character-offset-valid: MATCH.
#[test]
fn c_string_input_pinyin_character_offset_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_character_offset_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-character-offset-valid"),
        raw!(M(&[("length", I(1)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// c-string-input/pinyin-full-key-apostrophe-invalid: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn c_string_input_pinyin_full_key_apostrophe_invalid() {
    run_aborting(
        "c_string_inputs::c_string_input_pinyin_full_key_apostrophe_invalid",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "c-string-input/pinyin-full-key-apostrophe-invalid");
        },
    );
}

// c-string-input/pinyin-full-key-malformed: MATCH.
#[test]
fn c_string_input_pinyin_full_key_malformed() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_full_key_malformed",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-full-key-malformed"),
        raw!(M(&[("key", I(0)), ("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// c-string-input/pinyin-full-key-valid: MATCH.
#[test]
fn c_string_input_pinyin_full_key_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_full_key_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-full-key-valid"),
        raw!(M(&[("key", I(12331)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// c-string-input/pinyin-import-five-byte: MATCH.
#[test]
fn c_string_input_pinyin_import_five_byte() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_import_five_byte",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-import-five-byte"),
        raw!(M(&[
            ("found", B(true)),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16777217), I(117440513)]))
        ])),
        None,
    );
}

// c-string-input/pinyin-import-invalid: MATCH.
#[test]
fn c_string_input_pinyin_import_invalid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_import_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-import-invalid"),
        raw!(M(&[
            ("found", B(false)),
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[]))
        ])),
        None,
    );
}

// c-string-input/pinyin-import-null: MATCH.
#[test]
fn c_string_input_pinyin_import_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_import_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-import-null"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// c-string-input/pinyin-lookup-invalid: MATCH.
#[test]
fn c_string_input_pinyin_lookup_invalid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_lookup_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-lookup-invalid"),
        raw!(M(&[
            ("found", B(false)),
            ("logs", L(&[])),
            ("ret", Nil),
            ("tokens", L(&[]))
        ])),
        None,
    );
}

// c-string-input/pinyin-lookup-state-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_lookup_state_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_lookup_state_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-lookup-state-null"),
        raw!(M(&[("count", I(0)), ("logs", L(&[])), ("ret", B(false))])),
        Some(&["count", "logs", "ret"]),
    );
}

// c-string-input/pinyin-lookup-state-valid: MATCH.
#[test]
fn c_string_input_pinyin_lookup_state_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_lookup_state_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-lookup-state-valid"),
        raw!(M(&[("count", I(1)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// c-string-input/pinyin-lookup-surrogate: MATCH.
#[test]
fn c_string_input_pinyin_lookup_surrogate() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_lookup_surrogate",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-lookup-surrogate"),
        raw!(M(&[
            ("found", B(true)),
            ("logs", L(&[])),
            ("ret", Nil),
            ("tokens", L(&[I(16777217)]))
        ])),
        None,
    );
}

// c-string-input/pinyin-lookup-valid: MATCH.
#[test]
fn c_string_input_pinyin_lookup_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_lookup_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-lookup-valid"),
        raw!(M(&[
            ("found", B(true)),
            ("logs", L(&[])),
            ("ret", Nil),
            ("tokens", L(&[I(16777217)]))
        ])),
        None,
    );
}

// c-string-input/pinyin-parse_chewing-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_parse_chewing_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_chewing_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_chewing-null"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("untouched key", B(true))
        ])),
        Some(&["logs", "ret", "untouched key"]),
    );
}

// c-string-input/pinyin-parse_double_pinyin-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_parse_double_pinyin_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_double_pinyin_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_double_pinyin-null"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("untouched key", B(true))
        ])),
        Some(&["logs", "ret", "untouched key"]),
    );
}

// c-string-input/pinyin-parse_full_pinyin-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_parse_full_pinyin_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_full_pinyin_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_full_pinyin-null"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("untouched key", B(true))
        ])),
        Some(&["logs", "ret", "untouched key"]),
    );
}

// c-string-input/pinyin-parse_full_pinyin-valid: MATCH.
#[test]
fn c_string_input_pinyin_parse_full_pinyin_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_full_pinyin_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_full_pinyin-valid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("untouched key", B(false))
        ])),
        None,
    );
}

// c-string-input/pinyin-parse_more_chewings-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_parse_more_chewings_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_more_chewings_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_more_chewings-null"),
        raw!(M(&[("length", I(0)), ("logs", L(&[])), ("ret", I(0))])),
        Some(&["length", "logs", "ret"]),
    );
}

// c-string-input/pinyin-parse_more_double_pinyins-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_parse_more_double_pinyins_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_more_double_pinyins_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_more_double_pinyins-null"),
        raw!(M(&[("length", I(0)), ("logs", L(&[])), ("ret", I(0))])),
        Some(&["length", "logs", "ret"]),
    );
}

// c-string-input/pinyin-parse_more_full_pinyins-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_parse_more_full_pinyins_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_more_full_pinyins_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_more_full_pinyins-null"),
        raw!(M(&[("length", I(0)), ("logs", L(&[])), ("ret", I(0))])),
        Some(&["length", "logs", "ret"]),
    );
}

// c-string-input/pinyin-parse_more_full_pinyins-valid: MATCH.
#[test]
fn c_string_input_pinyin_parse_more_full_pinyins_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_parse_more_full_pinyins_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-parse_more_full_pinyins-valid"),
        raw!(M(&[("length", I(3)), ("logs", L(&[])), ("ret", I(3))])),
        None,
    );
}

// c-string-input/pinyin-remember-invalid: MATCH.
#[test]
fn c_string_input_pinyin_remember_invalid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_remember_invalid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-remember-invalid"),
        raw!(M(&[
            ("found", B(false)),
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[]))
        ])),
        None,
    );
}

// c-string-input/pinyin-remember-null: MATCH.
#[test]
fn c_string_input_pinyin_remember_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_remember_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-remember-null"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// c-string-input/pinyin-remember-surrogate: MATCH.
#[test]
fn c_string_input_pinyin_remember_surrogate() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_remember_surrogate",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-remember-surrogate"),
        raw!(M(&[
            ("found", B(true)),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16777217), I(117440513)]))
        ])),
        None,
    );
}

// c-string-input/pinyin-remember-valid: MATCH.
#[test]
fn c_string_input_pinyin_remember_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_remember_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-remember-valid"),
        raw!(M(&[
            ("found", B(true)),
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16777217), I(117440513)]))
        ])),
        None,
    );
}

// c-string-input/pinyin-segment-state-null: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn c_string_input_pinyin_segment_state_null() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_segment_state_null",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-segment-state-null"),
        raw!(M(&[
            ("count", I(0)),
            ("logs", L(&[L(&[S("GLib"), I(8)])])),
            ("ret", B(true))
        ])),
        Some(&["count", "logs", "ret"]),
    );
}

// c-string-input/pinyin-segment-state-valid: MATCH.
#[test]
fn c_string_input_pinyin_segment_state_valid() {
    run_j(
        "c_string_inputs::c_string_input_pinyin_segment_state_valid",
        Expect::RESULT,
        |k| b::case(k, "c-string-input/pinyin-segment-state-valid"),
        raw!(M(&[("count", I(2)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}
