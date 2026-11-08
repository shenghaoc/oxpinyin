//! The user bigram under mask_out, export and prediction: malformed planted grams, colliding hash keys, a lazily created user bigram and predicted-bigram rows (#525, #677, #1114, #1231). Generated from the frozen pin observations.

use crate::bodies::bigram_mask_prediction as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-zhuyin-mask-out-non-token-bigram-key: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_mask_out_non_token_bigram_key() {
    run_aborting(
        "bigram_mask_prediction::abort_zhuyin_mask_out_non_token_bigram_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-mask-out-non-token-bigram-key");
        },
    );
}

// abort-zhuyin-mask-out-residual-bigram-gram: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_mask_out_residual_bigram_gram() {
    run_aborting(
        "bigram_mask_prediction::abort_zhuyin_mask_out_residual_bigram_gram",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-mask-out-residual-bigram-gram");
        },
    );
}

// abort-zhuyin-mask-out-short-bigram-value: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_mask_out_short_bigram_value() {
    run_aborting(
        "bigram_mask_prediction::abort_zhuyin_mask_out_short_bigram_value",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-mask-out-short-bigram-value");
        },
    );
}

// import-after-partial-bigram-mask-zhuyin: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn import_after_partial_bigram_mask_zhuyin() {
    run_j(
        "bigram_mask_prediction::import_after_partial_bigram_mask_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "import-after-partial-bigram-mask-zhuyin"),
        raw!(M(&[
            ("first", B(true)),
            ("mask", B(false)),
            ("second", B(true))
        ])),
        Some(&["first", "mask", "second"]),
    );
}

// issue1114-collision-load-save-zhuyin: MATCH.
#[test]
fn issue1114_collision_load_save_zhuyin() {
    #[allow(unused_imports)]
    use crate::support::Raw::*;
    static PROFILE: crate::support::Raw =
        include!("fixtures/bigram_mask_prediction/profile-d2d8369f1034a5c9.rs");
    run_j(
        "bigram_mask_prediction::issue1114_collision_load_save_zhuyin",
        Expect::RESULT,
        |k| b::case_with_profile(k, "issue1114-collision-load-save-zhuyin", &PROFILE),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_mask_prediction/a4ebf892de124c15.rs"
        )),
        None,
    );
}

// issue677-mask-save-sequence-zhuyin: MATCH.
#[test]
fn issue677_mask_save_sequence_zhuyin() {
    run_j(
        "bigram_mask_prediction::issue677_mask_save_sequence_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "issue677-mask-save-sequence-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/bigram_mask_prediction/a5c1cd3c222e6c7d.rs"
        )),
        None,
    );
}

// lazy-user-bigram-zhuyin: MATCH.
#[test]
fn lazy_user_bigram_zhuyin() {
    #[allow(unused_imports)]
    use crate::support::Raw::*;
    static PROFILE: crate::support::Raw =
        include!("fixtures/bigram_mask_prediction/profile-5b523c4ab26c3f9e.rs");
    run_j(
        "bigram_mask_prediction::lazy_user_bigram_zhuyin",
        Expect::RESULT,
        |k| b::case_with_profile(k, "lazy-user-bigram-zhuyin", &PROFILE),
        raw!(include!(
            "fixtures/bigram_mask_prediction/51ba6845c4adbad7.rs"
        )),
        None,
    );
}
