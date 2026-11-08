//! C string inputs: malformed, non-scalar and NULL text at each facade entry (#1279). Generated from the frozen pin observations.

use crate::bodies::c_string_inputs as b;
use crate::support::{Expect, run_aborting, run_j};
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
