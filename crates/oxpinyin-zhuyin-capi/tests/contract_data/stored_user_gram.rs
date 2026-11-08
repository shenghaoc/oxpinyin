//! User grams stored, trained, masked, removed and predicted in their stored order, duplicates kept, and an item-less user start total merged with the system's (#1175, #757, #791). Generated from the frozen pin observations.

use crate::bodies::stored_user_gram as b;
use crate::support::{Expect, run_j};

// abort-train-unsorted-set-freq-miss-zhuyin: POLICY-ACCEPTED (1297-gram-partition-training.md).
#[test]
fn abort_train_unsorted_set_freq_miss_zhuyin() {
    run_j(
        "stored_user_gram::abort_train_unsorted_set_freq_miss_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "abort-train-unsorted-set-freq-miss-zhuyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// mask-out-itemless-gram-untouched-zhuyin-total-nonzero: MATCH.
#[test]
fn mask_out_itemless_gram_untouched_zhuyin_total_nonzero() {
    run_j(
        "stored_user_gram::mask_out_itemless_gram_untouched_zhuyin_total_nonzero",
        Expect::RESULT,
        |k| b::case(k, "mask-out-itemless-gram-untouched-zhuyin-total-nonzero"),
        raw!(M(&[
            (
                "grams",
                L(&[
                    L(&[I(70), L(&[L(&[I(16779429), I(70)])])]),
                    L(&[I(17), L(&[])])
                ])
            ),
            ("logs", L(&[])),
            ("masked", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// mask-out-itemless-gram-untouched-zhuyin-total-zero: MATCH.
#[test]
fn mask_out_itemless_gram_untouched_zhuyin_total_zero() {
    run_j(
        "stored_user_gram::mask_out_itemless_gram_untouched_zhuyin_total_zero",
        Expect::RESULT,
        |k| b::case(k, "mask-out-itemless-gram-untouched-zhuyin-total-zero"),
        raw!(M(&[
            (
                "grams",
                L(&[
                    L(&[I(70), L(&[L(&[I(16779429), I(70)])])]),
                    L(&[I(0), L(&[])])
                ])
            ),
            ("logs", L(&[])),
            ("masked", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// user-gram-duplicate-mask-save-zhuyin: MATCH.
#[test]
fn user_gram_duplicate_mask_save_zhuyin() {
    run_j(
        "stored_user_gram::user_gram_duplicate_mask_save_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-mask-save-zhuyin"),
        raw!(M(&[
            (
                "grams",
                L(&[L(&[
                    I(190),
                    L(&[L(&[I(117440513), I(90)]), L(&[I(117440513), I(100)])])
                ])])
            ),
            ("logs", L(&[])),
            ("masked", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// user-gram-duplicate-train-save-zhuyin: MATCH.
#[test]
fn user_gram_duplicate_train_save_zhuyin() {
    run_j(
        "stored_user_gram::user_gram_duplicate_train_save_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-train-save-zhuyin"),
        raw!(include!("fixtures/stored_user_gram/e65ebdf2512fa063.rs")),
        None,
    );
}

// user-gram-unsorted-mask-save-zhuyin: MATCH.
#[test]
fn user_gram_unsorted_mask_save_zhuyin() {
    run_j(
        "stored_user_gram::user_gram_unsorted_mask_save_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-mask-save-zhuyin"),
        raw!(M(&[
            (
                "grams",
                L(&[L(&[
                    I(150),
                    L(&[L(&[I(16779448), I(80)]), L(&[I(16779429), I(70)])])
                ])])
            ),
            ("logs", L(&[])),
            ("masked", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// user-gram-unsorted-train-save-zhuyin: MATCH.
#[test]
fn user_gram_unsorted_train_save_zhuyin() {
    run_j(
        "stored_user_gram::user_gram_unsorted_train_save_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-train-save-zhuyin"),
        raw!(include!("fixtures/stored_user_gram/f003b281b6927a4c.rs")),
        None,
    );
}
