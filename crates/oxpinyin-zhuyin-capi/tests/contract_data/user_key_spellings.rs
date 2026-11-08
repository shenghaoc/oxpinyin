//! User readings whose stored key is not a typed-input spelling, and crafted user.bin key words (#1418). Generated from the frozen pin observations.

use crate::bodies::user_key_spellings as b;
use crate::support::{Expect, run_j};

// user-key-spelling/zhuyin-den: MATCH.
#[test]
fn user_key_spelling_zhuyin_den() {
    run_j(
        "user_key_spellings::user_key_spelling_zhuyin_den",
        Expect::RESULT,
        |k| b::case(k, "user-key-spelling/zhuyin-den"),
        raw!(include!("fixtures/user_key_spellings/f1c912866df585cc.rs")),
        None,
    );
}

// user-key-spelling/zhuyin-den-tone4: MATCH.
#[test]
fn user_key_spelling_zhuyin_den_tone4() {
    run_j(
        "user_key_spellings::user_key_spelling_zhuyin_den_tone4",
        Expect::RESULT,
        |k| b::case(k, "user-key-spelling/zhuyin-den-tone4"),
        raw!(include!("fixtures/user_key_spellings/9989b91625b48e73.rs")),
        None,
    );
}

// user-key-spelling/zhuyin-eng: MATCH.
#[test]
fn user_key_spelling_zhuyin_eng() {
    run_j(
        "user_key_spellings::user_key_spelling_zhuyin_eng",
        Expect::RESULT,
        |k| b::case(k, "user-key-spelling/zhuyin-eng"),
        raw!(include!("fixtures/user_key_spellings/1be7141af8e9f697.rs")),
        None,
    );
}

// user-stored-key/zhuyin-0x8000: MATCH.
#[test]
fn user_stored_key_zhuyin_0x8000() {
    run_j(
        "user_key_spellings::user_stored_key_zhuyin_0x8000",
        Expect::RESULT,
        |k| b::case(k, "user-stored-key/zhuyin-0x8000"),
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

// user-stored-key/zhuyin-no-syllable: MATCH.
#[test]
fn user_stored_key_zhuyin_no_syllable() {
    run_j(
        "user_key_spellings::user_stored_key_zhuyin_no_syllable",
        Expect::RESULT,
        |k| b::case(k, "user-stored-key/zhuyin-no-syllable"),
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

// user-stored-key/zhuyin-tone6: MATCH.
#[test]
fn user_stored_key_zhuyin_tone6() {
    run_j(
        "user_key_spellings::user_stored_key_zhuyin_tone6",
        Expect::RESULT,
        |k| b::case(k, "user-stored-key/zhuyin-tone6"),
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
