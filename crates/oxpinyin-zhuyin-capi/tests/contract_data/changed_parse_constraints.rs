//! Chosen constraints kept or cleared across changed, empty and intermediate parses (#710). Generated from the frozen pin observations.

use crate::bodies::changed_parse_constraints as b;
use crate::support::{Expect, run_j};

// parse-constraints-zhuyin-changed-suffix-direct: MATCH.
#[test]
fn parse_constraints_zhuyin_changed_suffix_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_zhuyin_changed_suffix_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-zhuyin-changed-suffix-direct"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/changed_parse_constraints/c92bd594ed7dd085.rs"
        )),
        None,
    );
}

// parse-constraints-zhuyin-edit-within-span-direct: MATCH.
#[test]
fn parse_constraints_zhuyin_edit_within_span_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_zhuyin_edit_within_span_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-zhuyin-edit-within-span-direct"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/changed_parse_constraints/46975bc7715ea076.rs"
        )),
        None,
    );
}

// parse-constraints-zhuyin-span-past-end-direct: MATCH.
#[test]
fn parse_constraints_zhuyin_span_past_end_direct() {
    run_j(
        "changed_parse_constraints::parse_constraints_zhuyin_span_past_end_direct",
        Expect::RESULT,
        |k| b::case(k, "parse-constraints-zhuyin-span-past-end-direct"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/changed_parse_constraints/735f93626e804278.rs"
        )),
        None,
    );
}

// parse-constraints-zhuyin-temporary-shorter-intermediate-guess: MATCH.
#[test]
fn parse_constraints_zhuyin_temporary_shorter_intermediate_guess() {
    run_j(
        "changed_parse_constraints::parse_constraints_zhuyin_temporary_shorter_intermediate_guess",
        Expect::RESULT,
        |k| {
            b::case(
                k,
                "parse-constraints-zhuyin-temporary-shorter-intermediate-guess",
            )
        },
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/changed_parse_constraints/b559aba92f22572a.rs"
        )),
        None,
    );
}
