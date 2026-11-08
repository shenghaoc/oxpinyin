//! Empty parses, library unloads and a NORMAL choice forced through an empty parse (#695, #709, register row 54). Generated from the frozen pin observations.

use crate::bodies::row54_empty_forcing as b;
use crate::support::{Expect, run_j};

// row54-empty-zhuyin-chewings: MATCH.
#[test]
fn row54_empty_zhuyin_chewings() {
    run_j(
        "row54_empty_forcing::row54_empty_zhuyin_chewings",
        Expect::RESULT,
        |k| b::case(k, "row54-empty-zhuyin-chewings"),
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

// row54-empty-zhuyin-full_pinyins: MATCH.
#[test]
fn row54_empty_zhuyin_full_pinyins() {
    run_j(
        "row54_empty_forcing::row54_empty_zhuyin_full_pinyins",
        Expect::RESULT,
        |k| b::case(k, "row54-empty-zhuyin-full_pinyins"),
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

// row54-forcing-zhuyin-empty: MATCH.
#[test]
fn row54_forcing_zhuyin_empty() {
    run_j(
        "row54_empty_forcing::row54_forcing_zhuyin_empty",
        Expect::RESULT,
        |k| b::case(k, "row54-forcing-zhuyin-empty"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/row54_empty_forcing/3ef55d5e40844051.rs"
        )),
        None,
    );
}

// row54-forcing-zhuyin-empty-guess: MATCH.
#[test]
fn row54_forcing_zhuyin_empty_guess() {
    run_j(
        "row54_empty_forcing::row54_forcing_zhuyin_empty_guess",
        Expect::RESULT,
        |k| b::case(k, "row54-forcing-zhuyin-empty-guess"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/row54_empty_forcing/c29005a442adc380.rs"
        )),
        None,
    );
}

// row54-forcing-zhuyin-no-empty: MATCH.
#[test]
fn row54_forcing_zhuyin_no_empty() {
    run_j(
        "row54_empty_forcing::row54_forcing_zhuyin_no_empty",
        Expect::RESULT,
        |k| b::case(k, "row54-forcing-zhuyin-no-empty"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/row54_empty_forcing/8acfe7a5d98e261d.rs"
        )),
        None,
    );
}

// row54-unload-zhuyin: MATCH.
#[test]
fn row54_unload_zhuyin() {
    run_j(
        "row54_empty_forcing::row54_unload_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "row54-unload-zhuyin"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/row54_empty_forcing/6db8e65b51117db6.rs"
        )),
        None,
    );
}

// row54-unload-zhuyin-before: MATCH.
#[test]
fn row54_unload_zhuyin_before() {
    run_j(
        "row54_empty_forcing::row54_unload_zhuyin_before",
        Expect::RESULT,
        |k| b::case(k, "row54-unload-zhuyin-before"),
        raw!(include!("fixtures/row54_empty_forcing/81b0ba253d95cfa0.rs")),
        None,
    );
}

// row54-unload-zhuyin-reparse: MATCH.
#[test]
fn row54_unload_zhuyin_reparse() {
    run_j(
        "row54_empty_forcing::row54_unload_zhuyin_reparse",
        Expect::RESULT,
        |k| b::case(k, "row54-unload-zhuyin-reparse"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/row54_empty_forcing/f9bc530bfcd9b85c.rs"
        )),
        None,
    );
}
