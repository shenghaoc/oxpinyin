//! Toned initial-only keys and the zhuyin cursor, key and offset refusals (#525). Generated from the frozen pin observations.

use crate::bodies::toned_keys_zhuyin_refusals as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-get-right-pinyin-offset-empty-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_right_pinyin_offset_empty_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_get_right_pinyin_offset_empty_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-right-pinyin-offset-empty-matrix");
        },
    );
}

// abort-guess-candidates-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_candidates_past_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_guess_candidates_past_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-candidates-past-matrix");
        },
    );
}

// abort-guess-candidates-toned-initial: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_candidates_toned_initial() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_guess_candidates_toned_initial",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-candidates-toned-initial");
        },
    );
}

// abort-guess-candidates-toned-initial-before-offset: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_candidates_toned_initial_before_offset() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_guess_candidates_toned_initial_before_offset",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-candidates-toned-initial-before-offset");
        },
    );
}

// abort-guess-candidates-toned-initial-no-prepends: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_candidates_toned_initial_no_prepends() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_guess_candidates_toned_initial_no_prepends",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-candidates-toned-initial-no-prepends");
        },
    );
}

// abort-guess-sentence-toned-initial: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_sentence_toned_initial() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_guess_sentence_toned_initial",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-sentence-toned-initial");
        },
    );
}

// abort-guess-sentence-toned-initial-after-key: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_sentence_toned_initial_after_key() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_guess_sentence_toned_initial_after_key",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-sentence-toned-initial-after-key");
        },
    );
}

// abort-is-incomplete-toned-initial: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_is_incomplete_toned_initial() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_is_incomplete_toned_initial",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-is-incomplete-toned-initial");
        },
    );
}

// guess-candidates-toned-initial-before-offset-no-prepends: MATCH.
#[test]
fn guess_candidates_toned_initial_before_offset_no_prepends() {
    run_j(
        "toned_keys_zhuyin_refusals::guess_candidates_toned_initial_before_offset_no_prepends",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "guess-candidates-toned-initial-before-offset-no-prepends",
            )
        },
        raw!(M(&[("count", I(125)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// guess-candidates-toned-initial-past-key-no-prepends: MATCH.
#[test]
fn guess_candidates_toned_initial_past_key_no_prepends() {
    run_j(
        "toned_keys_zhuyin_refusals::guess_candidates_toned_initial_past_key_no_prepends",
        Expect::RESULT,
        |k| b::case(k, "guess-candidates-toned-initial-past-key-no-prepends"),
        raw!(M(&[("count", I(0)), ("logs", L(&[])), ("ret", B(true))])),
        None,
    );
}

// toned-initial-neighbours: MATCH.
#[test]
fn toned_initial_neighbours() {
    run_j(
        "toned_keys_zhuyin_refusals::toned_initial_neighbours",
        Expect::RESULT,
        |k| b::case(k, "toned-initial-neighbours"),
        raw!(include!(
            "fixtures/toned_keys_zhuyin_refusals/4a96d324c3b58cf4.rs"
        )),
        None,
    );
}
