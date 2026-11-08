//! A system phrase holding a raw word no import writes (a surrogate, a word above Unicode, NUL), shown losslessly through the export, the sentence, the candidates and a chosen, trained sentence (#1175, #894). Generated from the frozen pin observations.

use crate::bodies::system_phrase_text as b;
use crate::support::{Expect, run_j};

// phrase-text/pinyin-above-unicode-candidate: MATCH.
#[test]
fn phrase_text_pinyin_above_unicode_candidate() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_above_unicode_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-above-unicode-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2f4908080e4b999")])]))
        ])),
        None,
    );
}

// phrase-text/pinyin-above-unicode-choose: MATCH.
#[test]
fn phrase_text_pinyin_above_unicode_choose() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_above_unicode_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-above-unicode-choose"),
        raw!(include!("fixtures/system_phrase_text/f0e4b4119edc1eca.rs")),
        None,
    );
}

// phrase-text/pinyin-above-unicode-export: MATCH.
#[test]
fn phrase_text_pinyin_above_unicode_export() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_above_unicode_export",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-above-unicode-export"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "rows",
                L(&[
                    L(&[
                        B(true),
                        S("e794b2f4908080e4b999"),
                        S("6461312764613127646131"),
                        I(10)
                    ]),
                    L(&[B(true), S("e4b899"), S("626131"), I(10)])
                ])
            )
        ])),
        None,
    );
}

// phrase-text/pinyin-above-unicode-sentence: MATCH.
#[test]
fn phrase_text_pinyin_above_unicode_sentence() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_above_unicode_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-above-unicode-sentence"),
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

// phrase-text/pinyin-nul-candidate: MATCH.
#[test]
fn phrase_text_pinyin_nul_candidate() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_nul_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-nul-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2")])]))
        ])),
        None,
    );
}

// phrase-text/pinyin-nul-choose: MATCH.
#[test]
fn phrase_text_pinyin_nul_choose() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_nul_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-nul-choose"),
        raw!(include!("fixtures/system_phrase_text/bf194ef6b66e2db2.rs")),
        None,
    );
}

// phrase-text/pinyin-nul-export: MATCH.
#[test]
fn phrase_text_pinyin_nul_export() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_nul_export",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-nul-export"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "rows",
                L(&[
                    L(&[B(true), S("e794b2"), S("6461312764613127646131"), I(10)]),
                    L(&[B(true), S("e4b899"), S("626131"), I(10)])
                ])
            )
        ])),
        None,
    );
}

// phrase-text/pinyin-nul-sentence: MATCH.
#[test]
fn phrase_text_pinyin_nul_sentence() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_nul_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-nul-sentence"),
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

// phrase-text/pinyin-surrogate-candidate: MATCH.
#[test]
fn phrase_text_pinyin_surrogate_candidate() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_surrogate_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-surrogate-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2eda080e4b999")])]))
        ])),
        None,
    );
}

// phrase-text/pinyin-surrogate-choose: MATCH.
#[test]
fn phrase_text_pinyin_surrogate_choose() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_surrogate_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-surrogate-choose"),
        raw!(include!("fixtures/system_phrase_text/102994d243e670d5.rs")),
        None,
    );
}

// phrase-text/pinyin-surrogate-export: MATCH.
#[test]
fn phrase_text_pinyin_surrogate_export() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_surrogate_export",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-surrogate-export"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "rows",
                L(&[
                    L(&[
                        B(true),
                        S("e794b2eda080e4b999"),
                        S("6461312764613127646131"),
                        I(10)
                    ]),
                    L(&[B(true), S("e4b899"), S("626131"), I(10)])
                ])
            )
        ])),
        None,
    );
}

// phrase-text/pinyin-surrogate-sentence: MATCH.
#[test]
fn phrase_text_pinyin_surrogate_sentence() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_surrogate_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-surrogate-sentence"),
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

// phrase-text/pinyin-valid-candidate: MATCH.
#[test]
fn phrase_text_pinyin_valid_candidate() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_valid_candidate",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-valid-candidate"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(9)),
            ("ret", B(true)),
            ("rows", L(&[L(&[I(2), S("e794b2e4b881e4b999")])]))
        ])),
        None,
    );
}

// phrase-text/pinyin-valid-choose: MATCH.
#[test]
fn phrase_text_pinyin_valid_choose() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_valid_choose",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-valid-choose"),
        raw!(include!("fixtures/system_phrase_text/7a2fd5a3b9bcea7e.rs")),
        None,
    );
}

// phrase-text/pinyin-valid-export: MATCH.
#[test]
fn phrase_text_pinyin_valid_export() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_valid_export",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-valid-export"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "rows",
                L(&[
                    L(&[
                        B(true),
                        S("e794b2e4b881e4b999"),
                        S("6461312764613127646131"),
                        I(10)
                    ]),
                    L(&[B(true), S("e4b899"), S("626131"), I(10)])
                ])
            )
        ])),
        None,
    );
}

// phrase-text/pinyin-valid-sentence: MATCH.
#[test]
fn phrase_text_pinyin_valid_sentence() {
    run_j(
        "system_phrase_text::phrase_text_pinyin_valid_sentence",
        Expect::RESULT,
        |k| b::case(k, "phrase-text/pinyin-valid-sentence"),
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
