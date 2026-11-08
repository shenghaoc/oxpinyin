//! Resplit, divided and fuzzy matrix windows and every matrix reader on both facades (#682, #683, #1031). Generated from the frozen pin observations.

use crate::bodies::matrix_resplit_windows as b;
use crate::support::{Expect, run_j};

// matrix-resplit-surfaces-zhuyin-fangan: MATCH.
#[test]
fn matrix_resplit_surfaces_zhuyin_fangan() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_surfaces_zhuyin_fangan",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-surfaces-zhuyin-fangan"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/65f793bdbfac9959.rs"
        )),
        None,
    );
}

// matrix-resplit-surfaces-zhuyin-xian: MATCH.
#[test]
fn matrix_resplit_surfaces_zhuyin_xian() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_surfaces_zhuyin_xian",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-surfaces-zhuyin-xian"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/35355e0b91bd93db.rs"
        )),
        None,
    );
}

// matrix-resplit-zhuyin-banangang-plain-exact: MATCH.
#[test]
fn matrix_resplit_zhuyin_banangang_plain_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_zhuyin_banangang_plain_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-zhuyin-banangang-plain-exact"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/matrix_resplit_windows/e8d98bfc94516b66.rs"
        )),
        None,
    );
}

// matrix-resplit-zhuyin-banangang-plain-fuzzy: MATCH.
#[test]
fn matrix_resplit_zhuyin_banangang_plain_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_zhuyin_banangang_plain_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-zhuyin-banangang-plain-fuzzy"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/matrix_resplit_windows/a9dc0e2c456b967e.rs"
        )),
        None,
    );
}

// matrix-resplit-zhuyin-xian-plain-exact: MATCH.
#[test]
fn matrix_resplit_zhuyin_xian_plain_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_zhuyin_xian_plain_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-zhuyin-xian-plain-exact"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/matrix_resplit_windows/0a58dfd9314eaeeb.rs"
        )),
        None,
    );
}

// matrix-resplit-zhuyin-xian-plain-fuzzy: MATCH.
#[test]
fn matrix_resplit_zhuyin_xian_plain_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_zhuyin_xian_plain_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-zhuyin-xian-plain-fuzzy"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/matrix_resplit_windows/37626351eb588561.rs"
        )),
        None,
    );
}

// matrix-resplit-zhuyin-xian-tables-exact: MATCH.
#[test]
fn matrix_resplit_zhuyin_xian_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_zhuyin_xian_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-zhuyin-xian-tables-exact"),
        raw!(include!(
            "../../../oxpinyin-capi/tests/contract_data/fixtures/matrix_resplit_windows/0a58dfd9314eaeeb.rs"
        )),
        None,
    );
}
