//! Merged system and user gram lookups: stored-order interleave, duplicates, zero counts and short user grams in the n-best step, DYNAMIC_ADJUST and the phrase segmenter (#1175, #757, #918). Generated from the frozen pin observations.

use crate::bodies::gram_lookup as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-guess-candidates-dynamic-adjust-short-user-gram-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_candidates_dynamic_adjust_short_user_gram_zhuyin() {
    run_aborting(
        "gram_lookup::abort_guess_candidates_dynamic_adjust_short_user_gram_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "abort-guess-candidates-dynamic-adjust-short-user-gram-zhuyin",
            );
        },
    );
}

// abort-guess-sentence-short-user-gram-system-row-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_sentence_short_user_gram_system_row_zhuyin() {
    run_aborting(
        "gram_lookup::abort_guess_sentence_short_user_gram_system_row_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-sentence-short-user-gram-system-row-zhuyin");
        },
    );
}

// abort-phrase-segment-prefix-short-user-gram-zhuyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_phrase_segment_prefix_short_user_gram_zhuyin() {
    run_aborting(
        "gram_lookup::abort_phrase_segment_prefix_short_user_gram_zhuyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-phrase-segment-prefix-short-user-gram-zhuyin");
        },
    );
}

// parse-dynamic-adjust-short-user-gram-zhuyin: MATCH.
#[test]
fn parse_dynamic_adjust_short_user_gram_zhuyin() {
    run_j(
        "gram_lookup::parse_dynamic_adjust_short_user_gram_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "parse-dynamic-adjust-short-user-gram-zhuyin"),
        raw!(M(&[("length", I(5)), ("logs", L(&[])), ("parsed", I(5))])),
        None,
    );
}

// phrase-segment-nonscalar-before-short-user-gram-zhuyin: MATCH.
#[test]
fn phrase_segment_nonscalar_before_short_user_gram_zhuyin() {
    run_j(
        "gram_lookup::phrase_segment_nonscalar_before_short_user_gram_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "phrase-segment-nonscalar-before-short-user-gram-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[I(0), I(0)]))
        ])),
        None,
    );
}

// phrase-segment-nonscalar-raw-key-zhuyin: MATCH.
#[test]
fn phrase_segment_nonscalar_raw_key_zhuyin() {
    run_j(
        "gram_lookup::phrase_segment_nonscalar_raw_key_zhuyin",
        Expect::stderr(b""),
        |k| b::case(k, "phrase-segment-nonscalar-raw-key-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16777217)]))
        ])),
        None,
    );
}

// phrase-segment-prefix-sound-user-gram-zhuyin: MATCH.
#[test]
fn phrase_segment_prefix_sound_user_gram_zhuyin() {
    run_j(
        "gram_lookup::phrase_segment_prefix_sound_user_gram_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "phrase-segment-prefix-sound-user-gram-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[I(0), I(0)]))
        ])),
        None,
    );
}

// phrase-segment-prefix-unrelated-short-user-gram-zhuyin: MATCH.
#[test]
fn phrase_segment_prefix_unrelated_short_user_gram_zhuyin() {
    run_j(
        "gram_lookup::phrase_segment_prefix_unrelated_short_user_gram_zhuyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "phrase-segment-prefix-unrelated-short-user-gram-zhuyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[I(0), I(0)]))
        ])),
        None,
    );
}
