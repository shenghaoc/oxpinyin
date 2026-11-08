//! Token unigrams and training: stored unigrams of system and user tokens, zhuyin-only readings, the library total under repeated training and predicted-row acceptance (#540, #576, #808, #809, #1418). Generated from the frozen pin observations.

use crate::bodies::token_unigram_training as b;
use crate::support::{Expect, run_j};

// zhuyin-system-zhuyin-only-readings: MATCH.
#[test]
fn zhuyin_system_zhuyin_only_readings() {
    run_j(
        "token_unigram_training::zhuyin_system_zhuyin_only_readings",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-system-zhuyin-only-readings"),
        raw!(include!(
            "fixtures/token_unigram_training/495a94a5731b9c2a.rs"
        )),
        None,
    );
}

// zhuyin-token-unigram-system: MATCH.
#[test]
fn zhuyin_token_unigram_system() {
    run_j(
        "token_unigram_training::zhuyin_token_unigram_system",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-token-unigram-system"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/token_unigram_training/1b47f40ec2a18769.rs"
        )),
        None,
    );
}

// zhuyin-token-unigram-user-libraries: MATCH.
#[test]
fn zhuyin_token_unigram_user_libraries() {
    run_j(
        "token_unigram_training::zhuyin_token_unigram_user_libraries",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-token-unigram-user-libraries"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/token_unigram_training/7405429b7ed8fdc3.rs"
        )),
        None,
    );
}

// zhuyin-user-zhuyin-only-readings: MATCH.
#[test]
fn zhuyin_user_zhuyin_only_readings() {
    run_j(
        "token_unigram_training::zhuyin_user_zhuyin_only_readings",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-user-zhuyin-only-readings"),
        raw!(include!(
            "fixtures/token_unigram_training/c1563ef2e2f3e0a9.rs"
        )),
        None,
    );
}
