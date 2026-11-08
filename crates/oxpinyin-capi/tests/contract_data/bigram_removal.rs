//! Bigram removals subtract from the stored total, persisted across a save and a reopen (#1175, #740). Generated from the frozen pin observations.

use crate::bodies::bigram_removal as b;
use crate::support::{Expect, run_j};

// removal-bigram-independent-total-mask-out-pinyin: MATCH.
#[test]
fn removal_bigram_independent_total_mask_out_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_independent_total_mask_out_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-independent-total-mask-out-pinyin"),
        raw!(include!("fixtures/bigram_removal/e56279b3c14d4b07.rs")),
        None,
    );
}

// removal-bigram-independent-total-remove-candidate-pinyin: MATCH.
#[test]
fn removal_bigram_independent_total_remove_candidate_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_independent_total_remove_candidate_pinyin",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "removal-bigram-independent-total-remove-candidate-pinyin",
            )
        },
        raw!(include!("fixtures/bigram_removal/e56279b3c14d4b07.rs")),
        None,
    );
}

// removal-bigram-multiple-pinyin: MATCH.
#[test]
fn removal_bigram_multiple_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_multiple_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-multiple-pinyin"),
        raw!(include!("fixtures/bigram_removal/a1661ce3eeae7ab1.rs")),
        None,
    );
}

// removal-bigram-ordinary-mask-out-pinyin: MATCH.
#[test]
fn removal_bigram_ordinary_mask_out_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_ordinary_mask_out_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-ordinary-mask-out-pinyin"),
        raw!(include!("fixtures/bigram_removal/806bb0c52b8afd1e.rs")),
        None,
    );
}

// removal-bigram-ordinary-remove-candidate-pinyin: MATCH.
#[test]
fn removal_bigram_ordinary_remove_candidate_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_ordinary_remove_candidate_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-ordinary-remove-candidate-pinyin"),
        raw!(include!("fixtures/bigram_removal/806bb0c52b8afd1e.rs")),
        None,
    );
}

// removal-bigram-remove-zero-mask-out-pinyin: MATCH.
#[test]
fn removal_bigram_remove_zero_mask_out_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_remove_zero_mask_out_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-remove-zero-mask-out-pinyin"),
        raw!(include!("fixtures/bigram_removal/b6ff6416db942bc0.rs")),
        None,
    );
}

// removal-bigram-remove-zero-remove-candidate-pinyin: MATCH.
#[test]
fn removal_bigram_remove_zero_remove_candidate_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_remove_zero_remove_candidate_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-remove-zero-remove-candidate-pinyin"),
        raw!(include!("fixtures/bigram_removal/b6ff6416db942bc0.rs")),
        None,
    );
}

// removal-bigram-reported-mask-out-pinyin: MATCH.
#[test]
fn removal_bigram_reported_mask_out_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_reported_mask_out_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-reported-mask-out-pinyin"),
        raw!(include!("fixtures/bigram_removal/c1475af68fd01dd1.rs")),
        None,
    );
}

// removal-bigram-reported-remove-candidate-pinyin: MATCH.
#[test]
fn removal_bigram_reported_remove_candidate_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_reported_remove_candidate_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-reported-remove-candidate-pinyin"),
        raw!(include!("fixtures/bigram_removal/c1475af68fd01dd1.rs")),
        None,
    );
}

// removal-bigram-underflow-mask-out-pinyin: MATCH.
#[test]
fn removal_bigram_underflow_mask_out_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_underflow_mask_out_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-underflow-mask-out-pinyin"),
        raw!(include!("fixtures/bigram_removal/8e91f433774c2a36.rs")),
        None,
    );
}

// removal-bigram-underflow-remove-candidate-pinyin: MATCH.
#[test]
fn removal_bigram_underflow_remove_candidate_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_underflow_remove_candidate_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-underflow-remove-candidate-pinyin"),
        raw!(include!("fixtures/bigram_removal/8e91f433774c2a36.rs")),
        None,
    );
}

// removal-bigram-wrap-after-mask-out-pinyin: MATCH.
#[test]
fn removal_bigram_wrap_after_mask_out_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_wrap_after_mask_out_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-wrap-after-mask-out-pinyin"),
        raw!(include!("fixtures/bigram_removal/e74dfb15969a7cb2.rs")),
        None,
    );
}

// removal-bigram-wrap-after-remove-candidate-pinyin: MATCH.
#[test]
fn removal_bigram_wrap_after_remove_candidate_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_wrap_after_remove_candidate_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-wrap-after-remove-candidate-pinyin"),
        raw!(include!("fixtures/bigram_removal/e74dfb15969a7cb2.rs")),
        None,
    );
}

// removal-bigram-zero-survivors-mask-out-pinyin: MATCH.
#[test]
fn removal_bigram_zero_survivors_mask_out_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_zero_survivors_mask_out_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-zero-survivors-mask-out-pinyin"),
        raw!(include!("fixtures/bigram_removal/d657cbe666b80166.rs")),
        None,
    );
}

// removal-bigram-zero-survivors-remove-candidate-pinyin: MATCH.
#[test]
fn removal_bigram_zero_survivors_remove_candidate_pinyin() {
    run_j(
        "bigram_removal::removal_bigram_zero_survivors_remove_candidate_pinyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-zero-survivors-remove-candidate-pinyin"),
        raw!(include!("fixtures/bigram_removal/d657cbe666b80166.rs")),
        None,
    );
}
