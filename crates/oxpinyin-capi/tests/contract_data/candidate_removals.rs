//! Candidate removals and refusals of the wrong candidate types and of user phrases the pin cannot remove (#1175, #525). Generated from the frozen pin observations.

use crate::bodies::candidate_removals as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-choose-candidate-predicted-prefix-row: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_choose_candidate_predicted_prefix_row() {
    run_aborting(
        "candidate_removals::abort_choose_candidate_predicted_prefix_row",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-choose-candidate-predicted-prefix-row");
        },
    );
}

// abort-choose-candidate-without-sentence-offset-1: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_choose_candidate_without_sentence_offset_1() {
    run_aborting(
        "candidate_removals::abort_choose_candidate_without_sentence_offset_1",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-choose-candidate-without-sentence-offset-1");
        },
    );
}

// abort-choose-predicted-candidate-normal-row: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_choose_predicted_candidate_normal_row() {
    run_aborting(
        "candidate_removals::abort_choose_predicted_candidate_normal_row",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-choose-predicted-candidate-normal-row");
        },
    );
}

// abort-choose-predicted-candidate-sentence-row: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_choose_predicted_candidate_sentence_row() {
    run_aborting(
        "candidate_removals::abort_choose_predicted_candidate_sentence_row",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-choose-predicted-candidate-sentence-row");
        },
    );
}

// abort-get-candidate-nbest-index-normal-row: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_candidate_nbest_index_normal_row() {
    run_aborting(
        "candidate_removals::abort_get_candidate_nbest_index_normal_row",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-candidate-nbest-index-normal-row");
        },
    );
}

// abort-remove-user-candidate-missing-phrase-table: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_missing_phrase_table() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_missing_phrase_table",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-missing-phrase-table");
        },
    );
}

// abort-remove-user-candidate-multi-pron: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_multi_pron() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_multi_pron",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-multi-pron");
        },
    );
}

// abort-remove-user-candidate-non-token-bigram-key: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_non_token_bigram_key() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_non_token_bigram_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-non-token-bigram-key");
        },
    );
}

// abort-remove-user-candidate-normal-system-token: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_normal_system_token() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_normal_system_token",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-normal-system-token");
        },
    );
}

// abort-remove-user-candidate-one-projection: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_one_projection() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_one_projection",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-one-projection");
        },
    );
}

// abort-remove-user-candidate-residual-bigram-gram: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_residual_bigram_gram() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_residual_bigram_gram",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-residual-bigram-gram");
        },
    );
}

// abort-remove-user-candidate-sentence-row: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_sentence_row() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_sentence_row",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-sentence-row");
        },
    );
}

// abort-remove-user-candidate-short-bigram-value: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_short_bigram_value() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_short_bigram_value",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-short-bigram-value");
        },
    );
}

// abort-remove-user-candidate-twice: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_remove_user_candidate_twice() {
    run_aborting(
        "candidate_removals::abort_remove_user_candidate_twice",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-remove-user-candidate-twice");
        },
    );
}

// candidate-type-neighbours: MATCH.
#[test]
fn candidate_type_neighbours() {
    run_j(
        "candidate_removals::candidate_type_neighbours",
        Expect::RESULT,
        |k| b::case(k, "candidate-type-neighbours"),
        raw!(include!("fixtures/candidate_removals/14b5e15a74c9cbfd.rs")),
        None,
    );
}

// choose-candidate-without-sentence-offset-0: MATCH.
#[test]
fn choose_candidate_without_sentence_offset_0() {
    run_j(
        "candidate_removals::choose_candidate_without_sentence_offset_0",
        Expect::RESULT,
        |k| b::case(k, "choose-candidate-without-sentence-offset-0"),
        raw!(M(&[("logs", L(&[])), ("ret", I(1))])),
        None,
    );
}

// user-index-suggestion-without-prefix-marker: MATCH.
#[test]
fn user_index_suggestion_without_prefix_marker() {
    run_j(
        "candidate_removals::user_index_suggestion_without_prefix_marker",
        Expect::RESULT,
        |k| b::case(k, "user-index-suggestion-without-prefix-marker"),
        raw!(M(&[("logs", L(&[])), ("user_candidates", I(0))])),
        None,
    );
}
