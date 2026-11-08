//! Merged system and user gram lookups: stored-order interleave, duplicates, zero counts and short user grams in the n-best step, DYNAMIC_ADJUST and the phrase segmenter (#1175, #757, #918). Generated from the frozen pin observations.

use crate::bodies::gram_lookup as b;
use crate::support::{Expect, run_aborting, run_j};

// abort-guess-candidates-dynamic-adjust-short-user-gram-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_candidates_dynamic_adjust_short_user_gram_pinyin() {
    run_aborting(
        "gram_lookup::abort_guess_candidates_dynamic_adjust_short_user_gram_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(
                k,
                "abort-guess-candidates-dynamic-adjust-short-user-gram-pinyin",
            );
        },
    );
}

// abort-guess-sentence-short-user-gram-no-system-row: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_sentence_short_user_gram_no_system_row() {
    run_aborting(
        "gram_lookup::abort_guess_sentence_short_user_gram_no_system_row",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-sentence-short-user-gram-no-system-row");
        },
    );
}

// abort-guess-sentence-short-user-gram-system-row-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_guess_sentence_short_user_gram_system_row_pinyin() {
    run_aborting(
        "gram_lookup::abort_guess_sentence_short_user_gram_system_row_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-guess-sentence-short-user-gram-system-row-pinyin");
        },
    );
}

// abort-phrase-segment-prefix-short-user-gram-pinyin: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_phrase_segment_prefix_short_user_gram_pinyin() {
    run_aborting(
        "gram_lookup::abort_phrase_segment_prefix_short_user_gram_pinyin",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-phrase-segment-prefix-short-user-gram-pinyin");
        },
    );
}

// abort-phrase-segment-short-user-gram: the pin's abort, reproduced; POLICY-ACCEPTED (pin-abort-assert-line.md): the pin also wrote glibc's assertion line.
#[test]
fn abort_phrase_segment_short_user_gram() {
    run_aborting(
        "gram_lookup::abort_phrase_segment_short_user_gram",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| {
            b::case(k, "abort-phrase-segment-short-user-gram");
        },
    );
}

// guess-candidates-short-user-gram-dynamic-adjust-off: MATCH.
#[test]
fn guess_candidates_short_user_gram_dynamic_adjust_off() {
    run_j(
        "gram_lookup::guess_candidates_short_user_gram_dynamic_adjust_off",
        Expect::RESULT,
        |k| b::case(k, "guess-candidates-short-user-gram-dynamic-adjust-off"),
        raw!(include!("fixtures/gram_lookup/9cb66bd80f0fafcb.rs")),
        None,
    );
}

// guess-unrelated-short-user-gram: MATCH.
#[test]
fn guess_unrelated_short_user_gram() {
    run_j(
        "gram_lookup::guess_unrelated_short_user_gram",
        Expect::RESULT,
        |k| b::case(k, "guess-unrelated-short-user-gram"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("sentences", L(&[S("你好"), S("你好"), S("你好")]))
        ])),
        None,
    );
}

// parse-dynamic-adjust-short-user-gram-pinyin: MATCH.
#[test]
fn parse_dynamic_adjust_short_user_gram_pinyin() {
    run_j(
        "gram_lookup::parse_dynamic_adjust_short_user_gram_pinyin",
        Expect::RESULT,
        |k| b::case(k, "parse-dynamic-adjust-short-user-gram-pinyin"),
        raw!(M(&[("length", I(5)), ("logs", L(&[])), ("parsed", I(5))])),
        None,
    );
}

// phrase-segment-nonscalar-before-short-user-gram-pinyin: MATCH.
#[test]
fn phrase_segment_nonscalar_before_short_user_gram_pinyin() {
    run_j(
        "gram_lookup::phrase_segment_nonscalar_before_short_user_gram_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "phrase-segment-nonscalar-before-short-user-gram-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[I(0), I(0)]))
        ])),
        None,
    );
}

// phrase-segment-nonscalar-raw-key-pinyin: MATCH.
#[test]
fn phrase_segment_nonscalar_raw_key_pinyin() {
    run_j(
        "gram_lookup::phrase_segment_nonscalar_raw_key_pinyin",
        Expect::stderr(b""),
        |k| b::case(k, "phrase-segment-nonscalar-raw-key-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(true)),
            ("tokens", L(&[I(16777217)]))
        ])),
        None,
    );
}

// phrase-segment-prefix-sound-user-gram-pinyin: MATCH.
#[test]
fn phrase_segment_prefix_sound_user_gram_pinyin() {
    run_j(
        "gram_lookup::phrase_segment_prefix_sound_user_gram_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "phrase-segment-prefix-sound-user-gram-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[I(0), I(0)]))
        ])),
        None,
    );
}

// phrase-segment-prefix-unrelated-short-user-gram-pinyin: MATCH.
#[test]
fn phrase_segment_prefix_unrelated_short_user_gram_pinyin() {
    run_j(
        "gram_lookup::phrase_segment_prefix_unrelated_short_user_gram_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "phrase-segment-prefix-unrelated-short-user-gram-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("ret", B(false)),
            ("tokens", L(&[I(0), I(0)]))
        ])),
        None,
    );
}

// system-gram-duplicate-guess: MATCH.
#[test]
fn system_gram_duplicate_guess() {
    run_j(
        "gram_lookup::system_gram_duplicate_guess",
        Expect::RESULT,
        |k| b::case(k, "system-gram-duplicate-guess"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("sentences", L(&[S("尼好"), S("尼号"), S("尼豪")]))
        ])),
        None,
    );
}

// system-gram-unsorted-guess: MATCH.
#[test]
fn system_gram_unsorted_guess() {
    run_j(
        "gram_lookup::system_gram_unsorted_guess",
        Expect::RESULT,
        |k| b::case(k, "system-gram-unsorted-guess"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("sentences", L(&[S("你好"), S("你好"), S("你号")]))
        ])),
        None,
    );
}

// user-gram-duplicate-dynamic-adjust: MATCH.
#[test]
fn user_gram_duplicate_dynamic_adjust() {
    run_j(
        "gram_lookup::user_gram_duplicate_dynamic_adjust",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-dynamic-adjust"),
        raw!(include!("fixtures/gram_lookup/f21b021decc7f674.rs")),
        None,
    );
}

// user-gram-duplicate-guess: MATCH.
#[test]
fn user_gram_duplicate_guess() {
    run_j(
        "gram_lookup::user_gram_duplicate_guess",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-guess"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("sentences", L(&[S("尼号"), S("尼号"), S("尼好")]))
        ])),
        None,
    );
}

// user-gram-duplicate-phrase-segment: MATCH.
#[test]
fn user_gram_duplicate_phrase_segment() {
    run_j(
        "gram_lookup::user_gram_duplicate_phrase_segment",
        Expect::RESULT,
        |k| b::case(k, "user-gram-duplicate-phrase-segment"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "今天天气很好",
                L(&[
                    B(true),
                    L(&[
                        I(16800245),
                        I(0),
                        I(16810538),
                        I(0),
                        I(16779492),
                        I(16779429)
                    ])
                ])
            )
        ])),
        None,
    );
}

// user-gram-unsorted-dynamic-adjust: MATCH.
#[test]
fn user_gram_unsorted_dynamic_adjust() {
    run_j(
        "gram_lookup::user_gram_unsorted_dynamic_adjust",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-dynamic-adjust"),
        raw!(include!("fixtures/gram_lookup/9cb66bd80f0fafcb.rs")),
        None,
    );
}

// user-gram-unsorted-guess: MATCH.
#[test]
fn user_gram_unsorted_guess() {
    run_j(
        "gram_lookup::user_gram_unsorted_guess",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-guess"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("sentences", L(&[S("你好"), S("你好"), S("你好")]))
        ])),
        None,
    );
}

// user-gram-unsorted-phrase-segment: POLICY-ACCEPTED (1297-gram-partition-segmentation.md).
#[test]
fn user_gram_unsorted_phrase_segment() {
    run_j(
        "gram_lookup::user_gram_unsorted_phrase_segment",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "user-gram-unsorted-phrase-segment"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "今天天气很好",
                L(&[
                    B(true),
                    L(&[
                        I(16800245),
                        I(0),
                        I(16810538),
                        I(0),
                        I(16779492),
                        I(16779429)
                    ])
                ])
            )
        ])),
        None,
    );
}

// user-gram-unsorted-range-boundary: MATCH.
#[test]
fn user_gram_unsorted_range_boundary() {
    run_j(
        "gram_lookup::user_gram_unsorted_range_boundary",
        Expect::RESULT,
        |k| b::case(k, "user-gram-unsorted-range-boundary"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("sentences", L(&[S("逆好"), S("逆号"), S("逆豪")]))
        ])),
        None,
    );
}

// user-gram-zero-count-guess: MATCH.
#[test]
fn user_gram_zero_count_guess() {
    run_j(
        "gram_lookup::user_gram_zero_count_guess",
        Expect::RESULT,
        |k| b::case(k, "user-gram-zero-count-guess"),
        raw!(M(&[
            ("guessed", B(true)),
            ("logs", L(&[])),
            ("parsed", I(1)),
            ("sentences", L(&[S("阿"), S("阿"), S("啊")]))
        ])),
        None,
    );
}
