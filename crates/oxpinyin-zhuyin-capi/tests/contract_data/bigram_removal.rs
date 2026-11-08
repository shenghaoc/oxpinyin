//! Bigram removals subtract from the stored total, persisted across a save and a reopen (#1175, #740). Generated from the frozen pin observations.

use crate::bodies::bigram_removal as b;
use crate::support::{Expect, run_j};

// removal-bigram-independent-total-mask-out-zhuyin: MATCH.
#[test]
fn removal_bigram_independent_total_mask_out_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_independent_total_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-independent-total-mask-out-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/e56279b3c14d4b07.rs"
        )),
        None,
    );
}

// removal-bigram-multiple-zhuyin: MATCH.
#[test]
fn removal_bigram_multiple_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_multiple_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-multiple-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/a1661ce3eeae7ab1.rs"
        )),
        None,
    );
}

// removal-bigram-ordinary-mask-out-zhuyin: MATCH.
#[test]
fn removal_bigram_ordinary_mask_out_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_ordinary_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-ordinary-mask-out-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/806bb0c52b8afd1e.rs"
        )),
        None,
    );
}

// removal-bigram-remove-zero-mask-out-zhuyin: MATCH.
#[test]
fn removal_bigram_remove_zero_mask_out_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_remove_zero_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-remove-zero-mask-out-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/b6ff6416db942bc0.rs"
        )),
        None,
    );
}

// removal-bigram-reported-mask-out-zhuyin: MATCH.
#[test]
fn removal_bigram_reported_mask_out_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_reported_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-reported-mask-out-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/c1475af68fd01dd1.rs"
        )),
        None,
    );
}

// removal-bigram-underflow-mask-out-zhuyin: MATCH.
#[test]
fn removal_bigram_underflow_mask_out_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_underflow_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-underflow-mask-out-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/8e91f433774c2a36.rs"
        )),
        None,
    );
}

// removal-bigram-wrap-after-mask-out-zhuyin: MATCH.
#[test]
fn removal_bigram_wrap_after_mask_out_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_wrap_after_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-wrap-after-mask-out-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/e74dfb15969a7cb2.rs"
        )),
        None,
    );
}

// removal-bigram-zero-survivors-mask-out-zhuyin: MATCH.
#[test]
fn removal_bigram_zero_survivors_mask_out_zhuyin() {
    run_j(
        "bigram_removal::removal_bigram_zero_survivors_mask_out_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "removal-bigram-zero-survivors-mask-out-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_removal/d657cbe666b80166.rs"
        )),
        None,
    );
}
