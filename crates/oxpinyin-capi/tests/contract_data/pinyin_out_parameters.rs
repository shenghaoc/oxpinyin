//! Pinyin return values and out-parameters the pin leaves alone or writes (#542). Generated from the frozen pin observations.

use crate::bodies::pinyin_out_parameters as b;
use crate::support::{Expect, run_j};

// character-offset-out: MATCH.
#[test]
fn character_offset_out() {
    run_j(
        "pinyin_out_parameters::character_offset_out",
        Expect::RESULT,
        |k| b::case(k, "character-offset-out"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/99ef344f1162753b.rs"
        )),
        None,
    );
}

// empty-parse-sentence: MATCH.
#[test]
fn empty_parse_sentence() {
    run_j(
        "pinyin_out_parameters::empty_parse_sentence",
        Expect::RESULT,
        |k| b::case(k, "empty-parse-sentence"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/0659154fcc1510be.rs"
        )),
        None,
    );
}

// guess-candidates-keyless: MATCH.
#[test]
fn guess_candidates_keyless() {
    run_j(
        "pinyin_out_parameters::guess_candidates_keyless",
        Expect::RESULT,
        |k| b::case(k, "guess-candidates-keyless"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/7fee3378a6ebb92b.rs"
        )),
        None,
    );
}

// guess-sentence-keyless: MATCH.
#[test]
fn guess_sentence_keyless() {
    run_j(
        "pinyin_out_parameters::guess_sentence_keyless",
        Expect::RESULT,
        |k| b::case(k, "guess-sentence-keyless"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/0d7b6624ccea3fe6.rs"
        )),
        None,
    );
}

// key-rest-length: MATCH.
#[test]
fn key_rest_length() {
    run_j(
        "pinyin_out_parameters::key_rest_length",
        Expect::RESULT,
        |k| b::case(k, "key-rest-length"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/3c1ec0b10d0479fb.rs"
        )),
        None,
    );
}

// nth-pronunciation-range: MATCH.
#[test]
fn nth_pronunciation_range() {
    run_j(
        "pinyin_out_parameters::nth_pronunciation_range",
        Expect::RESULT,
        |k| b::case(k, "nth-pronunciation-range"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/c2c5b6de048f5508.rs"
        )),
        None,
    );
}

// sentence-before-guess: MATCH.
#[test]
fn sentence_before_guess() {
    run_j(
        "pinyin_out_parameters::sentence_before_guess",
        Expect::RESULT,
        |k| b::case(k, "sentence-before-guess"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/475d2706b895391f.rs"
        )),
        None,
    );
}

// sentence-before-guess-schemes: MATCH.
#[test]
fn sentence_before_guess_schemes() {
    run_j(
        "pinyin_out_parameters::sentence_before_guess_schemes",
        Expect::RESULT,
        |k| b::case(k, "sentence-before-guess-schemes"),
        raw!(M(&[
            ("chewing", L(&[B(false), S("untouched")])),
            ("double", L(&[B(false), S("untouched")])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// sentence-with-prefix-bytes: MATCH.
#[test]
fn sentence_with_prefix_bytes() {
    run_j(
        "pinyin_out_parameters::sentence_with_prefix_bytes",
        Expect::RESULT,
        |k| b::case(k, "sentence-with-prefix-bytes"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/70985a035133a232.rs"
        )),
        None,
    );
}

// static-key-slots: MATCH.
#[test]
fn static_key_slots() {
    run_j(
        "pinyin_out_parameters::static_key_slots",
        Expect::RESULT,
        |k| b::case(k, "static-key-slots"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/a9593d0aeae3008c.rs"
        )),
        None,
    );
}

// token-get-phrase-out: MATCH.
#[test]
fn token_get_phrase_out() {
    run_j(
        "pinyin_out_parameters::token_get_phrase_out",
        Expect::RESULT,
        |k| b::case(k, "token-get-phrase-out"),
        raw!(include!(
            "fixtures/pinyin_out_parameters/3550b8e7a1d705d8.rs"
        )),
        None,
    );
}

// unload-phrase-library-repeat: MATCH.
#[test]
fn unload_phrase_library_repeat() {
    run_j(
        "pinyin_out_parameters::unload_phrase_library_repeat",
        Expect::RESULT,
        |k| b::case(k, "unload-phrase-library-repeat"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "unload",
                L(&[
                    B(true),
                    B(true),
                    B(true),
                    B(false),
                    B(false),
                    B(false),
                    B(true)
                ])
            )
        ])),
        None,
    );
}
