//! GLib merge-sorted sentence tails, candidate and predicted lists, and the wrapping pinyin-length key (#921, #927, #1275). Generated from the frozen pin observations.

use crate::bodies::sort_pinyin_length as b;
use crate::support::{Expect, run_j};

// glib-sort-candidates-pinyin: MATCH.
#[test]
fn glib_sort_candidates_pinyin() {
    run_j(
        "sort_pinyin_length::glib_sort_candidates_pinyin",
        Expect::RESULT,
        |k| b::case(k, "glib-sort-candidates-pinyin"),
        raw!(include!("fixtures/sort_pinyin_length/ef1fad99d359aa6e.rs")),
        None,
    );
}

// glib-sort-predicted: MATCH.
#[test]
fn glib_sort_predicted() {
    run_j(
        "sort_pinyin_length::glib_sort_predicted",
        Expect::RESULT,
        |k| b::case(k, "glib-sort-predicted"),
        raw!(include!("fixtures/sort_pinyin_length/711332f87737739b.rs")),
        None,
    );
}

// glib-sort-tails-pinyin: MATCH.
#[test]
fn glib_sort_tails_pinyin() {
    run_j(
        "sort_pinyin_length::glib_sort_tails_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "glib-sort-tails-pinyin"),
        raw!(include!("fixtures/sort_pinyin_length/fa940aa785c7592d.rs")),
        None,
    );
}

// pinyin-length-wrap/span-15: MATCH.
#[test]
fn pinyin_length_wrap_span_15() {
    run_j(
        "sort_pinyin_length::pinyin_length_wrap_span_15",
        Expect::RESULT,
        |k| b::case(k, "pinyin-length-wrap/span-15"),
        raw!(include!("fixtures/sort_pinyin_length/eca552db9605c9a0.rs")),
        None,
    );
}

// pinyin-length-wrap/span-255: MATCH.
#[test]
fn pinyin_length_wrap_span_255() {
    run_j(
        "sort_pinyin_length::pinyin_length_wrap_span_255",
        Expect::RESULT,
        |k| b::case(k, "pinyin-length-wrap/span-255"),
        raw!(include!("fixtures/sort_pinyin_length/e0811270c69208ed.rs")),
        None,
    );
}

// pinyin-length-wrap/span-256: MATCH.
#[test]
fn pinyin_length_wrap_span_256() {
    run_j(
        "sort_pinyin_length::pinyin_length_wrap_span_256",
        Expect::RESULT,
        |k| b::case(k, "pinyin-length-wrap/span-256"),
        raw!(include!("fixtures/sort_pinyin_length/24ddac81d7c4b5ea.rs")),
        None,
    );
}

// pinyin-length-wrap/span-257: MATCH.
#[test]
fn pinyin_length_wrap_span_257() {
    run_j(
        "sort_pinyin_length::pinyin_length_wrap_span_257",
        Expect::RESULT,
        |k| b::case(k, "pinyin-length-wrap/span-257"),
        raw!(include!("fixtures/sort_pinyin_length/bb66130e6bf906e4.rs")),
        None,
    );
}

// pinyin-length-wrap/span-5: MATCH.
#[test]
fn pinyin_length_wrap_span_5() {
    run_j(
        "sort_pinyin_length::pinyin_length_wrap_span_5",
        Expect::RESULT,
        |k| b::case(k, "pinyin-length-wrap/span-5"),
        raw!(include!("fixtures/sort_pinyin_length/b3826fa4d2d2d23b.rs")),
        None,
    );
}

// pinyin-length-wrap/span-512: MATCH.
#[test]
fn pinyin_length_wrap_span_512() {
    run_j(
        "sort_pinyin_length::pinyin_length_wrap_span_512",
        Expect::RESULT,
        |k| b::case(k, "pinyin-length-wrap/span-512"),
        raw!(include!("fixtures/sort_pinyin_length/185e0185e0dc73df.rs")),
        None,
    );
}

// trellis-adjusted-score/divided-bian-off: MATCH.
#[test]
fn trellis_adjusted_score_divided_bian_off() {
    run_j(
        "sort_pinyin_length::trellis_adjusted_score_divided_bian_off",
        Expect::RESULT,
        |k| b::case(k, "trellis-adjusted-score/divided-bian-off"),
        raw!(M(&[
            (
                "bian",
                M(&[
                    ("guessed", B(true)),
                    ("parsed", I(4)),
                    ("sentences", L(&[S("便"), S("编"), S("变")]))
                ])
            ),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// trellis-adjusted-score/divided-bian-on: MATCH.
#[test]
fn trellis_adjusted_score_divided_bian_on() {
    run_j(
        "sort_pinyin_length::trellis_adjusted_score_divided_bian_on",
        Expect::RESULT,
        |k| b::case(k, "trellis-adjusted-score/divided-bian-on"),
        raw!(M(&[
            (
                "bian",
                M(&[
                    ("guessed", B(true)),
                    ("parsed", I(4)),
                    ("sentences", L(&[S("便"), S("编"), S("变")]))
                ])
            ),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// trellis-adjusted-score/resplit-jianai-off: MATCH.
#[test]
fn trellis_adjusted_score_resplit_jianai_off() {
    run_j(
        "sort_pinyin_length::trellis_adjusted_score_resplit_jianai_off",
        Expect::RESULT,
        |k| b::case(k, "trellis-adjusted-score/resplit-jianai-off"),
        raw!(M(&[
            (
                "jianai",
                M(&[
                    ("guessed", B(true)),
                    ("parsed", I(6)),
                    ("sentences", L(&[S("加奈"), S("加奈"), S("家乃")]))
                ])
            ),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// trellis-adjusted-score/resplit-jianai-on: MATCH.
#[test]
fn trellis_adjusted_score_resplit_jianai_on() {
    run_j(
        "sort_pinyin_length::trellis_adjusted_score_resplit_jianai_on",
        Expect::RESULT,
        |k| b::case(k, "trellis-adjusted-score/resplit-jianai-on"),
        raw!(M(&[
            (
                "jianai",
                M(&[
                    ("guessed", B(true)),
                    ("parsed", I(6)),
                    ("sentences", L(&[S("兼爱"), S("加奈"), S("兼爱")]))
                ])
            ),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// trellis-tail-sort/qian-off: MATCH.
#[test]
fn trellis_tail_sort_qian_off() {
    run_j(
        "sort_pinyin_length::trellis_tail_sort_qian_off",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "trellis-tail-sort/qian-off"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "qian",
                M(&[
                    ("guessed", B(true)),
                    ("parsed", I(4)),
                    ("sentences", L(&[S("前"), S("前"), S("千")]))
                ])
            )
        ])),
        None,
    );
}

// trellis-tail-sort/qian-on: MATCH.
#[test]
fn trellis_tail_sort_qian_on() {
    run_j(
        "sort_pinyin_length::trellis_tail_sort_qian_on",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "trellis-tail-sort/qian-on"),
        raw!(M(&[
            ("logs", L(&[])),
            (
                "qian",
                M(&[
                    ("guessed", B(true)),
                    ("parsed", I(4)),
                    ("sentences", L(&[S("前"), S("前"), S("千")]))
                ])
            )
        ])),
        None,
    );
}
