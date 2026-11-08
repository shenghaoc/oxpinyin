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
