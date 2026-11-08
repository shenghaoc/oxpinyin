//! Integer boundaries of remembering, unigram increments, merged bigrams, training and the bigram export, NUL-terminated remember inputs and saves over a stale temporary index (#1175, #540, #752, #542). Generated from the frozen pin observations.

use crate::bodies::integer_limits_d18 as b;
use crate::support::{Expect, run_j};

// d18-bigram-export-3fffffff: MATCH.
#[test]
fn d18_bigram_export_3fffffff() {
    run_j(
        "integer_limits_d18::d18_bigram_export_3fffffff",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-3fffffff"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(2147483646)]))
        ])),
        None,
    );
}

// d18-bigram-export-40000000: MATCH.
#[test]
fn d18_bigram_export_40000000() {
    run_j(
        "integer_limits_d18::d18_bigram_export_40000000",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-40000000"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(-2147483648)]))
        ])),
        None,
    );
}
// d18-bigram-export-ffffffff: MATCH.
#[test]
fn d18_bigram_export_ffffffff() {
    run_j(
        "integer_limits_d18::d18_bigram_export_ffffffff",
        Expect::RESULT,
        |k| b::case(k, "d18-bigram-export-ffffffff"),
        raw!(M(&[
            ("has", B(true)),
            ("logs", L(&[])),
            ("row", L(&[B(true), S("我好"), S("wo'hao"), I(-2)]))
        ])),
        None,
    );
}
// d18-merge-total-zero-pinyin: MATCH.
#[test]
fn d18_merge_total_zero_pinyin() {
    run_j(
        "integer_limits_d18::d18_merge_total_zero_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-merge-total-zero-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/c5e4fd2fca5d3b4d.rs")),
        None,
    );
}
// d18-remember-multiply-1431655765: MATCH.
#[test]
fn d18_remember_multiply_1431655765() {
    run_j(
        "integer_limits_d18::d18_remember_multiply_1431655765",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-multiply-1431655765"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(11)),
            (
                "steps",
                L(&[L(&[
                    I(1431655765),
                    B(true),
                    L(&[L(&[
                        B(true),
                        S("你好世界"),
                        S("ni'hao'shi'jie"),
                        I(1431655765)
                    ])]),
                    L(&[B(true), I(4294967295)])
                ])])
            )
        ])),
        None,
    );
}

// d18-remember-multiply-1431655766: MATCH.
#[test]
fn d18_remember_multiply_1431655766() {
    run_j(
        "integer_limits_d18::d18_remember_multiply_1431655766",
        Expect::RESULT,
        |k| b::case(k, "d18-remember-multiply-1431655766"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(11)),
            (
                "steps",
                L(&[L(&[
                    I(1431655766),
                    B(true),
                    L(&[L(&[
                        B(true),
                        S("你好世界"),
                        S("ni'hao'shi'jie"),
                        I(1431655766)
                    ])]),
                    L(&[B(true), I(2)])
                ])])
            )
        ])),
        None,
    );
}
// d18-train-total-overflow-pinyin: MATCH.
#[test]
fn d18_train_total_overflow_pinyin() {
    run_j(
        "integer_limits_d18::d18_train_total_overflow_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-train-total-overflow-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/907723db5a4587c8.rs")),
        None,
    );
}
// d18-unigram-total-boundary-pinyin: MATCH.
#[test]
fn d18_unigram_total_boundary_pinyin() {
    run_j(
        "integer_limits_d18::d18_unigram_total_boundary_pinyin",
        Expect::RESULT,
        |k| b::case(k, "d18-unigram-total-boundary-pinyin"),
        raw!(include!("fixtures/integer_limits_d18/4d52ca68da4a63d4.rs")),
        None,
    );
}
// remember-known-nul-suffix-pinyin: MATCH.
#[test]
fn remember_known_nul_suffix_pinyin() {
    run_j(
        "integer_limits_d18::remember_known_nul_suffix_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::case(k, "remember-known-nul-suffix-pinyin"),
        raw!(M(&[
            ("logs", L(&[])),
            ("parsed", I(5)),
            ("ret", B(true)),
            ("rows", L(&[L(&[B(true), S("你好"), S("ni'hao"), I(1)])]))
        ])),
        None,
    );
}
