//! User-library items whose stored words are replaced with a raw word no import writes, loaded, shown, saved and reloaded losslessly (#1175, #1198). Generated from the frozen pin observations.

use crate::bodies::user_stored_words as b;
use crate::support::{Expect, run_j};

// user-stored-words/zhuyin-1-above-unicode: MATCH.
#[test]
fn user_stored_words_zhuyin_1_above_unicode() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_1_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-1-above-unicode"),
        raw!(include!("fixtures/user_stored_words/ddd49bd78d465ce4.rs")),
        None,
    );
}

// user-stored-words/zhuyin-1-nul: MATCH.
#[test]
fn user_stored_words_zhuyin_1_nul() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_1_nul",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-1-nul"),
        raw!(include!("fixtures/user_stored_words/55ac25577e83f551.rs")),
        None,
    );
}

// user-stored-words/zhuyin-1-surrogate: MATCH.
#[test]
fn user_stored_words_zhuyin_1_surrogate() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_1_surrogate",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-1-surrogate"),
        raw!(include!("fixtures/user_stored_words/8c03f1b67641f618.rs")),
        None,
    );
}

// user-stored-words/zhuyin-1-valid: MATCH.
#[test]
fn user_stored_words_zhuyin_1_valid() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_1_valid",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-1-valid"),
        raw!(include!("fixtures/user_stored_words/f45c5edfb261a60e.rs")),
        None,
    );
}

// user-stored-words/zhuyin-5-above-unicode: MATCH.
#[test]
fn user_stored_words_zhuyin_7_above_unicode() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_7_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-7-above-unicode"),
        raw!(include!("fixtures/user_stored_words/e0e69334f6612c27.rs")),
        None,
    );
}

// user-stored-words/zhuyin-7-nul: MATCH.
#[test]
fn user_stored_words_zhuyin_7_nul() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_7_nul",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-7-nul"),
        raw!(include!("fixtures/user_stored_words/6be5726620046481.rs")),
        None,
    );
}

// user-stored-words/zhuyin-7-surrogate: MATCH.
#[test]
fn user_stored_words_zhuyin_7_surrogate() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_7_surrogate",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-7-surrogate"),
        raw!(include!("fixtures/user_stored_words/4a00b43d2604bcae.rs")),
        None,
    );
}

// user-stored-words/zhuyin-7-valid: MATCH.
#[test]
fn user_stored_words_zhuyin_7_valid() {
    run_j(
        "user_stored_words::user_stored_words_zhuyin_7_valid",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/zhuyin-7-valid"),
        raw!(include!("fixtures/user_stored_words/ea4572da7dc3ae35.rs")),
        None,
    );
}
