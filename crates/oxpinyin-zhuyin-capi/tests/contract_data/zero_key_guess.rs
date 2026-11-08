//! Zero-key separator parses refused by sentence and candidate guesses, fresh and after a guess (#902). Generated from the frozen pin observations.

use crate::bodies::zero_key_guess as b;
use crate::support::{Expect, run_j};

// zero-key-guess/zhuyin/0x18a/'''/guess_sentence: MATCH.
#[test]
fn zero_key_guess_zhuyin_0x18a_apos_apos_apos_guess_sentence() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_0x18a_apos_apos_apos_guess_sentence",
        Expect::RESULT,
        |k| b::case(k, "zero-key-guess/zhuyin/0x18a/'''/guess_sentence"),
        raw!(include!("fixtures/zero_key_guess/d774a96b57e82cf0.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/0x18a/''/guess_sentence: MATCH.
#[test]
fn zero_key_guess_zhuyin_0x18a_apos_apos_guess_sentence() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_0x18a_apos_apos_guess_sentence",
        Expect::RESULT,
        |k| b::case(k, "zero-key-guess/zhuyin/0x18a/''/guess_sentence"),
        raw!(include!("fixtures/zero_key_guess/da8e35701f9b6f9e.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/0x18a/'/guess_candidates_after_cursor: MATCH.
#[test]
fn zero_key_guess_zhuyin_0x18a_apos_guess_candidates_after_cursor() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_0x18a_apos_guess_candidates_after_cursor",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "zero-key-guess/zhuyin/0x18a/'/guess_candidates_after_cursor",
            )
        },
        raw!(include!("fixtures/zero_key_guess/90e1e805489bd526.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/0x18a/'/guess_candidates_before_cursor: MATCH.
#[test]
fn zero_key_guess_zhuyin_0x18a_apos_guess_candidates_before_cursor() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_0x18a_apos_guess_candidates_before_cursor",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "zero-key-guess/zhuyin/0x18a/'/guess_candidates_before_cursor",
            )
        },
        raw!(include!("fixtures/zero_key_guess/90e1e805489bd526.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/0x18a/'/guess_sentence: MATCH.
#[test]
fn zero_key_guess_zhuyin_0x18a_apos_guess_sentence() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_0x18a_apos_guess_sentence",
        Expect::RESULT,
        |k| b::case(k, "zero-key-guess/zhuyin/0x18a/'/guess_sentence"),
        raw!(include!("fixtures/zero_key_guess/90e1e805489bd526.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/0x18a/'/guess_sentence_with_prefix: MATCH.
#[test]
fn zero_key_guess_zhuyin_0x18a_apos_guess_sentence_with_prefix() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_0x18a_apos_guess_sentence_with_prefix",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "zero-key-guess/zhuyin/0x18a/'/guess_sentence_with_prefix",
            )
        },
        raw!(include!("fixtures/zero_key_guess/90e1e805489bd526.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/0x20/'/guess_sentence: MATCH.
#[test]
fn zero_key_guess_zhuyin_0x20_apos_guess_sentence() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_0x20_apos_guess_sentence",
        Expect::RESULT,
        |k| b::case(k, "zero-key-guess/zhuyin/0x20/'/guess_sentence"),
        raw!(include!("fixtures/zero_key_guess/934414412bb924c1.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/default/'''/guess_sentence: MATCH.
#[test]
fn zero_key_guess_zhuyin_default_apos_apos_apos_guess_sentence() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_default_apos_apos_apos_guess_sentence",
        Expect::RESULT,
        |k| b::case(k, "zero-key-guess/zhuyin/default/'''/guess_sentence"),
        raw!(include!("fixtures/zero_key_guess/2713c1d16eaf2371.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/default/''/guess_sentence: MATCH.
#[test]
fn zero_key_guess_zhuyin_default_apos_apos_guess_sentence() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_default_apos_apos_guess_sentence",
        Expect::RESULT,
        |k| b::case(k, "zero-key-guess/zhuyin/default/''/guess_sentence"),
        raw!(include!("fixtures/zero_key_guess/7616661a67db55b3.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/default/'/guess_candidates_after_cursor: MATCH.
#[test]
fn zero_key_guess_zhuyin_default_apos_guess_candidates_after_cursor() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_default_apos_guess_candidates_after_cursor",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "zero-key-guess/zhuyin/default/'/guess_candidates_after_cursor",
            )
        },
        raw!(include!("fixtures/zero_key_guess/934414412bb924c1.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/default/'/guess_candidates_before_cursor: MATCH.
#[test]
fn zero_key_guess_zhuyin_default_apos_guess_candidates_before_cursor() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_default_apos_guess_candidates_before_cursor",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "zero-key-guess/zhuyin/default/'/guess_candidates_before_cursor",
            )
        },
        raw!(include!("fixtures/zero_key_guess/934414412bb924c1.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/default/'/guess_sentence: MATCH.
#[test]
fn zero_key_guess_zhuyin_default_apos_guess_sentence() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_default_apos_guess_sentence",
        Expect::RESULT,
        |k| b::case(k, "zero-key-guess/zhuyin/default/'/guess_sentence"),
        raw!(include!("fixtures/zero_key_guess/934414412bb924c1.rs")),
        None,
    );
}

// zero-key-guess/zhuyin/default/'/guess_sentence_with_prefix: MATCH.
#[test]
fn zero_key_guess_zhuyin_default_apos_guess_sentence_with_prefix() {
    run_j(
        "zero_key_guess::zero_key_guess_zhuyin_default_apos_guess_sentence_with_prefix",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "zero-key-guess/zhuyin/default/'/guess_sentence_with_prefix",
            )
        },
        raw!(include!("fixtures/zero_key_guess/934414412bb924c1.rs")),
        None,
    );
}
