//! Prediction and sentence prefixes converted from valid, invalid, non-scalar and NULL UTF-8 (#1277). Generated from the frozen pin observations.

use crate::bodies::prefix_conversion as b;
use crate::support::{Expect, run_j};

// prefix-convert/zhuyin-sentence-above-unicode: MATCH.
#[test]
fn prefix_convert_zhuyin_sentence_invalid() {
    run_j(
        "prefix_conversion::prefix_convert_zhuyin_sentence_invalid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/zhuyin-sentence-invalid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e4b899")]))
        ])),
        None,
    );
}

// prefix-convert/zhuyin-sentence-null: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn prefix_convert_zhuyin_sentence_null() {
    run_j(
        "prefix_conversion::prefix_convert_zhuyin_sentence_null",
        Expect::stderr(b""),
        |k| b::case(k, "prefix-convert/zhuyin-sentence-null"),
        raw!(M(&[
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e4b899")]))
        ])),
        Some(&["ret", "seeded", "sentence"]),
    );
}

// prefix-convert/zhuyin-sentence-six-byte: MATCH.
#[test]
fn prefix_convert_zhuyin_sentence_six_byte() {
    run_j(
        "prefix_conversion::prefix_convert_zhuyin_sentence_six_byte",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/zhuyin-sentence-six-byte"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e6888c")]))
        ])),
        None,
    );
}

// prefix-convert/zhuyin-sentence-surrogate: MATCH.
#[test]
fn prefix_convert_zhuyin_sentence_valid() {
    run_j(
        "prefix_conversion::prefix_convert_zhuyin_sentence_valid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/zhuyin-sentence-valid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e6888c")]))
        ])),
        None,
    );
}
