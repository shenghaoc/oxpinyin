//! User readings whose stored key is not a typed-input spelling, and crafted user.bin key words (#1418). Generated from the frozen pin observations.

use crate::bodies::user_key_spellings as b;
use crate::support::{Expect, run_j};

// user-key-spelling/pinyin-egn: MATCH.
#[test]
fn user_key_spelling_pinyin_egn() {
    run_j(
        "user_key_spellings::user_key_spelling_pinyin_egn",
        Expect::RESULT,
        |k| b::case(k, "user-key-spelling/pinyin-egn"),
        raw!(include!("fixtures/user_key_spellings/aa9567250ece9e60.rs")),
        None,
    );
}

// user-key-spelling/pinyin-emg: MATCH.
#[test]
fn user_key_spelling_pinyin_nuen() {
    run_j(
        "user_key_spellings::user_key_spelling_pinyin_nuen",
        Expect::RESULT,
        |k| b::case(k, "user-key-spelling/pinyin-nuen"),
        raw!(include!("fixtures/user_key_spellings/18c5709045397129.rs")),
        None,
    );
}

// user-stored-key/pinyin-0x8000: MATCH.
#[test]
fn user_stored_key_pinyin_0x8000() {
    run_j(
        "user_key_spellings::user_stored_key_pinyin_0x8000",
        Expect::RESULT,
        |k| b::case(k, "user-stored-key/pinyin-0x8000"),
        raw!(M(&[
            ("imported", L(&[B(true), I(1), S("2b30")])),
            ("logs", L(&[])),
            ("phrase", L(&[B(true), I(1), S("e4bda0")])),
            ("reopened", L(&[B(true), I(1), S("0080")])),
            ("stored", S("0080"))
        ])),
        None,
    );
}

// user-stored-key/pinyin-no-syllable: MATCH.
#[test]
fn user_stored_key_pinyin_no_syllable() {
    run_j(
        "user_key_spellings::user_stored_key_pinyin_no_syllable",
        Expect::RESULT,
        |k| b::case(k, "user-stored-key/pinyin-no-syllable"),
        raw!(M(&[
            ("imported", L(&[B(true), I(1), S("2b30")])),
            ("logs", L(&[])),
            ("phrase", L(&[B(true), I(1), S("e4bda0")])),
            ("reopened", L(&[B(true), I(1), S("1f30")])),
            ("stored", S("1f30"))
        ])),
        None,
    );
}

// user-stored-key/pinyin-tone6: MATCH.
#[test]
fn user_stored_key_pinyin_tone6() {
    run_j(
        "user_key_spellings::user_stored_key_pinyin_tone6",
        Expect::RESULT,
        |k| b::case(k, "user-stored-key/pinyin-tone6"),
        raw!(M(&[
            ("imported", L(&[B(true), I(1), S("2b30")])),
            ("logs", L(&[])),
            ("phrase", L(&[B(true), I(1), S("e4bda0")])),
            ("reopened", L(&[B(true), I(1), S("2b60")])),
            ("stored", S("2b60"))
        ])),
        None,
    );
}
