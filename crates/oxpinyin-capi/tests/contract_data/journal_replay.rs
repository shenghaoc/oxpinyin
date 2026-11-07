//! Journal replay, removed items and chunk frames (#1175, #1623). Generated from the frozen pin observations.

use crate::bodies::journal_replay as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// chunk-frame-journal-exact-pinyin: MATCH.
#[test]
fn chunk_frame_journal_exact_pinyin() {
    run_j(
        "journal_replay::chunk_frame_journal_exact_pinyin",
        Expect::RESULT,
        |k| b::case(k, "chunk-frame-journal-exact-pinyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("logs", L(&[])),
            ("unigram", L(&[B(true), I(42)]))
        ])),
        None,
    );
}

// chunk-frame-journal-trailing-pinyin: MATCH.
#[test]
fn chunk_frame_journal_trailing_pinyin() {
    run_j(
        "journal_replay::chunk_frame_journal_trailing_pinyin",
        Expect::RESULT,
        |k| b::case(k, "chunk-frame-journal-trailing-pinyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("logs", L(&[])),
            ("unigram", L(&[B(true), I(3)]))
        ])),
        None,
    );
}

// chunk-frame-user-exact-pinyin: MATCH.
#[test]
fn chunk_frame_user_exact_pinyin() {
    run_j(
        "journal_replay::chunk_frame_user_exact_pinyin",
        Expect::RESULT,
        |k| b::case(k, "chunk-frame-user-exact-pinyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("logs", L(&[])),
            ("unigram", L(&[B(true), I(15)]))
        ])),
        None,
    );
}

// chunk-frame-user-trailing-pinyin: MATCH.
#[test]
fn chunk_frame_user_trailing_pinyin() {
    run_j(
        "journal_replay::chunk_frame_user_trailing_pinyin",
        Expect::RESULT,
        |k| b::case(k, "chunk-frame-user-trailing-pinyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("logs", L(&[])),
            ("unigram", L(&[B(false), I(0)]))
        ])),
        None,
    );
}

// removed-candidate-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn removed_candidate_pinyin() {
    run_j(
        "journal_replay::removed_candidate_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "removed-candidate-pinyin"),
        raw!(M(&[
            ("exact-candidates-ret", B(true)),
            ("exact-guessed", B(true)),
            ("removed", L(&[B(false), I(0), Nil]))
        ])),
        Some(&["exact-candidates-ret", "exact-guessed", "removed"]),
    );
}

// removed-extension-candidates-pinyin: MATCH.
#[test]
fn removed_extension_candidates_pinyin() {
    run_j(
        "journal_replay::removed_extension_candidates_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "removed-extension-candidates-pinyin"),
        raw!(include!("fixtures/journal_replay/128aa68a47e03eed.rs")),
        None,
    );
}

// removed-extension-sentence-pinyin: MATCH.
#[test]
fn removed_extension_sentence_pinyin() {
    run_j(
        "journal_replay::removed_extension_sentence_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "removed-extension-sentence-pinyin"),
        raw!(include!("fixtures/journal_replay/9ef35a02689cb87e.rs")),
        None,
    );
}

// removed-predict-choose-pinyin: MATCH.
#[test]
fn removed_predict_choose_pinyin() {
    run_j(
        "journal_replay::removed_predict_choose_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "removed-predict-choose-pinyin"),
        raw!(include!("fixtures/journal_replay/bc5b0cd953291605.rs")),
        None,
    );
}

// removed-predict-class-b-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn removed_predict_class_b_pinyin() {
    run_j(
        "journal_replay::removed_predict_class_b_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "removed-predict-class-b-pinyin"),
        raw!(M(&[("count", I(1)), ("ret", B(true))])),
        Some(&["count", "ret"]),
    );
}

// removed-predict-sole-pinyin: MATCH.
#[test]
fn removed_predict_sole_pinyin() {
    run_j(
        "journal_replay::removed_predict_sole_pinyin",
        Expect::stderr(b"open SCRATCH/user-X/user.conf failed."),
        |k| b::case(k, "removed-predict-sole-pinyin"),
        raw!(include!("fixtures/journal_replay/90d4fdf2276dca57.rs")),
        None,
    );
}

// replay-cross-item-unigram-unrecorded-past-area-pinyin: MATCH.
#[test]
fn replay_cross_item_unigram_unrecorded_past_area_pinyin() {
    run_j(
        "journal_replay::replay_cross_item_unigram_unrecorded_past_area_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-cross-item-unigram-unrecorded-past-area-pinyin"),
        raw!(include!("fixtures/journal_replay/c56d17b24c3907a1.rs")),
        None,
    );
}

// replay-cross-item-unigram-unrecorded-pinyin: MATCH.
#[test]
fn replay_cross_item_unigram_unrecorded_pinyin() {
    run_j(
        "journal_replay::replay_cross_item_unigram_unrecorded_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-cross-item-unigram-unrecorded-pinyin"),
        raw!(include!("fixtures/journal_replay/e4afbc4e895b6038.rs")),
        None,
    );
}

// replay-cross-item-unigram-unrecorded-reopen-pinyin: MATCH.
#[test]
fn replay_cross_item_unigram_unrecorded_reopen_pinyin() {
    run_j(
        "journal_replay::replay_cross_item_unigram_unrecorded_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-cross-item-unigram-unrecorded-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/5f33b00c3da7c878.rs")),
        None,
    );
}

// replay-failed-remove-pinyin: MATCH.
#[test]
fn replay_failed_remove_pinyin() {
    run_j(
        "journal_replay::replay_failed_remove_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-failed-remove-pinyin"),
        raw!(include!("fixtures/journal_replay/3f801cbcaa5c7f1a.rs")),
        None,
    );
}

// replay-matched-remove-pinyin: MATCH.
#[test]
fn replay_matched_remove_pinyin() {
    run_j(
        "journal_replay::replay_matched_remove_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-matched-remove-pinyin"),
        raw!(include!("fixtures/journal_replay/376a97cfe08c4ada.rs")),
        None,
    );
}

// replay-matched-remove-reopen-pinyin: MATCH.
#[test]
fn replay_matched_remove_reopen_pinyin() {
    run_j(
        "journal_replay::replay_matched_remove_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-matched-remove-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/dbb6d973385ad6b4.rs")),
        None,
    );
}

// replay-mismatch-then-valid-pinyin: MATCH.
#[test]
fn replay_mismatch_then_valid_pinyin() {
    run_j(
        "journal_replay::replay_mismatch_then_valid_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-mismatch-then-valid-pinyin"),
        raw!(include!("fixtures/journal_replay/93f47456388700b8.rs")),
        None,
    );
}

// replay-modify-absent-default-grow-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_modify_absent_default_grow_pinyin() {
    run_j(
        "journal_replay::replay_modify_absent_default_grow_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-absent-default-grow-pinyin"),
        raw!(include!("fixtures/journal_replay/067ee38fbe26671d.rs")),
        Some(&["absent", "ctx", "fresh", "one"]),
    );
}

// replay-modify-absent-default-pinyin: MATCH.
#[test]
fn replay_modify_absent_default_pinyin() {
    run_j(
        "journal_replay::replay_modify_absent_default_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-absent-default-pinyin"),
        raw!(include!("fixtures/journal_replay/03cbbd3ff5d908ee.rs")),
        None,
    );
}

// replay-modify-absent-pinyin: MATCH.
#[test]
fn replay_modify_absent_pinyin() {
    run_j(
        "journal_replay::replay_modify_absent_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-absent-pinyin"),
        raw!(include!("fixtures/journal_replay/5cafb055da812f34.rs")),
        None,
    );
}

// replay-modify-absent-stale-pinyin: MATCH.
#[test]
fn replay_modify_absent_stale_pinyin() {
    run_j(
        "journal_replay::replay_modify_absent_stale_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-absent-stale-pinyin"),
        raw!(include!("fixtures/journal_replay/34320e38ec2bcaa9.rs")),
        None,
    );
}

// replay-modify-empty-phrase-pinyin: MATCH.
#[test]
fn replay_modify_empty_phrase_pinyin() {
    run_j(
        "journal_replay::replay_modify_empty_phrase_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-empty-phrase-pinyin"),
        raw!(include!("fixtures/journal_replay/23796b91e69d5e3e.rs")),
        None,
    );
}

// replay-modify-header-overlong-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_modify_header_overlong_pinyin() {
    run_j(
        "journal_replay::replay_modify_header_overlong_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-header-overlong-pinyin"),
        raw!(include!("fixtures/journal_replay/67e37978e3b3d97f.rs")),
        Some(&["ctx", "fresh", "last", "one"]),
    );
}

// replay-modify-header-overlong-small-pinyin: MATCH.
#[test]
fn replay_modify_header_overlong_small_pinyin() {
    run_j(
        "journal_replay::replay_modify_header_overlong_small_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-header-overlong-small-pinyin"),
        raw!(include!("fixtures/journal_replay/41323850eb639e8e.rs")),
        None,
    );
}

// replay-modify-short-in-place-pinyin: MATCH.
#[test]
fn replay_modify_short_in_place_pinyin() {
    run_j(
        "journal_replay::replay_modify_short_in_place_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-short-in-place-pinyin"),
        raw!(include!("fixtures/journal_replay/673b35be30cf58bf.rs")),
        None,
    );
}

// replay-modify-trailing-new-grow-pinyin: MATCH.
#[test]
fn replay_modify_trailing_new_grow_pinyin() {
    run_j(
        "journal_replay::replay_modify_trailing_new_grow_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-trailing-new-grow-pinyin"),
        raw!(include!("fixtures/journal_replay/eda4566b3f6ad402.rs")),
        None,
    );
}

// replay-modify-trailing-new-grow-stored-old-pinyin: MATCH.
#[test]
fn replay_modify_trailing_new_grow_stored_old_pinyin() {
    run_j(
        "journal_replay::replay_modify_trailing_new_grow_stored_old_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-trailing-new-grow-stored-old-pinyin"),
        raw!(include!("fixtures/journal_replay/806b07e2185f5d5f.rs")),
        None,
    );
}

// replay-modify-trailing-new-in-place-pinyin: MATCH.
#[test]
fn replay_modify_trailing_new_in_place_pinyin() {
    run_j(
        "journal_replay::replay_modify_trailing_new_in_place_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-trailing-new-in-place-pinyin"),
        raw!(include!("fixtures/journal_replay/fbec8a2af84b107d.rs")),
        None,
    );
}

// replay-modify-trailing-new-in-place-stored-old-pinyin: MATCH.
#[test]
fn replay_modify_trailing_new_in_place_stored_old_pinyin() {
    run_j(
        "journal_replay::replay_modify_trailing_new_in_place_stored_old_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-trailing-new-in-place-stored-old-pinyin"),
        raw!(include!("fixtures/journal_replay/9d1d2d21bd7450b3.rs")),
        None,
    );
}

// replay-modify-trailing-old-pinyin: MATCH.
#[test]
fn replay_modify_trailing_old_pinyin() {
    run_j(
        "journal_replay::replay_modify_trailing_old_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-trailing-old-pinyin"),
        raw!(include!("fixtures/journal_replay/06add0f4fc955bb0.rs")),
        None,
    );
}

// replay-over-read-tail-pinyin: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn replay_over_read_tail_pinyin() {
    run_j(
        "journal_replay::replay_over_read_tail_pinyin",
        Expect::RESULT,
        |k| b::case(k, "replay-over-read-tail-pinyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("export", L(&[L(&[B(true), S("锕"), S("a"), I(7)])])),
            (
                "one",
                L(&[L(&[B(true), I(1), S("锕")]), L(&[B(true), I(3)])])
            )
        ])),
        Some(&["ctx", "export", "one"]),
    );
}

// replay-pron-import-dup-both-pinyin: MATCH.
#[test]
fn replay_pron_import_dup_both_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_dup_both_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-dup-both-pinyin"),
        raw!(include!("fixtures/journal_replay/3fe10cc176724eda.rs")),
        None,
    );
}

// replay-pron-import-dup-equal-pinyin: MATCH.
#[test]
fn replay_pron_import_dup_equal_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_dup_equal_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-dup-equal-pinyin"),
        raw!(include!("fixtures/journal_replay/e010f756b3169fe6.rs")),
        None,
    );
}

// replay-pron-import-dup-raised-first-pinyin: MATCH.
#[test]
fn replay_pron_import_dup_raised_first_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_dup_raised_first_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-dup-raised-first-pinyin"),
        raw!(include!("fixtures/journal_replay/6d12bb3b8de19cc5.rs")),
        None,
    );
}

// replay-pron-import-dup-raised-last-pinyin: MATCH.
#[test]
fn replay_pron_import_dup_raised_last_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_dup_raised_last_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-dup-raised-last-pinyin"),
        raw!(include!("fixtures/journal_replay/94ac6453eced5f79.rs")),
        None,
    );
}

// replay-pron-import-lowered-pinyin: MATCH.
#[test]
fn replay_pron_import_lowered_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_lowered_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-lowered-pinyin"),
        raw!(include!("fixtures/journal_replay/5fdb858ecf4888b7.rs")),
        None,
    );
}

// replay-pron-import-other-first-pinyin: MATCH.
#[test]
fn replay_pron_import_other_first_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_other_first_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-other-first-pinyin"),
        raw!(include!("fixtures/journal_replay/666634a7e419125b.rs")),
        None,
    );
}

// replay-pron-import-other-reading-pinyin: MATCH.
#[test]
fn replay_pron_import_other_reading_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_other_reading_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-other-reading-pinyin"),
        raw!(include!("fixtures/journal_replay/0fccb027a7dc08ac.rs")),
        None,
    );
}

// replay-pron-import-raised-pinyin: MATCH.
#[test]
fn replay_pron_import_raised_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_raised_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-raised-pinyin"),
        raw!(include!("fixtures/journal_replay/c45c375036843fc1.rs")),
        None,
    );
}

// replay-pron-import-unigram-only-pinyin: MATCH.
#[test]
fn replay_pron_import_unigram_only_pinyin() {
    run_j(
        "journal_replay::replay_pron_import_unigram_only_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-unigram-only-pinyin"),
        raw!(include!("fixtures/journal_replay/1c6746fe7fecd6b4.rs")),
        None,
    );
}

// replay-pron-mask-dup-both-pinyin: MATCH.
#[test]
fn replay_pron_mask_dup_both_pinyin() {
    run_j(
        "journal_replay::replay_pron_mask_dup_both_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-mask-dup-both-pinyin"),
        raw!(include!("fixtures/journal_replay/a71616b0bdfd6b4e.rs")),
        None,
    );
}

// replay-pron-mixed-pinyin: MATCH.
#[test]
fn replay_pron_mixed_pinyin() {
    run_j(
        "journal_replay::replay_pron_mixed_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-mixed-pinyin"),
        raw!(include!("fixtures/journal_replay/1a4116e5e5bd92be.rs")),
        None,
    );
}

// replay-pron-train-dup-both-pinyin: MATCH.
#[test]
fn replay_pron_train_dup_both_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_dup_both_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-dup-both-pinyin"),
        raw!(include!("fixtures/journal_replay/ec587fcc191056e1.rs")),
        None,
    );
}

// replay-pron-train-dup-equal-pinyin: MATCH.
#[test]
fn replay_pron_train_dup_equal_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_dup_equal_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-dup-equal-pinyin"),
        raw!(include!("fixtures/journal_replay/aa39880c9d58ff34.rs")),
        None,
    );
}

// replay-pron-train-dup-raised-first-pinyin: MATCH.
#[test]
fn replay_pron_train_dup_raised_first_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_dup_raised_first_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-dup-raised-first-pinyin"),
        raw!(include!("fixtures/journal_replay/a456ed92af05bdf3.rs")),
        None,
    );
}

// replay-pron-train-dup-raised-last-pinyin: MATCH.
#[test]
fn replay_pron_train_dup_raised_last_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_dup_raised_last_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-dup-raised-last-pinyin"),
        raw!(include!("fixtures/journal_replay/f784ff4d937e3de8.rs")),
        None,
    );
}

// replay-pron-train-lowered-pinyin: MATCH.
#[test]
fn replay_pron_train_lowered_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_lowered_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-lowered-pinyin"),
        raw!(include!("fixtures/journal_replay/796f64ae11612433.rs")),
        None,
    );
}

// replay-pron-train-other-first-pinyin: MATCH.
#[test]
fn replay_pron_train_other_first_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_other_first_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-other-first-pinyin"),
        raw!(include!("fixtures/journal_replay/69ee5a7d12552965.rs")),
        None,
    );
}

// replay-pron-train-other-reading-pinyin: MATCH.
#[test]
fn replay_pron_train_other_reading_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_other_reading_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-other-reading-pinyin"),
        raw!(include!("fixtures/journal_replay/308bb855cbb98aa4.rs")),
        None,
    );
}

// replay-pron-train-raised-pinyin: MATCH.
#[test]
fn replay_pron_train_raised_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_raised_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-raised-pinyin"),
        raw!(include!("fixtures/journal_replay/ec16884469f2599a.rs")),
        None,
    );
}

// replay-pron-train-unigram-only-pinyin: MATCH.
#[test]
fn replay_pron_train_unigram_only_pinyin() {
    run_j(
        "journal_replay::replay_pron_train_unigram_only_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-unigram-only-pinyin"),
        raw!(include!("fixtures/journal_replay/38b74bcb1bf81697.rs")),
        None,
    );
}

// replay-pron-unigram-combined-pinyin: MATCH.
#[test]
fn replay_pron_unigram_combined_pinyin() {
    run_j(
        "journal_replay::replay_pron_unigram_combined_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-unigram-combined-pinyin"),
        raw!(include!("fixtures/journal_replay/d29b37152e9bb004.rs")),
        None,
    );
}

// replay-remove-absent-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_remove_absent_pinyin() {
    run_j(
        "journal_replay::replay_remove_absent_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-remove-absent-pinyin"),
        raw!(include!("fixtures/journal_replay/d6b91d5e2d212be4.rs")),
        Some(&["absent", "ctx", "fresh", "one"]),
    );
}

// replay-remove-trailing-old-pinyin: MATCH.
#[test]
fn replay_remove_trailing_old_pinyin() {
    run_j(
        "journal_replay::replay_remove_trailing_old_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-remove-trailing-old-pinyin"),
        raw!(include!("fixtures/journal_replay/3f801cbcaa5c7f1a.rs")),
        None,
    );
}

// replay-seed-add-remove-unigrams-reopen-pinyin: MATCH.
#[test]
fn replay_seed_add_remove_unigrams_reopen_pinyin() {
    run_j(
        "journal_replay::replay_seed_add_remove_unigrams_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-seed-add-remove-unigrams-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/46ad80e90b7b925e.rs")),
        None,
    );
}

// replay-seed-header-total-reopen-pinyin: MATCH.
#[test]
fn replay_seed_header_total_reopen_pinyin() {
    run_j(
        "journal_replay::replay_seed_header_total_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-seed-header-total-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/2a3ace3fcb2ebd47.rs")),
        None,
    );
}

// replay-seed-mixed-repeated-unigrams-reopen-pinyin: MATCH.
#[test]
fn replay_seed_mixed_repeated_unigrams_reopen_pinyin() {
    run_j(
        "journal_replay::replay_seed_mixed_repeated_unigrams_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-seed-mixed-repeated-unigrams-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/5ca7813627b2197b.rs")),
        None,
    );
}

// replay-seed-restored-unigram-reopen-pinyin: MATCH.
#[test]
fn replay_seed_restored_unigram_reopen_pinyin() {
    run_j(
        "journal_replay::replay_seed_restored_unigram_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-seed-restored-unigram-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/2a3ace3fcb2ebd47.rs")),
        None,
    );
}

// replay-seed-stale-storage-unigram-reopen-pinyin: MATCH.
#[test]
fn replay_seed_stale_storage_unigram_reopen_pinyin() {
    run_j(
        "journal_replay::replay_seed_stale_storage_unigram_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-seed-stale-storage-unigram-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/aba8e4390a65ba26.rs")),
        None,
    );
}

// replay-short-add-five-bytes-pinyin: the pin's abort, reproduced.
#[test]
fn replay_short_add_five_bytes_pinyin() {
    run_aborting(
        "journal_replay::replay_short_add_five_bytes_pinyin",
        Expect::aborts(b"").with_files(&[("gb_char.dbin", UserFile::Chunk(111, "eddf2c6a697228fbd2fcdeb52ff62dc4b2e1e67fb7246931819fe2a0eb063b61")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:1|"))]),
        |k| {
            b::case(k, "replay-short-add-five-bytes-pinyin");
        },
    );
}

// replay-stop-before-header-token-modify-pinyin: MATCH.
#[test]
fn replay_stop_before_header_token_modify_pinyin() {
    run_j(
        "journal_replay::replay_stop_before_header_token_modify_pinyin",
        Expect::RESULT,
        |k| b::case(k, "replay-stop-before-header-token-modify-pinyin"),
        raw!(include!("fixtures/journal_replay/ccb0d2121f83cab2.rs")),
        None,
    );
}

// replay-stop-before-header-token-remove-pinyin: MATCH.
#[test]
fn replay_stop_before_header_token_remove_pinyin() {
    run_j(
        "journal_replay::replay_stop_before_header_token_remove_pinyin",
        Expect::RESULT,
        |k| b::case(k, "replay-stop-before-header-token-remove-pinyin"),
        raw!(include!("fixtures/journal_replay/22f68a3e9f1d3b85.rs")),
        None,
    );
}

// replay-touched-duplicate-invalid-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_touched_duplicate_invalid_pinyin() {
    run_j(
        "journal_replay::replay_touched_duplicate_invalid_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-touched-duplicate-invalid-pinyin"),
        raw!(include!("fixtures/journal_replay/67e37978e3b3d97f.rs")),
        Some(&["ctx", "fresh", "last", "one"]),
    );
}

// replay-touched-many-invalid-pinyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_touched_many_invalid_pinyin() {
    run_j(
        "journal_replay::replay_touched_many_invalid_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-touched-many-invalid-pinyin"),
        raw!(include!("fixtures/journal_replay/67e37978e3b3d97f.rs")),
        Some(&["ctx", "fresh", "last", "one"]),
    );
}

// replay-touched-order-duplicates-reopen-pinyin: MATCH.
#[test]
fn replay_touched_order_duplicates_reopen_pinyin() {
    run_j(
        "journal_replay::replay_touched_order_duplicates_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-touched-order-duplicates-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/338db60d6ae1c49f.rs")),
        None,
    );
}

// replay-touched-sparse-add-remove-relocate-reopen-pinyin: MATCH.
#[test]
fn replay_touched_sparse_add_remove_relocate_reopen_pinyin() {
    run_j(
        "journal_replay::replay_touched_sparse_add_remove_relocate_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-touched-sparse-add-remove-relocate-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/029f9224725e43c4.rs")),
        None,
    );
}

// replay-unigram-add-appended-item-reopen-pinyin: MATCH.
#[test]
fn replay_unigram_add_appended_item_reopen_pinyin() {
    run_j(
        "journal_replay::replay_unigram_add_appended_item_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-add-appended-item-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/31795ee26c6eeccf.rs")),
        None,
    );
}

// replay-unigram-add-changed-reading-reopen-pinyin: MATCH.
#[test]
fn replay_unigram_add_changed_reading_reopen_pinyin() {
    run_j(
        "journal_replay::replay_unigram_add_changed_reading_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-add-changed-reading-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/47ccb9b774e8ed83.rs")),
        None,
    );
}

// replay-unigram-add-live-item-control-pinyin: MATCH.
#[test]
fn replay_unigram_add_live_item_control_pinyin() {
    run_j(
        "journal_replay::replay_unigram_add_live_item_control_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-add-live-item-control-pinyin"),
        raw!(include!("fixtures/journal_replay/06add0f4fc955bb0.rs")),
        None,
    );
}

// replay-unigram-add-live-item-pinyin: MATCH.
#[test]
fn replay_unigram_add_live_item_pinyin() {
    run_j(
        "journal_replay::replay_unigram_add_live_item_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-add-live-item-pinyin"),
        raw!(include!("fixtures/journal_replay/fc4cb58c4255bb46.rs")),
        None,
    );
}

// replay-unigram-add-live-item-reopen-pinyin: MATCH.
#[test]
fn replay_unigram_add_live_item_reopen_pinyin() {
    run_j(
        "journal_replay::replay_unigram_add_live_item_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-add-live-item-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/b64d624d14c12c34.rs")),
        None,
    );
}

// replay-unigram-add-overridden-item-reopen-pinyin: MATCH.
#[test]
fn replay_unigram_add_overridden_item_reopen_pinyin() {
    run_j(
        "journal_replay::replay_unigram_add_overridden_item_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-add-overridden-item-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/e81cba3a3f0afa12.rs")),
        None,
    );
}

// replay-unigram-refused-changed-reading-reopen-pinyin: MATCH.
#[test]
fn replay_unigram_refused_changed_reading_reopen_pinyin() {
    run_j(
        "journal_replay::replay_unigram_refused_changed_reading_reopen_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-refused-changed-reading-reopen-pinyin"),
        raw!(include!("fixtures/journal_replay/7dae5605be1f4fa5.rs")),
        None,
    );
}

// unigram-lifecycle-import-system-pinyin: MATCH.
#[test]
fn unigram_lifecycle_import_system_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_import_system_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-import-system-pinyin"),
        raw!(include!("fixtures/journal_replay/450f349198a01fc9.rs")),
        None,
    );
}

// unigram-lifecycle-mask-pinyin: MATCH.
#[test]
fn unigram_lifecycle_mask_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_mask_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-mask-pinyin"),
        raw!(include!("fixtures/journal_replay/e70e06cb89113ea8.rs")),
        None,
    );
}

// unigram-lifecycle-missing-pinyin: MATCH.
#[test]
fn unigram_lifecycle_missing_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_missing_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-missing-pinyin"),
        raw!(include!("fixtures/journal_replay/1dd8150dee7b9c2b.rs")),
        None,
    );
}

// unigram-lifecycle-overflow-pinyin: MATCH.
#[test]
fn unigram_lifecycle_overflow_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_overflow_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-overflow-pinyin"),
        raw!(include!("fixtures/journal_replay/b7ef5fc4f4f98b04.rs")),
        None,
    );
}

// unigram-lifecycle-reload-pinyin: MATCH.
#[test]
fn unigram_lifecycle_reload_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_reload_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-reload-pinyin"),
        raw!(include!("fixtures/journal_replay/2f69c5bcbe8d5263.rs")),
        None,
    );
}

// unigram-lifecycle-removed-pinyin: MATCH.
#[test]
fn unigram_lifecycle_removed_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_removed_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-removed-pinyin"),
        raw!(include!("fixtures/journal_replay/dc7ba47c1843b627.rs")),
        None,
    );
}

// unigram-lifecycle-repeat-pinyin: MATCH.
#[test]
fn unigram_lifecycle_repeat_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_repeat_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-repeat-pinyin"),
        raw!(include!("fixtures/journal_replay/6ee887f6be90a023.rs")),
        None,
    );
}

// unigram-lifecycle-replacement-pinyin: MATCH.
#[test]
fn unigram_lifecycle_replacement_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_replacement_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-replacement-pinyin"),
        raw!(include!("fixtures/journal_replay/fb4274d46f2f7897.rs")),
        None,
    );
}

// unigram-lifecycle-timing-pinyin: MATCH.
#[test]
fn unigram_lifecycle_timing_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_timing_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-timing-pinyin"),
        raw!(include!("fixtures/journal_replay/7b4ebf2298c2ce83.rs")),
        None,
    );
}

// unigram-lifecycle-training-pinyin: MATCH.
#[test]
fn unigram_lifecycle_training_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_training_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-training-pinyin"),
        raw!(include!("fixtures/journal_replay/d8c1e7c70181520c.rs")),
        None,
    );
}

// unigram-lifecycle-wrap-pinyin: MATCH.
#[test]
fn unigram_lifecycle_wrap_pinyin() {
    run_j(
        "journal_replay::unigram_lifecycle_wrap_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-wrap-pinyin"),
        raw!(include!("fixtures/journal_replay/f6fa013619e359a1.rs")),
        None,
    );
}

// unigram-null-user-save-pinyin: MATCH.
#[test]
fn unigram_null_user_save_pinyin() {
    run_j(
        "journal_replay::unigram_null_user_save_pinyin",
        Expect::stderr(b"open  failed.write  failed.\n"),
        |k| b::case(k, "unigram-null-user-save-pinyin"),
        raw!(include!("fixtures/journal_replay/ccc2e2595ce52868.rs")),
        None,
    );
}

// unigram-refused-reload-pinyin: the pin's abort, reproduced.
#[test]
fn unigram_refused_reload_pinyin() {
    run_aborting(
        "journal_replay::unigram_refused_reload_pinyin",
        Expect::aborts(b""),
        |k| {
            b::case(k, "unigram-refused-reload-pinyin");
        },
    );
}
