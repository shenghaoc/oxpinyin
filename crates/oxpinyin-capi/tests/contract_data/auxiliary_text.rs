//! Auxiliary text renderers over full, double and chewing parses (#542, #925). Generated from the frozen pin observations.

use crate::bodies::auxiliary_text as b;
use crate::support::{Expect, run_j};

// aux-text-chewing-cursors: MATCH.
#[test]
fn aux_text_chewing_cursors() {
    run_j(
        "auxiliary_text::aux_text_chewing_cursors",
        Expect::RESULT,
        |k| b::case(k, "aux-text-chewing-cursors"),
        raw!(include!("fixtures/auxiliary_text/72d17bcd266cb49a.rs")),
        None,
    );
}

// aux-text-chewing-overrun-cursor: MATCH.
#[test]
fn aux_text_chewing_overrun_cursor() {
    run_j(
        "auxiliary_text::aux_text_chewing_overrun_cursor",
        Expect::RESULT,
        |k| b::case(k, "aux-text-chewing-overrun-cursor"),
        raw!(include!("fixtures/auxiliary_text/e73c024884eac3a6.rs")),
        None,
    );
}

// aux-text-chewing-parse: MATCH.
#[test]
fn aux_text_chewing_parse() {
    run_j(
        "auxiliary_text::aux_text_chewing_parse",
        Expect::RESULT,
        |k| b::case(k, "aux-text-chewing-parse"),
        raw!(include!("fixtures/auxiliary_text/794c7b31986a0375.rs")),
        None,
    );
}

// aux-text-chewing-zhuyin-cursors: MATCH.
#[test]
fn aux_text_divided_table() {
    run_j(
        "auxiliary_text::aux_text_divided_table",
        Expect::RESULT,
        |k| b::case(k, "aux-text-divided-table"),
        raw!(include!("fixtures/auxiliary_text/7316bf1296a065e8.rs")),
        None,
    );
}

// aux-text-double-parse: MATCH.
#[test]
fn aux_text_double_parse() {
    run_j(
        "auxiliary_text::aux_text_double_parse",
        Expect::RESULT,
        |k| b::case(k, "aux-text-double-parse"),
        raw!(include!("fixtures/auxiliary_text/f212f8569e31c8bd.rs")),
        None,
    );
}

// aux-text-full-parse: MATCH.
#[test]
fn aux_text_full_parse() {
    run_j(
        "auxiliary_text::aux_text_full_parse",
        Expect::RESULT,
        |k| b::case(k, "aux-text-full-parse"),
        raw!(include!("fixtures/auxiliary_text/1931bdd022e1df3e.rs")),
        None,
    );
}

// aux-text-full-parse-apostrophe: MATCH.
#[test]
fn aux_text_full_parse_apostrophe() {
    run_j(
        "auxiliary_text::aux_text_full_parse_apostrophe",
        Expect::RESULT,
        |k| b::case(k, "aux-text-full-parse-apostrophe"),
        raw!(include!("fixtures/auxiliary_text/3e24b7c1cb318c51.rs")),
        None,
    );
}

// aux-text-fuzzy-keys: MATCH.
#[test]
fn aux_text_fuzzy_keys() {
    run_j(
        "auxiliary_text::aux_text_fuzzy_keys",
        Expect::RESULT,
        |k| b::case(k, "aux-text-fuzzy-keys"),
        raw!(include!("fixtures/auxiliary_text/9705d0e2dee7e0a8.rs")),
        None,
    );
}

// aux-text-mixed-parses: MATCH.
#[test]
fn aux_text_mixed_parses() {
    run_j(
        "auxiliary_text::aux_text_mixed_parses",
        Expect::RESULT,
        |k| b::case(k, "aux-text-mixed-parses"),
        raw!(M(&[
            ("chewing", L(&[B(true), S("ㄒㄧ|ㄢ ")])),
            ("double_pinyin", L(&[B(true), S("xian| ")])),
            ("full_pinyin", L(&[B(true), S("xi|an ")])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// aux-text-no-parse: MATCH.
#[test]
fn aux_text_no_parse() {
    run_j(
        "auxiliary_text::aux_text_no_parse",
        Expect::RESULT,
        |k| b::case(k, "aux-text-no-parse"),
        raw!(M(&[
            ("chewing", L(&[B(false), S("")])),
            ("double_pinyin", L(&[B(false), S("")])),
            ("full_pinyin", L(&[B(false), S("")])),
            ("logs", L(&[]))
        ])),
        None,
    );
}

// aux-text-resplit-table: MATCH.
#[test]
fn aux_text_resplit_table() {
    run_j(
        "auxiliary_text::aux_text_resplit_table",
        Expect::RESULT,
        |k| b::case(k, "aux-text-resplit-table"),
        raw!(include!("fixtures/auxiliary_text/e30401c6a14ddd85.rs")),
        None,
    );
}

// aux-text-tones: MATCH.
#[test]
fn aux_text_tones() {
    run_j(
        "auxiliary_text::aux_text_tones",
        Expect::RESULT,
        |k| b::case(k, "aux-text-tones"),
        raw!(include!("fixtures/auxiliary_text/19e6d60862f6f878.rs")),
        None,
    );
}
