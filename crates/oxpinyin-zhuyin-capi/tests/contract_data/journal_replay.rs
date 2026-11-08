//! Journal replay, removed items and chunk frames (#1175, #1623). Generated from the frozen pin observations.

use crate::bodies::journal_replay as b;
use crate::support::{Expect, UserFile, run_aborting, run_j};

// chunk-frame-journal-exact-zhuyin: MATCH.
#[test]
fn chunk_frame_journal_exact_zhuyin() {
    run_j(
        "journal_replay::chunk_frame_journal_exact_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "chunk-frame-journal-exact-zhuyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("logs", L(&[])),
            ("unigram", L(&[B(true), I(42)]))
        ])),
        None,
    );
}

// chunk-frame-journal-trailing-zhuyin: MATCH.
#[test]
fn chunk_frame_journal_trailing_zhuyin() {
    run_j(
        "journal_replay::chunk_frame_journal_trailing_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "chunk-frame-journal-trailing-zhuyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("logs", L(&[])),
            ("unigram", L(&[B(true), I(3)]))
        ])),
        None,
    );
}

// removed-candidate-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn removed_candidate_zhuyin() {
    run_j(
        "journal_replay::removed_candidate_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "removed-candidate-zhuyin"),
        raw!(M(&[
            ("exact-candidates-ret", B(true)),
            ("exact-guessed", B(true)),
            ("removed", L(&[B(false), I(0), Nil]))
        ])),
        Some(&["exact-candidates-ret", "exact-guessed", "removed"]),
    );
}

// removed-extension-candidates-zhuyin: MATCH.
#[test]
fn removed_extension_candidates_zhuyin() {
    run_j(
        "journal_replay::removed_extension_candidates_zhuyin",
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "removed-extension-candidates-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/128aa68a47e03eed.rs"
        )),
        None,
    );
}

// replay-cross-item-unigram-unrecorded-zhuyin: MATCH.
#[test]
fn replay_cross_item_unigram_unrecorded_zhuyin() {
    run_j(
        "journal_replay::replay_cross_item_unigram_unrecorded_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-cross-item-unigram-unrecorded-zhuyin"),
        raw!(include!("fixtures/journal_replay/0f171d3d534b1df4.rs")),
        None,
    );
}

// replay-matched-remove-zhuyin: MATCH.
#[test]
fn replay_matched_remove_zhuyin() {
    run_j(
        "journal_replay::replay_matched_remove_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-matched-remove-zhuyin"),
        raw!(include!("fixtures/journal_replay/47b23bb9d3529e02.rs")),
        None,
    );
}

// replay-mismatch-then-valid-zhuyin: MATCH.
#[test]
fn replay_mismatch_then_valid_zhuyin() {
    run_j(
        "journal_replay::replay_mismatch_then_valid_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-mismatch-then-valid-zhuyin"),
        raw!(include!("fixtures/journal_replay/7271c2de66dcee9a.rs")),
        None,
    );
}

// replay-modify-absent-default-grow-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_modify_absent_default_grow_zhuyin() {
    run_j(
        "journal_replay::replay_modify_absent_default_grow_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-absent-default-grow-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/067ee38fbe26671d.rs"
        )),
        Some(&["absent", "ctx", "fresh", "one"]),
    );
}

// replay-modify-absent-zhuyin: MATCH.
#[test]
fn replay_modify_absent_zhuyin() {
    run_j(
        "journal_replay::replay_modify_absent_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-absent-zhuyin"),
        raw!(include!("fixtures/journal_replay/ea8a2f05583a3f32.rs")),
        None,
    );
}

// replay-modify-header-overlong-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_modify_header_overlong_zhuyin() {
    run_j(
        "journal_replay::replay_modify_header_overlong_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-header-overlong-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/67e37978e3b3d97f.rs"
        )),
        Some(&["ctx", "fresh", "last", "one"]),
    );
}

// replay-modify-trailing-new-grow-zhuyin: MATCH.
#[test]
fn replay_modify_trailing_new_grow_zhuyin() {
    run_j(
        "journal_replay::replay_modify_trailing_new_grow_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-modify-trailing-new-grow-zhuyin"),
        raw!(include!("fixtures/journal_replay/f92f7be8d7987bdb.rs")),
        None,
    );
}

// replay-over-read-tail-zhuyin: the pin is not run; oxpinyin returns its declared fields.
#[test]
fn replay_over_read_tail_zhuyin() {
    run_j(
        "journal_replay::replay_over_read_tail_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "replay-over-read-tail-zhuyin"),
        raw!(M(&[
            ("ctx", B(true)),
            ("export", S("NO EXPORT API")),
            (
                "one",
                L(&[L(&[B(true), I(1), S("锕")]), L(&[B(true), I(3)])])
            )
        ])),
        Some(&["ctx", "export", "one"]),
    );
}

// replay-pron-import-dup-both-zhuyin: MATCH.
#[test]
fn replay_pron_import_dup_both_zhuyin() {
    run_j(
        "journal_replay::replay_pron_import_dup_both_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-import-dup-both-zhuyin"),
        raw!(include!("fixtures/journal_replay/6a165a0754eac92e.rs")),
        None,
    );
}

// replay-pron-mask-dup-both-zhuyin: MATCH.
#[test]
fn replay_pron_mask_dup_both_zhuyin() {
    run_j(
        "journal_replay::replay_pron_mask_dup_both_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-mask-dup-both-zhuyin"),
        raw!(include!("fixtures/journal_replay/6c202f4551218b89.rs")),
        None,
    );
}

// replay-pron-mixed-zhuyin: MATCH.
#[test]
fn replay_pron_mixed_zhuyin() {
    run_j(
        "journal_replay::replay_pron_mixed_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-mixed-zhuyin"),
        raw!(include!("fixtures/journal_replay/84fc063ef482d6e0.rs")),
        None,
    );
}

// replay-pron-train-dup-both-zhuyin: MATCH.
#[test]
fn replay_pron_train_dup_both_zhuyin() {
    run_j(
        "journal_replay::replay_pron_train_dup_both_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-pron-train-dup-both-zhuyin"),
        raw!(include!("fixtures/journal_replay/e5023812aace4b45.rs")),
        None,
    );
}

// replay-remove-absent-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_remove_absent_zhuyin() {
    run_j(
        "journal_replay::replay_remove_absent_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-remove-absent-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/d6b91d5e2d212be4.rs"
        )),
        Some(&["absent", "ctx", "fresh", "one"]),
    );
}

// replay-seed-mixed-repeated-unigrams-reopen-zhuyin: MATCH.
#[test]
fn replay_seed_mixed_repeated_unigrams_reopen_zhuyin() {
    run_j(
        "journal_replay::replay_seed_mixed_repeated_unigrams_reopen_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-seed-mixed-repeated-unigrams-reopen-zhuyin"),
        raw!(include!("fixtures/journal_replay/f5fc9b2c7402c8fe.rs")),
        None,
    );
}

// replay-short-add-five-bytes-zhuyin: the pin's abort, reproduced.
#[test]
fn replay_short_add_five_bytes_zhuyin() {
    run_aborting(
        "journal_replay::replay_short_add_five_bytes_zhuyin",
        Expect::aborts(b"").with_files(&[("gb_char.dbin", UserFile::Chunk(111, "eddf2c6a697228fbd2fcdeb52ff62dc4b2e1e67fb7246931819fe2a0eb063b61")), ("user.conf", UserFile::Conf("binary format version:7|model data version:14|database format:Tkrzw|open counter:0|"))]),
        |k| {
            b::case(k, "replay-short-add-five-bytes-zhuyin");
        },
    );
}

// replay-stop-before-header-token-modify-zhuyin: MATCH.
#[test]
fn replay_stop_before_header_token_modify_zhuyin() {
    run_j(
        "journal_replay::replay_stop_before_header_token_modify_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "replay-stop-before-header-token-modify-zhuyin"),
        raw!(include!("fixtures/journal_replay/d86ee9f60b39c08b.rs")),
        None,
    );
}

// replay-touched-duplicate-invalid-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_touched_duplicate_invalid_zhuyin() {
    run_j(
        "journal_replay::replay_touched_duplicate_invalid_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-touched-duplicate-invalid-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/67e37978e3b3d97f.rs"
        )),
        Some(&["ctx", "fresh", "last", "one"]),
    );
}

// replay-touched-many-invalid-zhuyin: the pin crashes (SIGSEGV); oxpinyin returns its declared fields.
#[test]
fn replay_touched_many_invalid_zhuyin() {
    run_j(
        "journal_replay::replay_touched_many_invalid_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-touched-many-invalid-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/67e37978e3b3d97f.rs"
        )),
        Some(&["ctx", "fresh", "last", "one"]),
    );
}

// replay-touched-order-duplicates-reopen-zhuyin: MATCH.
#[test]
fn replay_touched_order_duplicates_reopen_zhuyin() {
    run_j(
        "journal_replay::replay_touched_order_duplicates_reopen_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-touched-order-duplicates-reopen-zhuyin"),
        raw!(include!("fixtures/journal_replay/ce34f516ec459dc4.rs")),
        None,
    );
}

// replay-unigram-add-live-item-reopen-zhuyin: MATCH.
#[test]
fn replay_unigram_add_live_item_reopen_zhuyin() {
    run_j(
        "journal_replay::replay_unigram_add_live_item_reopen_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "replay-unigram-add-live-item-reopen-zhuyin"),
        raw!(include!("fixtures/journal_replay/087daddc349c894c.rs")),
        None,
    );
}

// unigram-lifecycle-missing-zhuyin: MATCH.
#[test]
fn unigram_lifecycle_missing_zhuyin() {
    run_j(
        "journal_replay::unigram_lifecycle_missing_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-missing-zhuyin"),
        raw!(include!("fixtures/journal_replay/8864a5510dd01cb5.rs")),
        None,
    );
}

// unigram-lifecycle-overflow-zhuyin: MATCH.
#[test]
fn unigram_lifecycle_overflow_zhuyin() {
    run_j(
        "journal_replay::unigram_lifecycle_overflow_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-overflow-zhuyin"),
        raw!(include!("fixtures/journal_replay/db965c14e5f05e99.rs")),
        None,
    );
}

// unigram-lifecycle-repeat-zhuyin: MATCH.
#[test]
fn unigram_lifecycle_repeat_zhuyin() {
    run_j(
        "journal_replay::unigram_lifecycle_repeat_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "unigram-lifecycle-repeat-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/6ee887f6be90a023.rs"
        )),
        None,
    );
}

// unigram-null-user-save-zhuyin: MATCH.
#[test]
fn unigram_null_user_save_zhuyin() {
    run_j(
        "journal_replay::unigram_null_user_save_zhuyin",
        Expect::stderr(b"open  failed."),
        |k| b::case(k, "unigram-null-user-save-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/journal_replay/ccc2e2595ce52868.rs"
        )),
        None,
    );
}

// unigram-refused-reload-zhuyin: the pin's abort, reproduced.
#[test]
fn unigram_refused_reload_zhuyin() {
    run_aborting(
        "journal_replay::unigram_refused_reload_zhuyin",
        Expect::aborts(b""),
        |k| {
            b::case(k, "unigram-refused-reload-zhuyin");
        },
    );
}
