//! Zero-consumption sentence guesses after full pinyin, chewing and double pinyin parses (#710). Generated from the frozen pin observations.

use crate::bodies::zero_consumption as b;
use crate::support::{Expect, run_j};

// zero-consumption-zhuyin-chewings-!: MATCH.
#[test]
fn zero_consumption_zhuyin_chewings_empty() {
    run_j(
        "zero_consumption::zero_consumption_zhuyin_chewings_empty",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-zhuyin-chewings-empty"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-zhuyin-chewings-empty-prefix: MATCH.
#[test]
fn zero_consumption_zhuyin_chewings_empty_prefix() {
    run_j(
        "zero_consumption::zero_consumption_zhuyin_chewings_empty_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-zhuyin-chewings-empty-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-zhuyin-full_pinyins-!: MATCH.
#[test]
fn zero_consumption_zhuyin_full_pinyins_bang_prefix() {
    run_j(
        "zero_consumption::zero_consumption_zhuyin_full_pinyins_bang_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-zhuyin-full_pinyins-!-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-zhuyin-full_pinyins-empty: MATCH.
#[test]
fn zero_consumption_zhuyin_full_pinyins_n() {
    run_j(
        "zero_consumption::zero_consumption_zhuyin_full_pinyins_n",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-zhuyin-full_pinyins-n"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-zhuyin-full_pinyins-n-prefix: MATCH.
