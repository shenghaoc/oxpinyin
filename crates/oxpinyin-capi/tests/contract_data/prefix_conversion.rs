//! Prediction and sentence prefixes converted from valid, invalid, non-scalar and NULL UTF-8 (#1277). Generated from the frozen pin observations.

use crate::bodies::prefix_conversion as b;
use crate::support::{Expect, run_j};

// prefix-convert/pinyin-sentence-above-unicode: MATCH.
#[test]
fn prefix_convert_pinyin_sentence_five_byte() {
    run_j(
        "prefix_conversion::prefix_convert_pinyin_sentence_five_byte",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/pinyin-sentence-five-byte"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e6888c")]))
        ])),
        None,
    );
}

// prefix-convert/pinyin-sentence-invalid: MATCH.
#[test]
fn prefix_convert_pinyin_sentence_invalid() {
    run_j(
        "prefix_conversion::prefix_convert_pinyin_sentence_invalid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/pinyin-sentence-invalid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e4b899")]))
        ])),
        None,
    );
}

// prefix-convert/pinyin-sentence-null: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn prefix_convert_pinyin_sentence_null() {
    run_j(
        "prefix_conversion::prefix_convert_pinyin_sentence_null",
        Expect::stderr(b""),
        |k| b::case(k, "prefix-convert/pinyin-sentence-null"),
        raw!(M(&[
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e4b899")]))
        ])),
        Some(&["ret", "seeded", "sentence"]),
    );
}

// prefix-convert/pinyin-sentence-six-byte: MATCH.
#[test]
fn prefix_convert_pinyin_sentence_valid() {
    run_j(
        "prefix_conversion::prefix_convert_pinyin_sentence_valid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/pinyin-sentence-valid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("seeded", B(true)),
            ("sentence", L(&[B(true), S("e6888c")]))
        ])),
        None,
    );
}

// prefix-convert/predicted-above-unicode: MATCH.
#[test]
fn prefix_convert_predicted_invalid() {
    run_j(
        "prefix_conversion::prefix_convert_predicted_invalid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/predicted-invalid"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false)), ("rows", L(&[]))])),
        None,
    );
}

// prefix-convert/predicted-null: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn prefix_convert_predicted_null() {
    run_j(
        "prefix_conversion::prefix_convert_predicted_null",
        Expect::stderr(b""),
        |k| b::case(k, "prefix-convert/predicted-null"),
        raw!(M(&[("ret", B(false)), ("rows", L(&[]))])),
        Some(&["ret", "rows"]),
    );
}

// prefix-convert/predicted-six-byte: MATCH.
#[test]
fn prefix_convert_predicted_surrogate() {
    run_j(
        "prefix_conversion::prefix_convert_predicted_surrogate",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/predicted-surrogate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(5), S("e4b899")])]))
        ])),
        None,
    );
}

// prefix-convert/predicted-valid: MATCH.
#[test]
fn prefix_convert_predicted_valid() {
    run_j(
        "prefix_conversion::prefix_convert_predicted_valid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/predicted-valid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(5), S("e4b899")])]))
        ])),
        None,
    );
}

// prefix-convert/punctuation-above-unicode: MATCH.
#[test]
fn prefix_convert_punctuation_above_unicode() {
    run_j(
        "prefix_conversion::prefix_convert_punctuation_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/punctuation-above-unicode"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(5), S("e4b899")])]))
        ])),
        None,
    );
}

// prefix-convert/punctuation-five-byte: MATCH.
#[test]
fn prefix_convert_punctuation_invalid() {
    run_j(
        "prefix_conversion::prefix_convert_punctuation_invalid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/punctuation-invalid"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true)), ("rows", L(&[]))])),
        None,
    );
}

// prefix-convert/punctuation-null: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn prefix_convert_punctuation_null() {
    run_j(
        "prefix_conversion::prefix_convert_punctuation_null",
        Expect::stderr(b""),
        |k| b::case(k, "prefix-convert/punctuation-null"),
        raw!(M(&[("ret", B(true)), ("rows", L(&[]))])),
        Some(&["ret", "rows"]),
    );
}

// prefix-convert/punctuation-six-byte: MATCH.
#[test]
fn prefix_convert_punctuation_valid() {
    run_j(
        "prefix_conversion::prefix_convert_punctuation_valid",
        Expect::RESULT,
        |k| b::case(k, "prefix-convert/punctuation-valid"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(5), S("e4b899")])]))
        ])),
        None,
    );
}
