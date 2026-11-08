//! The zhuyin twins of the out-parameter and library-unload contracts (#542). Generated from the frozen pin observations.

use crate::bodies::zhuyin_facade_twins as b;
use crate::support::{Expect, run_j};

// user-library-token-unload-zhuyin: MATCH.
#[test]
fn user_library_token_unload_zhuyin() {
    run_j(
        "zhuyin_facade_twins::user_library_token_unload_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-library-token-unload-zhuyin"),
        raw!(include!("fixtures/zhuyin_facade_twins/38ae298a1f47a11e.rs")),
        None,
    );
}

// zhuyin-alloc-instance-after-fini: MATCH.
#[test]
fn zhuyin_alloc_instance_after_fini() {
    run_j(
        "zhuyin_facade_twins::zhuyin_alloc_instance_after_fini",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-alloc-instance-after-fini"),
        raw!(M(&[("live instance", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// zhuyin-character-offset-out: MATCH.
#[test]
fn zhuyin_character_offset_out() {
    run_j(
        "zhuyin_facade_twins::zhuyin_character_offset_out",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-character-offset-out"),
        raw!(include!("fixtures/zhuyin_facade_twins/acfa181e0938af54.rs")),
        None,
    );
}

// zhuyin-key-rest-length: MATCH.
#[test]
fn zhuyin_key_rest_length() {
    run_j(
        "zhuyin_facade_twins::zhuyin_key_rest_length",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-key-rest-length"),
        raw!(include!("fixtures/zhuyin_facade_twins/3c1ec0b10d0479fb.rs")),
        None,
    );
}

// zhuyin-nth-pronunciation-range: MATCH.
#[test]
fn zhuyin_nth_pronunciation_range() {
    run_j(
        "zhuyin_facade_twins::zhuyin_nth_pronunciation_range",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-nth-pronunciation-range"),
        raw!(include!("fixtures/zhuyin_facade_twins/c2c5b6de048f5508.rs")),
        None,
    );
}

// zhuyin-sentence-before-guess: MATCH.
#[test]
fn zhuyin_sentence_before_guess() {
    run_j(
        "zhuyin_facade_twins::zhuyin_sentence_before_guess",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-sentence-before-guess"),
        raw!(include!("fixtures/zhuyin_facade_twins/b31876c65592c22f.rs")),
        None,
    );
}

// zhuyin-static-key-slots: MATCH.
#[test]
fn zhuyin_static_key_slots() {
    run_j(
        "zhuyin_facade_twins::zhuyin_static_key_slots",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-static-key-slots"),
        raw!(include!("fixtures/zhuyin_facade_twins/bbcd530db1a373ee.rs")),
        None,
    );
}

// zhuyin-token-get-phrase-out: MATCH.
#[test]
fn zhuyin_token_get_phrase_out() {
    run_j(
        "zhuyin_facade_twins::zhuyin_token_get_phrase_out",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-token-get-phrase-out"),
        raw!(include!("fixtures/zhuyin_facade_twins/3550b8e7a1d705d8.rs")),
        None,
    );
}

// zhuyin-unload-phrase-library-effect: MATCH.
#[test]
fn zhuyin_unload_phrase_library_effect() {
    run_j(
        "zhuyin_facade_twins::zhuyin_unload_phrase_library_effect",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-unload-phrase-library-effect"),
        raw!(include!("fixtures/zhuyin_facade_twins/657a51689d3ca3e2.rs")),
        None,
    );
}

// zhuyin-unload-phrase-library-repeat: MATCH.
#[test]
fn zhuyin_unload_phrase_library_repeat() {
    run_j(
        "zhuyin_facade_twins::zhuyin_unload_phrase_library_repeat",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-unload-phrase-library-repeat"),
        raw!(include!("fixtures/zhuyin_facade_twins/b73529d502d9c02e.rs")),
        None,
    );
}
