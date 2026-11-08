//! A system phrase holding a raw word no import writes (a surrogate, a word above Unicode, NUL), shown losslessly through the export, the sentence, the candidates and a chosen, trained sentence (#1175, #894). Generated from the frozen pin observations.

use crate::bodies::system_phrase_text as b;
use crate::support::{Expect, run_j};

// phrase-text/zhuyin-above-unicode-candidate: MATCH.
#[test]
fn phrase_text_zhuyin_above_unicode_candidate() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_above_unicode_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-above-unicode-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2f4908080e4b999")])]))
        ])),
        None,
    );
}

// phrase-text/zhuyin-above-unicode-choose: MATCH.
#[test]
fn phrase_text_zhuyin_above_unicode_choose() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_above_unicode_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-above-unicode-choose"),
        raw!(include!("fixtures/system_phrase_text/c7eeb60fc5e152ac.rs")),
        None,
    );
}

// phrase-text/zhuyin-above-unicode-export: MATCH.
#[test]
fn phrase_text_zhuyin_above_unicode_sentence() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_above_unicode_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-above-unicode-sentence"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(12)),
            ("ret", B(true)),
            ("text", S("e794b2f4908080e4b999e4b899"))
        ])),
        None,
    );
}

// phrase-text/zhuyin-nul-candidate: MATCH.
#[test]
fn phrase_text_zhuyin_nul_candidate() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_nul_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-nul-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2")])]))
        ])),
        None,
    );
}

// phrase-text/zhuyin-nul-choose: MATCH.
#[test]
fn phrase_text_zhuyin_nul_choose() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_nul_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-nul-choose"),
        raw!(include!("fixtures/system_phrase_text/77400ef636e3cc6b.rs")),
        None,
    );
}

// phrase-text/zhuyin-nul-export: MATCH.
#[test]
fn phrase_text_zhuyin_nul_sentence() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_nul_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-nul-sentence"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(12)),
            ("ret", B(true)),
            ("text", S("e794b2e4b899"))
        ])),
        None,
    );
}

// phrase-text/zhuyin-surrogate-candidate: MATCH.
#[test]
fn phrase_text_zhuyin_surrogate_candidate() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_surrogate_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-surrogate-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2eda080e4b999")])]))
        ])),
        None,
    );
}

// phrase-text/zhuyin-surrogate-choose: MATCH.
#[test]
fn phrase_text_zhuyin_surrogate_choose() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_surrogate_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-surrogate-choose"),
        raw!(include!("fixtures/system_phrase_text/d76b9887bdf9788b.rs")),
        None,
    );
}

// phrase-text/zhuyin-surrogate-export: MATCH.
#[test]
fn phrase_text_zhuyin_surrogate_sentence() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_surrogate_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-surrogate-sentence"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(12)),
            ("ret", B(true)),
            ("text", S("e794b2eda080e4b999e4b899"))
        ])),
        None,
    );
}

// phrase-text/zhuyin-valid-candidate: MATCH.
#[test]
fn phrase_text_zhuyin_valid_candidate() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_valid_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-valid-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2e4b881e4b999")])]))
        ])),
        None,
    );
}

// phrase-text/zhuyin-valid-choose: MATCH.
#[test]
fn phrase_text_zhuyin_valid_choose() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_valid_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-valid-choose"),
        raw!(include!("fixtures/system_phrase_text/98392d35bc068afa.rs")),
        None,
    );
}

// phrase-text/zhuyin-valid-export: MATCH.
#[test]
fn phrase_text_zhuyin_valid_export() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_valid_export",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-valid-export"),
        raw!(M(&[
            (
                "exports",
                M(&[
                    ("begin_get_phrases", B(false)),
                    ("end_get_phrases", B(false)),
                    ("iterator_get_next_phrase", B(false)),
                    ("iterator_has_next_phrase", B(false))
                ])
            ),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// phrase-text/zhuyin-valid-sentence: MATCH.
#[test]
fn phrase_text_zhuyin_valid_sentence() {
    run_j(
        "system_phrase_text::phrase_text_zhuyin_valid_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/zhuyin-valid-sentence"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(12)),
            ("ret", B(true)),
            ("text", S("e794b2e4b881e4b999e4b899"))
        ])),
        None,
    );
}
