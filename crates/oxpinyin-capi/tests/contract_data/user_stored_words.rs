//! User-library items whose stored words are replaced with a raw word no import writes, loaded, shown, saved and reloaded losslessly (#1175, #1198). Generated from the frozen pin observations.

use crate::bodies::user_stored_words as b;
use crate::support::{Expect, run_j};

// user-stored-words/pinyin-1-above-unicode: MATCH.
#[test]
fn user_stored_words_pinyin_1_above_unicode() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_1_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-1-above-unicode"),
        raw!(include!("fixtures/user_stored_words/0b584a2caaf9229c.rs")),
        None,
    );
}

// user-stored-words/pinyin-1-nul: MATCH.
#[test]
fn user_stored_words_pinyin_1_nul() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_1_nul",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-1-nul"),
        raw!(include!("fixtures/user_stored_words/546b09d8cca62d7f.rs")),
        None,
    );
}

// user-stored-words/pinyin-1-surrogate: MATCH.
#[test]
fn user_stored_words_pinyin_1_surrogate() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_1_surrogate",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-1-surrogate"),
        raw!(include!("fixtures/user_stored_words/7aeb97f5597bad5b.rs")),
        None,
    );
}

// user-stored-words/pinyin-1-valid: MATCH.
#[test]
fn user_stored_words_pinyin_1_valid() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_1_valid",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-1-valid"),
        raw!(include!("fixtures/user_stored_words/81af0074e3efd5bd.rs")),
        None,
    );
}

// user-stored-words/pinyin-5-above-unicode: MATCH.
#[test]
fn user_stored_words_pinyin_7_above_unicode() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_7_above_unicode",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-7-above-unicode"),
        raw!(include!("fixtures/user_stored_words/34b53462c181fcaf.rs")),
        None,
    );
}

// user-stored-words/pinyin-7-nul: MATCH.
#[test]
fn user_stored_words_pinyin_7_nul() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_7_nul",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-7-nul"),
        raw!(include!("fixtures/user_stored_words/40fe85b9cdb57fd2.rs")),
        None,
    );
}

// user-stored-words/pinyin-7-surrogate: MATCH.
#[test]
fn user_stored_words_pinyin_7_surrogate() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_7_surrogate",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-7-surrogate"),
        raw!(include!("fixtures/user_stored_words/7e30b28341fdd7eb.rs")),
        None,
    );
}

// user-stored-words/pinyin-7-valid: MATCH.
#[test]
fn user_stored_words_pinyin_7_valid() {
    run_j(
        "user_stored_words::user_stored_words_pinyin_7_valid",
        Expect::RESULT,
        |k| b::case(k, "user-stored-words/pinyin-7-valid"),
        raw!(include!("fixtures/user_stored_words/209c9de5355c7038.rs")),
        None,
    );
}
