//! Empty parses, library unloads and a NORMAL choice forced through an empty parse (#695, #709, register row 54). Generated from the frozen pin observations.

use crate::bodies::row54_empty_forcing as b;
use crate::support::{Expect, run_j};

// row54-empty-pinyin-chewings: MATCH.
#[test]
fn row54_empty_pinyin_chewings() {
    run_j(
        "row54_empty_forcing::row54_empty_pinyin_chewings",
        Expect::RESULT,
        |k| b::case(k, "row54-empty-pinyin-chewings"),
        raw!(M(&[
            ("empty_guess", B(false)),
            ("empty_parse", I(0)),
            ("first_guess", B(true)),
            ("first_parse", I(6)),
            ("logs", L(&[])),
            ("recovered_guess", B(true)),
            ("reparse", I(6))
        ])),
        None,
    );
}

// row54-empty-pinyin-double: MATCH.
#[test]
fn row54_empty_pinyin_double() {
    run_j(
        "row54_empty_forcing::row54_empty_pinyin_double",
        Expect::RESULT,
        |k| b::case(k, "row54-empty-pinyin-double"),
        raw!(M(&[
            ("empty_guess", B(false)),
            ("empty_parse", I(0)),
            ("first_guess", B(true)),
            ("first_parse", I(4)),
            ("logs", L(&[])),
            ("recovered_guess", B(true)),
            ("reparse", I(4))
        ])),
        None,
    );
}

// row54-empty-pinyin-full_pinyins: MATCH.
#[test]
fn row54_empty_pinyin_full_pinyins() {
    run_j(
        "row54_empty_forcing::row54_empty_pinyin_full_pinyins",
        Expect::RESULT,
        |k| b::case(k, "row54-empty-pinyin-full_pinyins"),
        raw!(M(&[
            ("empty_guess", B(false)),
            ("empty_parse", I(0)),
            ("first_guess", B(true)),
            ("first_parse", I(5)),
            ("logs", L(&[])),
            ("recovered_guess", B(true)),
            ("reparse", I(5))
        ])),
        None,
    );
}

// row54-forcing-pinyin-empty: MATCH.
#[test]
fn row54_forcing_pinyin_empty() {
    run_j(
        "row54_empty_forcing::row54_forcing_pinyin_empty",
        Expect::RESULT,
        |k| b::case(k, "row54-forcing-pinyin-empty"),
        raw!(include!("fixtures/row54_empty_forcing/3ef55d5e40844051.rs")),
        None,
    );
}

// row54-forcing-pinyin-empty-guess: MATCH.
#[test]
fn row54_forcing_pinyin_empty_guess() {
    run_j(
        "row54_empty_forcing::row54_forcing_pinyin_empty_guess",
        Expect::RESULT,
        |k| b::case(k, "row54-forcing-pinyin-empty-guess"),
        raw!(include!("fixtures/row54_empty_forcing/c29005a442adc380.rs")),
        None,
    );
}

// row54-forcing-pinyin-no-empty: MATCH.
#[test]
fn row54_forcing_pinyin_no_empty() {
    run_j(
        "row54_empty_forcing::row54_forcing_pinyin_no_empty",
        Expect::RESULT,
        |k| b::case(k, "row54-forcing-pinyin-no-empty"),
        raw!(include!("fixtures/row54_empty_forcing/8acfe7a5d98e261d.rs")),
        None,
    );
}

// row54-unload-pinyin: MATCH.
#[test]
fn row54_unload_pinyin() {
    run_j(
        "row54_empty_forcing::row54_unload_pinyin",
        Expect::RESULT,
        |k| b::case(k, "row54-unload-pinyin"),
        raw!(include!("fixtures/row54_empty_forcing/6db8e65b51117db6.rs")),
        None,
    );
}

// row54-unload-pinyin-reparse: MATCH.
#[test]
fn row54_unload_pinyin_reparse() {
    run_j(
        "row54_empty_forcing::row54_unload_pinyin_reparse",
        Expect::RESULT,
        |k| b::case(k, "row54-unload-pinyin-reparse"),
        raw!(include!("fixtures/row54_empty_forcing/f9bc530bfcd9b85c.rs")),
        None,
    );
}
