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
        // g_build_filename joins with \ on Windows, and the UCRT's text-mode stderr writes \n as \r\n.
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "removed-candidate-pinyin"),
        raw!(M(&[
            ("exact-candidates-ret", B(true)),
            ("exact-guessed", B(true)),
            ("removed", L(&[B(false), I(0), Nil]))
        ])),
        Some(&["exact-candidates-ret", "exact-guessed", "removed"]),
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
// replay-short-add-five-bytes-pinyin: the pin's abort, reproduced.
#[test]
fn replay_short_add_five_bytes_pinyin() {
    // user.conf is written in text mode: CRLF on Windows (external-io-boundary.md, "Native text-mode line endings").
    #[cfg(windows)]
    const USER_CONF: &str = "binary format version:7\r|model data version:14\r|database format:Tkrzw\r|open counter:1\r|";
    #[cfg(not(windows))]
    const USER_CONF: &str =
        "binary format version:7|model data version:14|database format:Tkrzw|open counter:1|";
    run_aborting(
        "journal_replay::replay_short_add_five_bytes_pinyin",
        Expect::aborts(b"").with_files(&[
            (
                "gb_char.dbin",
                UserFile::Chunk(
                    111,
                    "eddf2c6a697228fbd2fcdeb52ff62dc4b2e1e67fb7246931819fe2a0eb063b61",
                ),
            ),
            ("user.conf", UserFile::Conf(USER_CONF)),
        ]),
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
