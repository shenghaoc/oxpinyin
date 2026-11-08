//! Toned initial-only keys and the zhuyin cursor, key and offset refusals (#525). Generated from the frozen pin observations.

use crate::bodies::toned_keys_zhuyin_refusals as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-zhuyin-get-character-offset-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_get_character_offset_past_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_get_character_offset_past_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-get-character-offset-past-matrix");
        },
    );
}

// abort-zhuyin-get-left-zhuyin-offset-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_get_left_zhuyin_offset_past_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_get_left_zhuyin_offset_past_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-get-left-zhuyin-offset-past-matrix");
        },
    );
}

// abort-zhuyin-get-right-zhuyin-offset-empty-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_get_right_zhuyin_offset_empty_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_get_right_zhuyin_offset_empty_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-get-right-zhuyin-offset-empty-matrix");
        },
    );
}

// abort-zhuyin-get-right-zhuyin-offset-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_get_right_zhuyin_offset_past_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_get_right_zhuyin_offset_past_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-get-right-zhuyin-offset-past-matrix");
        },
    );
}

// abort-zhuyin-get-zhuyin-key-empty-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_get_zhuyin_key_empty_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_get_zhuyin_key_empty_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-get-zhuyin-key-empty-matrix");
        },
    );
}

// abort-zhuyin-get-zhuyin-key-rest-empty-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_get_zhuyin_key_rest_empty_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_get_zhuyin_key_rest_empty_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-get-zhuyin-key-rest-empty-matrix");
        },
    );
}

// abort-zhuyin-guess-candidates-after-cursor-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_guess_candidates_after_cursor_past_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_guess_candidates_after_cursor_past_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-guess-candidates-after-cursor-past-matrix");
        },
    );
}

// abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_guess_candidates_after_cursor_reserved_slot_plus_one() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_guess_candidates_after_cursor_reserved_slot_plus_one",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "abort-zhuyin-guess-candidates-after-cursor-reserved-slot-plus-one",
            );
        },
    );
}

// abort-zhuyin-guess-candidates-before-cursor-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_guess_candidates_before_cursor_past_matrix() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_guess_candidates_before_cursor_past_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-guess-candidates-before-cursor-past-matrix");
        },
    );
}

// abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_guess_candidates_before_cursor_reserved_slot_plus_one() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_guess_candidates_before_cursor_reserved_slot_plus_one",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "abort-zhuyin-guess-candidates-before-cursor-reserved-slot-plus-one",
            );
        },
    );
}

// abort-zhuyin-guess-candidates-total-zero: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_guess_candidates_total_zero() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_guess_candidates_total_zero",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-guess-candidates-total-zero");
        },
    );
}

// abort-zhuyin-parse-full-pinyin-apostrophe: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_zhuyin_parse_full_pinyin_apostrophe() {
    run_aborting(
        "toned_keys_zhuyin_refusals::abort_zhuyin_parse_full_pinyin_apostrophe",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-zhuyin-parse-full-pinyin-apostrophe");
        },
    );
}

// zhuyin-cursor-neighbours: MATCH.
#[test]
fn zhuyin_cursor_neighbours() {
    run_j(
        "toned_keys_zhuyin_refusals::zhuyin_cursor_neighbours",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-cursor-neighbours"),
        raw!(include!(
            "fixtures/toned_keys_zhuyin_refusals/dd208041e5a152c8.rs"
        )),
        None,
    );
}

// zhuyin-get-zhuyin-key-rest-size-max-empty-matrix: MATCH.
#[test]
fn zhuyin_get_zhuyin_key_rest_size_max_empty_matrix() {
    run_j(
        "toned_keys_zhuyin_refusals::zhuyin_get_zhuyin_key_rest_size_max_empty_matrix",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-get-zhuyin-key-rest-size-max-empty-matrix"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// zhuyin-get-zhuyin-key-size-max-empty-matrix: MATCH.
#[test]
fn zhuyin_get_zhuyin_key_size_max_empty_matrix() {
    run_j(
        "toned_keys_zhuyin_refusals::zhuyin_get_zhuyin_key_size_max_empty_matrix",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-get-zhuyin-key-size-max-empty-matrix"),
        raw!(M(&[("logs", L(&[])), ("ret", B(false))])),
        None,
    );
}

// zhuyin-scheme-and-library-neighbours: MATCH.
#[test]
fn zhuyin_scheme_and_library_neighbours() {
    run_j(
        "toned_keys_zhuyin_refusals::zhuyin_scheme_and_library_neighbours",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-scheme-and-library-neighbours"),
        raw!(include!(
            "fixtures/toned_keys_zhuyin_refusals/fb80b5cc2c4090f2.rs"
        )),
        None,
    );
}

// zhuyin-total-zero-ranks-nothing-after-cursor: MATCH.
#[test]
fn zhuyin_total_zero_ranks_nothing_after_cursor() {
    run_j(
        "toned_keys_zhuyin_refusals::zhuyin_total_zero_ranks_nothing_after_cursor",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-total-zero-ranks-nothing-after-cursor"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true)), ("rows", I(0))])),
        None,
    );
}

// zhuyin-total-zero-ranks-nothing-before-cursor: MATCH.
#[test]
fn zhuyin_total_zero_ranks_nothing_before_cursor() {
    run_j(
        "toned_keys_zhuyin_refusals::zhuyin_total_zero_ranks_nothing_before_cursor",
        Expect::RESULT,
        |k| b::case(k, "zhuyin-total-zero-ranks-nothing-before-cursor"),
        raw!(M(&[("logs", L(&[])), ("ret", B(true)), ("rows", I(0))])),
        None,
    );
}
