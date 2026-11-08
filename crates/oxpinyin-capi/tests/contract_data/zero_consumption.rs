//! Zero-consumption sentence guesses after full pinyin, chewing and double pinyin parses (#710). Generated from the frozen pin observations.

use crate::bodies::zero_consumption as b;
use crate::support::{Expect, run_j};

// zero-consumption-pinyin-chewings-!: MATCH.
#[test]
fn zero_consumption_pinyin_chewings_bang() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_chewings_bang",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-chewings-!"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-chewings-!-prefix: MATCH.
#[test]
fn zero_consumption_pinyin_chewings_bang_prefix() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_chewings_bang_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-chewings-!-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-chewings-empty: MATCH.
#[test]
fn zero_consumption_pinyin_chewings_empty() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_chewings_empty",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-chewings-empty"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-chewings-empty-prefix: MATCH.
#[test]
fn zero_consumption_pinyin_chewings_empty_prefix() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_chewings_empty_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-chewings-empty-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-double_pinyins-!: MATCH.
#[test]
fn zero_consumption_pinyin_double_pinyins_bang() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_double_pinyins_bang",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-double_pinyins-!"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-double_pinyins-!-prefix: MATCH.
#[test]
fn zero_consumption_pinyin_double_pinyins_bang_prefix() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_double_pinyins_bang_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-double_pinyins-!-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-double_pinyins-empty: MATCH.
#[test]
fn zero_consumption_pinyin_double_pinyins_empty() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_double_pinyins_empty",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-double_pinyins-empty"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-double_pinyins-empty-prefix: MATCH.
#[test]
fn zero_consumption_pinyin_double_pinyins_empty_prefix() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_double_pinyins_empty_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-double_pinyins-empty-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-full_pinyins-!: MATCH.
#[test]
fn zero_consumption_pinyin_full_pinyins_bang() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_full_pinyins_bang",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-full_pinyins-!"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-full_pinyins-!-prefix: MATCH.
#[test]
fn zero_consumption_pinyin_full_pinyins_bang_prefix() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_full_pinyins_bang_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-full_pinyins-!-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-full_pinyins-empty: MATCH.
#[test]
fn zero_consumption_pinyin_full_pinyins_empty() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_full_pinyins_empty",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-full_pinyins-empty"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-full_pinyins-empty-prefix: MATCH.
#[test]
fn zero_consumption_pinyin_full_pinyins_empty_prefix() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_full_pinyins_empty_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-full_pinyins-empty-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-full_pinyins-n: MATCH.
#[test]
fn zero_consumption_pinyin_full_pinyins_n() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_full_pinyins_n",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-full_pinyins-n"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}

// zero-consumption-pinyin-full_pinyins-n-prefix: MATCH.
#[test]
fn zero_consumption_pinyin_full_pinyins_n_prefix() {
    run_j(
        "zero_consumption::zero_consumption_pinyin_full_pinyins_n_prefix",
        Expect::RESULT,
        |k| b::case(k, "zero-consumption-pinyin-full_pinyins-n-prefix"),
        raw!(include!("fixtures/zero_consumption/32de4dbacd930f0a.rs")),
        None,
    );
}
