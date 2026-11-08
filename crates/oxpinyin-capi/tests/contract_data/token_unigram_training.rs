//! Token unigrams and training: stored unigrams of system and user tokens, zhuyin-only readings, the library total under repeated training and predicted-row acceptance (#540, #576, #808, #809, #1418). Generated from the frozen pin observations.

use crate::bodies::token_unigram_training as b;
use crate::support::{Expect, run_j};

// predicted-prefix-user-phrase-unigram: MATCH.
#[test]
fn predicted_prefix_user_phrase_unigram() {
    run_j(
        "token_unigram_training::predicted_prefix_user_phrase_unigram",
        Expect::RESULT,
        |k| b::case(k, "predicted-prefix-user-phrase-unigram"),
        raw!(include!(
            "fixtures/token_unigram_training/992673e0e306f027.rs"
        )),
        None,
    );
}

// predicted-unigram-overflow: MATCH.
#[test]
fn predicted_unigram_overflow() {
    run_j(
        "token_unigram_training::predicted_unigram_overflow",
        Expect::RESULT,
        |k| b::case(k, "predicted-unigram-overflow"),
        raw!(M(&[
            ("accepted before the first refusal", I(264)),
            ("chosen", L(&[I(2), I(5)])),
            ("found", B(true)),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// system-zhuyin-only-readings: MATCH.
#[test]
fn system_zhuyin_only_readings() {
    run_j(
        "token_unigram_training::system_zhuyin_only_readings",
        Expect::RESULT,
        |k| b::case(k, "system-zhuyin-only-readings"),
        raw!(include!(
            "fixtures/token_unigram_training/8b63ae5c2c0f3fb4.rs"
        )),
        None,
    );
}

// token-unigram-system: MATCH.
#[test]
fn token_unigram_system() {
    run_j(
        "token_unigram_training::token_unigram_system",
        Expect::RESULT,
        |k| b::case(k, "token-unigram-system"),
        raw!(include!(
            "fixtures/token_unigram_training/1b47f40ec2a18769.rs"
        )),
        None,
    );
}

// token-unigram-user-libraries: MATCH.
#[test]
fn token_unigram_user_libraries() {
    run_j(
        "token_unigram_training::token_unigram_user_libraries",
        Expect::RESULT,
        |k| b::case(k, "token-unigram-user-libraries"),
        raw!(include!(
            "fixtures/token_unigram_training/7405429b7ed8fdc3.rs"
        )),
        None,
    );
}

// train-unigram-total: MATCH.
#[test]
fn train_unigram_total() {
    run_j(
        "token_unigram_training::train_unigram_total",
        Expect::RESULT,
        |k| b::case(k, "train-unigram-total"),
        raw!(include!(
            "fixtures/token_unigram_training/936e3852a6b0fd77.rs"
        )),
        None,
    );
}

// user-zhuyin-only-readings: MATCH.
#[test]
fn user_zhuyin_only_readings() {
    run_j(
        "token_unigram_training::user_zhuyin_only_readings",
        Expect::RESULT,
        |k| b::case(k, "user-zhuyin-only-readings"),
        raw!(include!(
            "fixtures/token_unigram_training/371743aa0066977d.rs"
        )),
        None,
    );
}
