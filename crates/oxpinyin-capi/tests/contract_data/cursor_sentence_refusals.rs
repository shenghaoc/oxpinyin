//! Cursor, offset, sentence-index and auxiliary-text refusals (#525). Generated from the frozen pin observations.

use crate::bodies::cursor_sentence_refusals as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-chewing-auxiliary-text-leading-apostrophe: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_chewing_auxiliary_text_leading_apostrophe() {
    run_aborting(
        "cursor_sentence_refusals::abort_chewing_auxiliary_text_leading_apostrophe",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-chewing-auxiliary-text-leading-apostrophe");
        },
    );
}

// abort-double-auxiliary-text-cut-three-bytes-in: the pin's abort, reproduced.
#[test]
fn abort_double_auxiliary_text_cut_three_bytes_in() {
    run_aborting(
        "cursor_sentence_refusals::abort_double_auxiliary_text_cut_three_bytes_in",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-double-auxiliary-text-cut-three-bytes-in");
        },
    );
}

// abort-double-pinyin-auxiliary-text-leading-apostrophe: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_double_pinyin_auxiliary_text_leading_apostrophe() {
    run_aborting(
        "cursor_sentence_refusals::abort_double_pinyin_auxiliary_text_leading_apostrophe",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-double-pinyin-auxiliary-text-leading-apostrophe");
        },
    );
}

// abort-full-pinyin-auxiliary-text-doubled-leading-apostrophe: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_full_pinyin_auxiliary_text_doubled_leading_apostrophe() {
    run_aborting(
        "cursor_sentence_refusals::abort_full_pinyin_auxiliary_text_doubled_leading_apostrophe",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "abort-full-pinyin-auxiliary-text-doubled-leading-apostrophe",
            );
        },
    );
}

// abort-full-pinyin-auxiliary-text-leading-apostrophe: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_full_pinyin_auxiliary_text_leading_apostrophe() {
    run_aborting(
        "cursor_sentence_refusals::abort_full_pinyin_auxiliary_text_leading_apostrophe",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-full-pinyin-auxiliary-text-leading-apostrophe");
        },
    );
}

// abort-get-character-offset-after-separator: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_character_offset_after_separator() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_character_offset_after_separator",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-character-offset-after-separator");
        },
    );
}

// abort-get-character-offset-past-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_character_offset_past_matrix() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_character_offset_past_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-character-offset-past-matrix");
        },
    );
}

// abort-get-left-pinyin-offset-after-separator: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_left_pinyin_offset_after_separator() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_left_pinyin_offset_after_separator",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-left-pinyin-offset-after-separator");
        },
    );
}

// abort-get-pinyin-key-empty-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_pinyin_key_empty_matrix() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_pinyin_key_empty_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-pinyin-key-empty-matrix");
        },
    );
}

// abort-get-pinyin-key-rest-empty-matrix: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_pinyin_key_rest_empty_matrix() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_pinyin_key_rest_empty_matrix",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-pinyin-key-rest-empty-matrix");
        },
    );
}

// abort-get-right-pinyin-offset-after-separator: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_right_pinyin_offset_after_separator() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_right_pinyin_offset_after_separator",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-right-pinyin-offset-after-separator");
        },
    );
}

// abort-get-right-pinyin-offset-nihao-5: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_right_pinyin_offset_nihao_5() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_right_pinyin_offset_nihao_5",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-right-pinyin-offset-nihao-5");
        },
    );
}

// abort-get-sentence-past-rows: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_get_sentence_past_rows() {
    run_aborting(
        "cursor_sentence_refusals::abort_get_sentence_past_rows",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-get-sentence-past-rows");
        },
    );
}

// abort-parse-full-pinyin-apostrophe: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_parse_full_pinyin_apostrophe() {
    run_aborting(
        "cursor_sentence_refusals::abort_parse_full_pinyin_apostrophe",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-parse-full-pinyin-apostrophe");
        },
    );
}

// cursor-and-sentence-neighbours: MATCH.
#[test]
fn cursor_and_sentence_neighbours() {
    run_j(
        "cursor_sentence_refusals::cursor_and_sentence_neighbours",
        Expect::RESULT,
        |k| b::case(k, "cursor-and-sentence-neighbours"),
        raw!(include!(
            "fixtures/cursor_sentence_refusals/1b6623d6918c03ac.rs"
        )),
        None,
    );
}

// get-pinyin-key-size-max-empty-matrix: MATCH.
#[test]
fn get_pinyin_key_size_max_empty_matrix() {
    run_j(
        "cursor_sentence_refusals::get_pinyin_key_size_max_empty_matrix",
        Expect::RESULT,
        |k| b::case(k, "get-pinyin-key-size-max-empty-matrix"),
        raw!(M(&[
            ("key", L(&[B(false), B(true)])),
            ("logs", L(&[])),
            ("rest", L(&[B(false), B(true)]))
        ])),
        None,
    );
}
