//! Chosen constraints kept or cleared across changed, empty and intermediate parses (#710). Generated from the frozen pin observations.

use crate::bodies::changed_parse_constraints as b;
use crate::support::{Expect, run_j};

// parse-constraints-pinyin-changed-suffix-direct: MATCH.
#[test]
fn parse_constraints_pinyin_changed_suffix_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_changed_suffix_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-changed-suffix-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/c92bd594ed7dd085.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-changed-suffix-empty: MATCH.
#[test]
fn parse_constraints_pinyin_changed_suffix_empty() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_changed_suffix_empty",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-changed-suffix-empty"),
        raw!(include!(
            "fixtures/changed_parse_constraints/ec5a34e2c40009d6.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-changed-suffix-empty-guess: MATCH.
#[test]
fn parse_constraints_pinyin_changed_suffix_empty_guess() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_changed_suffix_empty_guess",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-changed-suffix-empty-guess"),
        raw!(include!(
            "fixtures/changed_parse_constraints/684d4bda0cbee9e2.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-changed-suffix-guess: MATCH.
#[test]
fn parse_constraints_pinyin_changed_suffix_guess() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_changed_suffix_guess",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-changed-suffix-guess"),
        raw!(include!(
            "fixtures/changed_parse_constraints/4ddb041c6980bd21.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-different-first-direct: MATCH.
#[test]
fn parse_constraints_pinyin_different_first_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_different_first_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-different-first-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/8bd5504464c616bb.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-edit-within-span-direct: MATCH.
#[test]
fn parse_constraints_pinyin_edit_within_span_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_edit_within_span_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-edit-within-span-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/46975bc7715ea076.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-longer-direct: MATCH.
#[test]
fn parse_constraints_pinyin_longer_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_longer_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-longer-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/c7f3f06529f08c64.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-shorter-direct: MATCH.
#[test]
fn parse_constraints_pinyin_shorter_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_shorter_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-shorter-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/1070e192d31cd9ac.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-span-past-end-direct: MATCH.
#[test]
fn parse_constraints_pinyin_span_past_end_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_span_past_end_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-span-past-end-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/735f93626e804278.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-span-past-end-empty-guess: MATCH.
#[test]
fn parse_constraints_pinyin_span_past_end_empty_guess() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_span_past_end_empty_guess",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-span-past-end-empty-guess"),
        raw!(include!(
            "fixtures/changed_parse_constraints/7ab2ddc60e1e1777.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-temporary-mismatch-direct: MATCH.
#[test]
fn parse_constraints_pinyin_temporary_mismatch_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_temporary_mismatch_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-temporary-mismatch-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/631ed7bb2be89803.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-temporary-mismatch-intermediate-guess: MATCH.
#[test]
fn parse_constraints_pinyin_temporary_mismatch_intermediate_guess() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_temporary_mismatch_intermediate_guess",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "parse-constraints-pinyin-temporary-mismatch-intermediate-guess",
            )
        },
        raw!(include!(
            "fixtures/changed_parse_constraints/f1de2daca9897b51.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-temporary-shorter-direct: MATCH.
#[test]
fn parse_constraints_pinyin_temporary_shorter_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_temporary_shorter_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-temporary-shorter-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/e3d1f58b2306c7d4.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-temporary-shorter-intermediate-guess: MATCH.
#[test]
fn parse_constraints_pinyin_temporary_shorter_intermediate_guess() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_temporary_shorter_intermediate_guess",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "parse-constraints-pinyin-temporary-shorter-intermediate-guess",
            )
        },
        raw!(include!(
            "fixtures/changed_parse_constraints/b559aba92f22572a.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-two-choices-direct: MATCH.
#[test]
fn parse_constraints_pinyin_two_choices_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_two_choices_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-two-choices-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/45c83d5c3991d8df.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-whole-span-kept-direct: MATCH.
#[test]
fn parse_constraints_pinyin_whole_span_kept_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_whole_span_kept_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-whole-span-kept-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/99e0f7af5f40d3aa.rs"
        )),
        None,
    );
}

// parse-constraints-pinyin-whole-span-past-end-direct: MATCH.
#[test]
fn parse_constraints_pinyin_whole_span_past_end_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_pinyin_whole_span_past_end_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-pinyin-whole-span-past-end-direct"),
        raw!(include!(
            "fixtures/changed_parse_constraints/e3b5f9a5db9e7d5d.rs"
        )),
        None,
    );
}
