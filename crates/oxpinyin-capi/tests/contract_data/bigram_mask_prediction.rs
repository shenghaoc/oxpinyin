//! The user bigram under mask_out, export and prediction: malformed planted grams, colliding hash keys, a lazily created user bigram and predicted-bigram rows (#525, #677, #1114, #1231). Generated from the frozen pin observations.

use crate::bodies::bigram_mask_prediction as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-choose-candidate-predicted-bigram-row: the pin's abort, reproduced; no frozen reference: the expectation is the pin's assert at pinyin.cpp:2507-2508 (`PREDICTED_BIGRAM_CANDIDATE != candidate->m_candidate_type`), reached by passing the planted kind-4 row to pinyin_choose_candidate, and the init's `open %s failed.` line (pin-abort-assert-line.md: oxpinyin does not write glibc's assertion line).
#[test]
fn abort_choose_candidate_predicted_bigram_row() {
    run_aborting(
        "bigram_mask_prediction::abort_choose_candidate_predicted_bigram_row",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-choose-candidate-predicted-bigram-row");
        },
    );
}

// abort-begin-bigram-phrases-non-token-key: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_begin_bigram_phrases_non_token_key() {
    run_aborting(
        "bigram_mask_prediction::abort_begin_bigram_phrases_non_token_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-begin-bigram-phrases-non-token-key");
        },
    );
}

// abort-bigram-export-short-predecessor: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_bigram_export_short_predecessor() {
    run_aborting(
        "bigram_mask_prediction::abort_bigram_export_short_predecessor",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-bigram-export-short-predecessor");
        },
    );
}

// abort-mask-out-non-token-bigram-key: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_non_token_bigram_key() {
    run_aborting(
        "bigram_mask_prediction::abort_mask_out_non_token_bigram_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-mask-out-non-token-bigram-key");
        },
    );
}

// abort-mask-out-residual-bigram-gram: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_residual_bigram_gram() {
    run_aborting(
        "bigram_mask_prediction::abort_mask_out_residual_bigram_gram",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-mask-out-residual-bigram-gram");
        },
    );
}

// abort-mask-out-short-bigram-value: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_mask_out_short_bigram_value() {
    run_aborting(
        "bigram_mask_prediction::abort_mask_out_short_bigram_value",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-mask-out-short-bigram-value");
        },
    );
}

// import-after-partial-bigram-mask-pinyin: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn import_after_partial_bigram_mask_pinyin() {
    run_j(
        "bigram_mask_prediction::import_after_partial_bigram_mask_pinyin",
        Expect::RESULT,
        |k| b::case(k, "import-after-partial-bigram-mask-pinyin"),
        raw!(M(&[
            ("first", B(true)),
            ("mask", B(false)),
            ("second", B(true))
        ])),
        Some(&["first", "mask", "second"]),
    );
}

// issue1114-collision-load-save-pinyin: MATCH.
#[test]
fn issue1114_collision_load_save_pinyin() {
    #[allow(unused_imports)]
    use crate::support::Raw::*;
    static PROFILE: crate::support::Raw =
        include!("fixtures/bigram_mask_prediction/profile-75410d6735c12b1b.rs");
    run_j(
        "bigram_mask_prediction::issue1114_collision_load_save_pinyin",
        Expect::RESULT,
        |k| b::case_with_profile(k, "issue1114-collision-load-save-pinyin", &PROFILE),
        raw!(include!(
            "fixtures/bigram_mask_prediction/a4ebf892de124c15.rs"
        )),
        None,
    );
}

// issue677-mask-save-sequence-pinyin: MATCH.
#[test]
fn issue677_mask_save_sequence_pinyin() {
    run_j(
        "bigram_mask_prediction::issue677_mask_save_sequence_pinyin",
        Expect::RESULT,
        |k| b::case(k, "issue677-mask-save-sequence-pinyin"),
        raw!(include!(
            "fixtures/bigram_mask_prediction/a5c1cd3c222e6c7d.rs"
        )),
        None,
    );
}

// lazy-user-bigram-pinyin: MATCH.
#[test]
fn lazy_user_bigram_pinyin() {
    #[allow(unused_imports)]
    use crate::support::Raw::*;
    static PROFILE: crate::support::Raw =
        include!("fixtures/bigram_mask_prediction/profile-851748e04e373131.rs");
    run_j(
        "bigram_mask_prediction::lazy_user_bigram_pinyin",
        Expect::RESULT,
        |k| b::case_with_profile(k, "lazy-user-bigram-pinyin", &PROFILE),
        raw!(include!(
            "fixtures/bigram_mask_prediction/3fe2bbf43c365463.rs"
        )),
        None,
    );
}

// mask-out-short-bigram-value-masked-key: MATCH.
#[test]
fn mask_out_short_bigram_value_masked_key() {
    run_j(
        "bigram_mask_prediction::mask_out_short_bigram_value_masked_key",
        Expect::RESULT,
        |k| b::case(k, "mask-out-short-bigram-value-masked-key"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// predicted-bigram-is-user-candidate: MATCH.
#[test]
fn predicted_bigram_is_user_candidate() {
    run_j(
        "bigram_mask_prediction::predicted_bigram_is_user_candidate",
        Expect::RESULT,
        |k| b::case(k, "predicted-bigram-is-user-candidate"),
        raw!(M(&[
            ("is-user", B(false)),
            ("logs", L(&[])),
            ("text", L(&[B(true), S("龘龘")])),
            ("type", L(&[B(true), I(4)]))
        ])),
        None,
    );
}
