//! Integer boundaries of remembering, unigram increments, merged bigrams, training and the bigram export, NUL-terminated remember inputs and saves over a stale temporary index (#1175, #540, #752, #542). Generated from the frozen pin observations.

use crate::bodies::integer_limits_d18 as b;
use crate::support::{Expect, run_j};

// d18-merge-count-zero-zhuyin: MATCH.
#[test]
fn d18_merge_count_zero_zhuyin() {
    run_j(
        "integer_limits_d18::d18_merge_count_zero_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-count-zero-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/07ed9eebc10f04ce.rs")),
        None,
    );
}

// d18-merge-total-zero-zhuyin: MATCH.
#[test]
fn d18_merge_total_zero_zhuyin() {
    run_j(
        "integer_limits_d18::d18_merge_total_zero_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-total-zero-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/e5f32ea4d47a3bc3.rs")),
        None,
    );
}

// d18-review-user-only-zero-total-zhuyin: MATCH.
#[test]
fn d18_review_user_only_zero_total_zhuyin() {
    run_j(
        "integer_limits_d18::d18_review_user_only_zero_total_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-review-user-only-zero-total-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/db501ff6eee3e6ba.rs")),
        None,
    );
}

// d18-review-zero-count-zero-total-default-zhuyin: MATCH.
#[test]
fn d18_review_zero_count_zero_total_default_zhuyin() {
    run_j(
        "integer_limits_d18::d18_review_zero_count_zero_total_default_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-review-zero-count-zero-total-default-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/2b9440e218b0a6f2.rs")),
        None,
    );
}

// d18-train-seed-zero-zhuyin: MATCH.
#[test]
fn d18_train_seed_zero_zhuyin() {
    run_j(
        "integer_limits_d18::d18_train_seed_zero_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-seed-zero-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/f76d84995b8eec0c.rs")),
        None,
    );
}

// d18-train-stored-zero-zhuyin: MATCH.
#[test]
fn d18_train_stored_zero_zhuyin() {
    run_j(
        "integer_limits_d18::d18_train_stored_zero_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-stored-zero-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/28798fd0445782e8.rs")),
        None,
    );
}

// d18-train-total-overflow-zhuyin: MATCH.
#[test]
fn d18_train_total_overflow_zhuyin() {
    run_j(
        "integer_limits_d18::d18_train_total_overflow_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-total-overflow-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/907723db5a4587c8.rs")),
        None,
    );
}

// d18-unigram-total-boundary-zhuyin: MATCH.
#[test]
fn d18_unigram_total_boundary_zhuyin() {
    run_j(
        "integer_limits_d18::d18_unigram_total_boundary_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "d18-unigram-total-boundary-zhuyin"),
        raw!(include!("fixtures/integer_limits_d18/4d52ca68da4a63d4.rs")),
        None,
    );
}

// issue542-stale-user_phrase_index.bin-zhuyin: MATCH.
#[test]
fn issue542_stale_user_phrase_index_dot_bin_zhuyin() {
    // g_rename is MoveFileExW with MOVEFILE_REPLACE_EXISTING (gstdio.c, GLib 2.90.1): a directory over an existing directory is an error (Microsoft Learn, MoveFileExW), so the empty-directory round's rename fails and its stale directory stays; a directory over an existing file is refused as rename(2) refuses it (compatibility-policy.md, 2026-11-03 UTC; oxpinyin-input path::rename), so the live-file round ends as the pin's did. Both failing rounds print the rename line, and text-mode stderr writes \n as \r\n.
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/integer_limits_d18/1c57f44b3d1ec4c0.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/integer_limits_d18/7ca7480deb427fe4.rs"));
    run_j(
        "integer_limits_d18::issue542_stale_user_phrase_index_dot_bin_zhuyin",
        // The rename line comes from the second and the third round (see the result above), and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_phrase_index.bin.tmp to SCRATCH\\user-X\\user_phrase_index.bin failed.\r\nopen SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_phrase_index.bin.tmp to SCRATCH\\user-X\\user_phrase_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_phrase_index.bin.tmp to SCRATCH/user-X/user_phrase_index.bin failed.\n"
        }),
        |k| b::case(k, "issue542-stale-user_phrase_index.bin-zhuyin"),
        expected,
        None,
    );
}

// issue542-stale-user_pinyin_index.bin-zhuyin: MATCH.
#[test]
fn issue542_stale_user_pinyin_index_dot_bin_zhuyin() {
    // g_rename is MoveFileExW with MOVEFILE_REPLACE_EXISTING (gstdio.c, GLib 2.90.1): a directory over an existing directory is an error (Microsoft Learn, MoveFileExW), so the empty-directory round's rename fails and its stale directory stays; a directory over an existing file is refused as rename(2) refuses it (compatibility-policy.md, 2026-11-03 UTC; oxpinyin-input path::rename), so the live-file round ends as the pin's did. Both failing rounds print the rename line, and text-mode stderr writes \n as \r\n.
    #[cfg(windows)]
    let expected = raw!(include!("fixtures/integer_limits_d18/1c57f44b3d1ec4c0.rs"));
    #[cfg(not(windows))]
    let expected = raw!(include!("fixtures/integer_limits_d18/7ca7480deb427fe4.rs"));
    run_j(
        "integer_limits_d18::issue542_stale_user_pinyin_index_dot_bin_zhuyin",
        // The rename line comes from the second and the third round (see the result above), and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed.open SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\nopen SCRATCH\\user-X\\user.conf failed.rename SCRATCH\\user-X\\user_pinyin_index.bin.tmp to SCRATCH\\user-X\\user_pinyin_index.bin failed.\r\n"
        } else {
            b"open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.open SCRATCH/user-X/user.conf failed.rename SCRATCH/user-X/user_pinyin_index.bin.tmp to SCRATCH/user-X/user_pinyin_index.bin failed.\n"
        }),
        |k| b::case(k, "issue542-stale-user_pinyin_index.bin-zhuyin"),
        expected,
        None,
    );
}
