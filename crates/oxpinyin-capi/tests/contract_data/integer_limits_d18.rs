//! Integer boundaries of remembering, unigram increments, merged bigrams, training and the bigram export, NUL-terminated remember inputs and saves over a stale temporary index (#1175, #540, #752, #542). Generated from the frozen pin observations.

use crate::bodies::integer_limits_d18 as b;
use crate::support::{Expect, run_j};

// d18-bigram-export-3fffffff: MATCH.
#[test]
fn d18_bigram_export_3fffffff() {
    run_j(
        "integer_limits_d18::d18_bigram_export_3fffffff",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-3fffffff"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(2147483646)]))
        ])),
        None,
    );
}

// d18-bigram-export-40000000: MATCH.
#[test]
fn d18_bigram_export_40000000() {
    run_j(
        "integer_limits_d18::d18_bigram_export_40000000",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-40000000"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(-2147483648)]))
        ])),
        None,
    );
}

// d18-bigram-export-7fffffff: MATCH.
#[test]
fn d18_bigram_export_7fffffff() {
    run_j(
        "integer_limits_d18::d18_bigram_export_7fffffff",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-7fffffff"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(-2)]))
        ])),
        None,
    );
}

// d18-bigram-export-80000000: MATCH.
#[test]
fn d18_bigram_export_80000000() {
    run_j(
        "integer_limits_d18::d18_bigram_export_80000000",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-80000000"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(0)]))
        ])),
        None,
    );
}

// d18-bigram-export-80000001: MATCH.
#[test]
fn d18_bigram_export_80000001() {
    run_j(
        "integer_limits_d18::d18_bigram_export_80000001",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-80000001"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(2)]))
        ])),
        None,
    );
}

// d18-bigram-export-ffffffff: MATCH.
#[test]
fn d18_bigram_export_ffffffff() {
    run_j(
        "integer_limits_d18::d18_bigram_export_ffffffff",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-ffffffff"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(-2)]))
        ])),
        None,
    );
}

// d18-merge-count-after-pinyin: MATCH.
#[test]
fn d18_merge_count_after_pinyin() {
    run_j(
        "integer_limits_d18::d18_merge_count_after_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-count-after-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/381352191c5e5c15.rs")),
        None,
    );
}

// d18-merge-count-before-pinyin: MATCH.
#[test]
fn d18_merge_count_before_pinyin() {
    run_j(
        "integer_limits_d18::d18_merge_count_before_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-count-before-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/cb57989f2b8b52df.rs")),
        None,
    );
}

// d18-merge-count-zero-pinyin: MATCH.
#[test]
fn d18_merge_count_zero_pinyin() {
    run_j(
        "integer_limits_d18::d18_merge_count_zero_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-count-zero-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/381352191c5e5c15.rs")),
        None,
    );
}

// d18-merge-total-after-pinyin: MATCH.
#[test]
fn d18_merge_total_after_pinyin() {
    run_j(
        "integer_limits_d18::d18_merge_total_after_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-total-after-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/a55f6c028c0186b1.rs")),
        None,
    );
}

// d18-merge-total-before-pinyin: MATCH.
#[test]
fn d18_merge_total_before_pinyin() {
    run_j(
        "integer_limits_d18::d18_merge_total_before_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-total-before-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/00173aa816e8302a.rs")),
        None,
    );
}

// d18-merge-total-zero-pinyin: MATCH.
#[test]
fn d18_merge_total_zero_pinyin() {
    run_j(
        "integer_limits_d18::d18_merge_total_zero_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-total-zero-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/c5e4fd2fca5d3b4d.rs")),
        None,
    );
}

// d18-predicted-total-after: MATCH.
#[test]
fn d18_predicted_total_after() {
    run_j(
        "integer_limits_d18::d18_predicted_total_after",
        Expect::RESULT,
        |k| b::case(k, "d18-predicted-total-after"),
        raw!(include!("fixtures/integer_limits_d18/a42d54eca171544f.rs")),
        None,
    );
}

// d18-predicted-total-max: MATCH.
#[test]
fn d18_predicted_total_max() {
    run_j(
        "integer_limits_d18::d18_predicted_total_max",
        Expect::RESULT,
        |k| b::case(k, "d18-predicted-total-max"),
        raw!(include!("fixtures/integer_limits_d18/ab8e48f0a7a82c5a.rs")),
        None,
    );
}

// d18-predicted-total-zero: MATCH.
#[test]
fn d18_predicted_total_zero() {
    run_j(
        "integer_limits_d18::d18_predicted_total_zero",
        Expect::RESULT,
        |k| b::case(k, "d18-predicted-total-zero"),
        raw!(include!("fixtures/integer_limits_d18/2ebd45abf8a91038.rs")),
        None,
    );
}

// d18-remember-multiply-1431655765: MATCH.
#[test]
fn d18_remember_multiply_1431655765() {
    run_j(
        "integer_limits_d18::d18_remember_multiply_1431655765",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-multiply-1431655765"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(11)),
            (
                "steps",
                L(&[L(&[
                    I(1431655765),
                    B(true),
                    L(&[L(&[
                        B(true),
                        S("你好世界"),
                        S("ni'hao'shi'jie"),
                        I(1431655765)
                    ])]),
                    L(&[B(true), I(4294967295)])
                ])])
            )
        ])),
        None,
    );
}

// d18-remember-multiply-1431655766: MATCH.
#[test]
fn d18_remember_multiply_1431655766() {
    run_j(
        "integer_limits_d18::d18_remember_multiply_1431655766",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-multiply-1431655766"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(11)),
            (
                "steps",
                L(&[L(&[
                    I(1431655766),
                    B(true),
                    L(&[L(&[
                        B(true),
                        S("你好世界"),
                        S("ni'hao'shi'jie"),
                        I(1431655766)
                    ])]),
                    L(&[B(true), I(2)])
                ])])
            )
        ])),
        None,
    );
}

// d18-remember-multiply-715827882: MATCH.
#[test]
fn d18_remember_multiply_715827882() {
    run_j(
        "integer_limits_d18::d18_remember_multiply_715827882",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-multiply-715827882"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(11)),
            (
                "steps",
                L(&[L(&[
                    I(715827882),
                    B(true),
                    L(&[L(&[
                        B(true),
                        S("你好世界"),
                        S("ni'hao'shi'jie"),
                        I(715827882)
                    ])]),
                    L(&[B(true), I(2147483646)])
                ])])
            )
        ])),
        None,
    );
}

// d18-remember-multiply-715827883: MATCH.
#[test]
fn d18_remember_multiply_715827883() {
    run_j(
        "integer_limits_d18::d18_remember_multiply_715827883",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-multiply-715827883"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(11)),
            (
                "steps",
                L(&[L(&[
                    I(715827883),
                    B(true),
                    L(&[L(&[
                        B(true),
                        S("你好世界"),
                        S("ni'hao'shi'jie"),
                        I(715827883)
                    ])]),
                    L(&[B(true), I(2147483649)])
                ])])
            )
        ])),
        None,
    );
}

// d18-remember-reported: MATCH.
#[test]
fn d18_remember_reported() {
    run_j(
        "integer_limits_d18::d18_remember_reported",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-reported"),
        raw!(include!("fixtures/integer_limits_d18/ab1d25f92d78e05e.rs")),
        None,
    );
}

// d18-remember-signed-boundary: MATCH.
#[test]
fn d18_remember_signed_boundary() {
    run_j(
        "integer_limits_d18::d18_remember_signed_boundary",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-signed-boundary"),
        raw!(include!("fixtures/integer_limits_d18/928fbd06c8ef2d28.rs")),
        None,
    );
}

// d18-review-predicted-user-only-zero-total: MATCH.
#[test]
fn d18_review_predicted_user_only_zero_total() {
    run_j(
        "integer_limits_d18::d18_review_predicted_user_only_zero_total",
        Expect::RESULT,
        |k| b::case(k, "d18-review-predicted-user-only-zero-total"),
        raw!(include!("fixtures/integer_limits_d18/c8b9bda8d522b033.rs")),
        None,
    );
}

// d18-review-unigram-library-two-pinyin: MATCH.
#[test]
fn d18_review_unigram_library_two_pinyin() {
    run_j(
        "integer_limits_d18::d18_review_unigram_library_two_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-review-unigram-library-two-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/d82c08bfbf163785.rs")),
        None,
    );
}

// d18-review-user-only-zero-total-pinyin: MATCH.
#[test]
fn d18_review_user_only_zero_total_pinyin() {
    run_j(
        "integer_limits_d18::d18_review_user_only_zero_total_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-review-user-only-zero-total-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/267bcc58c423e621.rs")),
        None,
    );
}

// d18-review-zero-count-zero-total-default-pinyin: MATCH.
#[test]
fn d18_review_zero_count_zero_total_default_pinyin() {
    run_j(
        "integer_limits_d18::d18_review_zero_count_zero_total_default_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-review-zero-count-zero-total-default-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/381352191c5e5c15.rs")),
        None,
    );
}

// d18-review-zero-count-zero-total-half-pinyin: MATCH.
#[test]
fn d18_review_zero_count_zero_total_half_pinyin() {
    run_j(
        "integer_limits_d18::d18_review_zero_count_zero_total_half_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-review-zero-count-zero-total-half-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/7a0c463ceb1b2cdc.rs")),
        None,
    );
}

// d18-review-zero-count-zero-total-zero-lambda-pinyin: MATCH.
#[test]
fn d18_review_zero_count_zero_total_zero_lambda_pinyin() {
    run_j(
        "integer_limits_d18::d18_review_zero_count_zero_total_zero_lambda_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-review-zero-count-zero-total-zero-lambda-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/4f55018da8693b03.rs")),
        None,
    );
}

// d18-train-seed-after-pinyin: MATCH.
#[test]
fn d18_train_seed_after_pinyin() {
    run_j(
        "integer_limits_d18::d18_train_seed_after_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-seed-after-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/ac21d3fe889e1846.rs")),
        None,
    );
}

// d18-train-seed-before-pinyin: MATCH.
#[test]
fn d18_train_seed_before_pinyin() {
    run_j(
        "integer_limits_d18::d18_train_seed_before_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-seed-before-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/94da3805ec14323f.rs")),
        None,
    );
}

// d18-train-seed-zero-pinyin: MATCH.
#[test]
fn d18_train_seed_zero_pinyin() {
    run_j(
        "integer_limits_d18::d18_train_seed_zero_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-seed-zero-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/f76d84995b8eec0c.rs")),
        None,
    );
}

// d18-train-stored-zero-pinyin: MATCH.
#[test]
fn d18_train_stored_zero_pinyin() {
    run_j(
        "integer_limits_d18::d18_train_stored_zero_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-stored-zero-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/28798fd0445782e8.rs")),
        None,
    );
}

// d18-train-total-max-pinyin: MATCH.
#[test]
fn d18_train_total_max_pinyin() {
    run_j(
        "integer_limits_d18::d18_train_total_max_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-total-max-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/7b62ef743d990c08.rs")),
        None,
    );
}

// d18-train-total-overflow-pinyin: MATCH.
#[test]
fn d18_train_total_overflow_pinyin() {
    run_j(
        "integer_limits_d18::d18_train_total_overflow_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-total-overflow-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/907723db5a4587c8.rs")),
        None,
    );
}

// d18-unigram-max-pinyin: MATCH.
#[test]
fn d18_unigram_max_pinyin() {
    run_j(
        "integer_limits_d18::d18_unigram_max_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-unigram-max-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/b92e1a001a2c597b.rs")),
        None,
    );
}

// d18-unigram-total-boundary-pinyin: MATCH.
#[test]
fn d18_unigram_total_boundary_pinyin() {
    run_j(
        "integer_limits_d18::d18_unigram_total_boundary_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-unigram-total-boundary-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/4d52ca68da4a63d4.rs")),
        None,
    );
}

// issue542-stale-user_phrase_index.bin-pinyin: MATCH.
#[test]
fn issue542_stale_user_phrase_index_dot_bin_pinyin() {
    // g_rename is MoveFileExW with MOVEFILE_REPLACE_EXISTING (gstdio.c, GLib 2.90.1): a directory over an existing directory is an error (Microsoft Learn, MoveFileExW), so the empty-directory round's rename fails and its stale directory stays; a directory over an existing file is refused as rename(2) refuses it (compatibility-policy.md, 2026-11-03 UTC; oxpinyin-input path::rename), so the live-file round ends as the pin's did. Both failing rounds print the rename line, and text-mode stderr writes \n as \r\n.
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/integer_limits_d18/1c57f44b3d1ec4c0.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/integer_limits_d18/7ca7480deb427fe4.rs"));
    run_j(
        "integer_limits_d18::issue542_stale_user_phrase_index_dot_bin_pinyin",
        // The rename line comes from the second and the third round (see the result above), and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_phrase_index.bin.tmp to SCRATCH\\user-X\\user_phrase_index.bin failed.\r\nopen SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_phrase_index.bin.tmp to SCRATCH\\user-X\\user_phrase_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_phrase_index.bin.tmp to SCRATCH/user-X/user_phrase_index.bin failed.\n"
        }),
        |k| b::case(k, "issue542-stale-user_phrase_index.bin-pinyin"),
        expected,
        None,
    );
}

// issue542-stale-user_pinyin_index.bin-pinyin: MATCH.
#[test]
fn issue542_stale_user_pinyin_index_dot_bin_pinyin() {
    // g_rename is MoveFileExW with MOVEFILE_REPLACE_EXISTING (gstdio.c, GLib 2.90.1): a directory over an existing directory is an error (Microsoft Learn, MoveFileExW), so the empty-directory round's rename fails and its stale directory stays; a directory over an existing file is refused as rename(2) refuses it (compatibility-policy.md, 2026-11-03 UTC; oxpinyin-input path::rename), so the live-file round ends as the pin's did. Both failing rounds print the rename line, and text-mode stderr writes \n as \r\n.
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/integer_limits_d18/1c57f44b3d1ec4c0.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/integer_limits_d18/7ca7480deb427fe4.rs"));
    run_j(
        "integer_limits_d18::issue542_stale_user_pinyin_index_dot_bin_pinyin",
        // The rename line comes from the second and the third round (see the result above), and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\nopen SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\n"
        }),
        |k| b::case(k, "issue542-stale-user_pinyin_index.bin-pinyin"),
        expected,
        None,
    );
}

// remember-known-nul-suffix-pinyin: MATCH.
#[test]
fn remember_known_nul_suffix_pinyin() {
    run_j(
        "integer_limits_d18::remember_known_nul_suffix_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "remember-known-nul-suffix-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("ret", B(true)),
            ("rows", L(&[L(&[B(true), S("你好"), S("ni'hao"), I(1)])]))
        ])),
        None,
    );
}

// remember-nul-chewing-pinyin: MATCH.
#[test]
fn remember_nul_chewing_pinyin() {
    run_j(
        "integer_limits_d18::remember_nul_chewing_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "remember-nul-chewing-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(6)),
            ("ret", B(false)),
            ("rows", L(&[]))
        ])),
        None,
    );
}

// remember-nul-double-pinyin: MATCH.
#[test]
fn remember_nul_double_pinyin() {
    run_j(
        "integer_limits_d18::remember_nul_double_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "remember-nul-double-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(4)),
            ("ret", B(false)),
            ("rows", L(&[]))
        ])),
        None,
    );
}
