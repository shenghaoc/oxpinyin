//! User grams stored, trained, masked, removed and predicted in their stored order, duplicates kept, and an item-less user start total merged with the system's (#1175, #757, #791). Generated from the frozen pin observations.

use crate::bodies::stored_user_gram as b;
use crate::support::{Expect, run_j};

// abort-train-unsorted-set-freq-miss-pinyin: POLICY-ACCEPTED (1297-gram-partition-training.md).
#[test]
fn abort_train_unsorted_set_freq_miss_pinyin() {
    run_j(
        "stored_user_gram::abort_train_unsorted_set_freq_miss_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "abort-train-unsorted-set-freq-miss-pinyin"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// itemless-user-total-guess: MATCH.
#[test]
fn itemless_user_total_guess() {
    run_j(
        "stored_user_gram::itemless_user_total_guess",
        Expect::RESULT,
        |k| b::case(k, "itemless-user-total-guess"),
        raw!(include!("fixtures/stored_user_gram/0bd81d1e784aa8dd.rs")),
        None,
    );
}

// itemless-user-total-guess-above-one: MATCH.
#[test]
fn itemless_user_total_guess_above_one() {
    run_j(
        "stored_user_gram::itemless_user_total_guess_above_one",
        Expect::RESULT,
        |k| b::case(k, "itemless-user-total-guess-above-one"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "yanjiushengmingqiyuan",
                L(&[
                    B(true),
                    L(&[
                        L(&[B(true), S("研究生命起源")]),
                        L(&[B(true), S("研究生命起源")]),
                        L(&[B(true), S("研究生名气元")])
                    ])
                ])
            )
        ])),
        None,
    );
}

// itemless-user-total-guess-above-one-control: MATCH.
#[test]
fn itemless_user_total_guess_above_one_control() {
    run_j(
        "stored_user_gram::itemless_user_total_guess_above_one_control",
        Expect::RESULT,
        |k| b::case(k, "itemless-user-total-guess-above-one-control"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "yanjiushengmingqiyuan",
                L(&[
                    B(true),
                    L(&[
                        L(&[B(true), S("研究生命起源")]),
                        L(&[B(true), S("研究生命起源")]),
                        L(&[B(true), S("研究生名气元")])
                    ])
                ])
            )
        ])),
        None,
    );
}

// itemless-user-total-phrase-segment: MATCH.
#[test]
fn itemless_user_total_phrase_segment() {
    run_j(
        "stored_user_gram::itemless_user_total_phrase_segment",
        Expect::RESULT,
        |k| b::case(k, "itemless-user-total-phrase-segment"),
        raw!(include!("fixtures/stored_user_gram/e176a9ca4e67bca6.rs")),
        None,
    );
}

// mask-out-itemless-gram-untouched-pinyin-total-nonzero: MATCH.
#[test]
fn mask_out_itemless_gram_untouched_pinyin_total_nonzero() {
    run_j(
        "stored_user_gram::mask_out_itemless_gram_untouched_pinyin_total_nonzero",
        Expect::RESULT,
        |k| b::case(k, "mask-out-itemless-gram-untouched-pinyin-total-nonzero"),
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

// mask-out-itemless-gram-untouched-pinyin-total-zero: MATCH.
#[test]
fn mask_out_itemless_gram_untouched_pinyin_total_zero() {
    run_j(
        "stored_user_gram::mask_out_itemless_gram_untouched_pinyin_total_zero",
        Expect::RESULT,
        |k| b::case(k, "mask-out-itemless-gram-untouched-pinyin-total-zero"),
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

// mask-out-itemless-gram-untouched-remove-candidate-total-nonzero: MATCH.
#[test]
fn mask_out_itemless_gram_untouched_remove_candidate_total_nonzero() {
    run_j(
        "stored_user_gram::mask_out_itemless_gram_untouched_remove_candidate_total_nonzero",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "mask-out-itemless-gram-untouched-remove-candidate-total-nonzero",
            )
        },
        raw!(M(&[
            (
                "grams",
                L(&[
                    L(&[I(70), L(&[L(&[I(16779429), I(70)])])]),
                    L(&[I(17), L(&[])])
                ])
            ),
            ("logs", L(&[])),
            ("removed", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// mask-out-itemless-gram-untouched-remove-candidate-total-zero: MATCH.
#[test]
fn mask_out_itemless_gram_untouched_remove_candidate_total_zero() {
    run_j(
        "stored_user_gram::mask_out_itemless_gram_untouched_remove_candidate_total_zero",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "mask-out-itemless-gram-untouched-remove-candidate-total-zero",
            )
        },
        raw!(M(&[
            (
                "grams",
                L(&[
                    L(&[I(70), L(&[L(&[I(16779429), I(70)])])]),
                    L(&[I(0), L(&[])])
                ])
            ),
            ("logs", L(&[])),
            ("removed", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// user-gram-duplicate-mask-save-pinyin: MATCH.
#[test]
fn user_gram_duplicate_mask_save_pinyin() {
    run_j(
        "stored_user_gram::user_gram_duplicate_mask_save_pinyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-mask-save-pinyin"),
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

// user-gram-duplicate-predicted: MATCH.
#[test]
fn user_gram_duplicate_predicted() {
    run_j(
        "stored_user_gram::user_gram_duplicate_predicted",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-predicted"),
        raw!(include!("fixtures/stored_user_gram/d559ec21dbf0c9a1.rs")),
        None,
    );
}

// user-gram-duplicate-remove-candidate-save: MATCH.
#[test]
fn user_gram_duplicate_remove_candidate_save() {
    run_j(
        "stored_user_gram::user_gram_duplicate_remove_candidate_save",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-remove-candidate-save"),
        raw!(M(&[
            (
                "grams",
                L(&[L(&[
                    I(150),
                    L(&[L(&[I(16779429), I(70)]), L(&[I(16779429), I(80)])])
                ])])
            ),
            ("logs", L(&[])),
            ("removed", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// user-gram-duplicate-train-save-pinyin: MATCH.
#[test]
fn user_gram_duplicate_train_save_pinyin() {
    run_j(
        "stored_user_gram::user_gram_duplicate_train_save_pinyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-train-save-pinyin"),
        raw!(include!("fixtures/stored_user_gram/e65ebdf2512fa063.rs")),
        None,
    );
}

// user-gram-unsorted-mask-save-pinyin: MATCH.
#[test]
fn user_gram_unsorted_mask_save_pinyin() {
    run_j(
        "stored_user_gram::user_gram_unsorted_mask_save_pinyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-mask-save-pinyin"),
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

// user-gram-unsorted-predicted: MATCH.
#[test]
fn user_gram_unsorted_predicted() {
    run_j(
        "stored_user_gram::user_gram_unsorted_predicted",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-predicted"),
        raw!(include!("fixtures/stored_user_gram/511e9651452771f9.rs")),
        None,
    );
}

// user-gram-unsorted-remove-candidate-save: MATCH.
#[test]
fn user_gram_unsorted_remove_candidate_save() {
    run_j(
        "stored_user_gram::user_gram_unsorted_remove_candidate_save",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-remove-candidate-save"),
        raw!(M(&[
            (
                "grams",
                L(&[L(&[
                    I(150),
                    L(&[L(&[I(16779448), I(80)]), L(&[I(16779429), I(70)])])
                ])])
            ),
            ("logs", L(&[])),
            ("removed", B(true)),
            ("save", B(true)),
            ("trained", B(true))
        ])),
        None,
    );
}

// user-gram-unsorted-train-save-pinyin: MATCH.
#[test]
fn user_gram_unsorted_train_save_pinyin() {
    run_j(
        "stored_user_gram::user_gram_unsorted_train_save_pinyin",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-train-save-pinyin"),
        raw!(include!("fixtures/stored_user_gram/f003b281b6927a4c.rs")),
        None,
    );
}
