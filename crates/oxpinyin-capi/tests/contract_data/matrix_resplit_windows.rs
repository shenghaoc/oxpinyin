//! Resplit, divided and fuzzy matrix windows and every matrix reader on both facades (#682, #683, #1031). Generated from the frozen pin observations.

use crate::bodies::matrix_resplit_windows as b;
use crate::support::{Expect, run_j};

// matrix-resplit-pinyin-anangang-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_anangang_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_anangang_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-anangang-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/0d1555be5ca3c792.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-anangang-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_anangang_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_anangang_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-anangang-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/f9dad5d6ea9af5c6.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-baguanangang-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_baguanangang_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_baguanangang_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-baguanangang-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/b065434ac09fe065.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-baguanangang-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_baguanangang_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_baguanangang_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-baguanangang-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/3177f74c78b0af69.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-banangang-plain-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_banangang_plain_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_banangang_plain_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-banangang-plain-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/e8d98bfc94516b66.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-banangang-plain-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_banangang_plain_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_banangang_plain_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-banangang-plain-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/a9dc0e2c456b967e.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-banangang-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_banangang_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_banangang_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-banangang-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/e9b236e0dfdf5421.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-banangang-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_banangang_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_banangang_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-banangang-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/c1f8ae9f6b75815f.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-chenanengang-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_chenanengang_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_chenanengang_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-chenanengang-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/563a804acb22f3b7.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-chenanengang-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_chenanengang_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_chenanengang_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-chenanengang-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/442398bfae2f6653.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-fanangang-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_fanangang_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_fanangang_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-fanangang-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/5269848ce730bbed.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-fanangang-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_fanangang_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_fanangang_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-fanangang-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/7afb318bcb8c9c5a.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xian-plain-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_xian_plain_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xian_plain_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xian-plain-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/0a58dfd9314eaeeb.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xian-plain-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_xian_plain_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xian_plain_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xian-plain-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/37626351eb588561.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xian-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_xian_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xian_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xian-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/52db397a4e79ad04.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xian-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_xian_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xian_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xian-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/b8bed7edf74cdd10.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xianai-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_xianai_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xianai_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xianai-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/56f91999e5c20fe0.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xianai-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_xianai_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xianai_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xianai-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/4279998b201c4f1b.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xiane-tables-exact: MATCH.
#[test]
fn matrix_resplit_pinyin_xiane_tables_exact() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xiane_tables_exact",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xiane-tables-exact"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/25236ba66afc2a91.rs"
        )),
        None,
    );
}

// matrix-resplit-pinyin-xiane-tables-fuzzy: MATCH.
#[test]
fn matrix_resplit_pinyin_xiane_tables_fuzzy() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_pinyin_xiane_tables_fuzzy",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-pinyin-xiane-tables-fuzzy"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/f22107f788169301.rs"
        )),
        None,
    );
}

// matrix-resplit-surfaces-pinyin-fangan: MATCH.
#[test]
fn matrix_resplit_surfaces_pinyin_fangan() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_surfaces_pinyin_fangan",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-surfaces-pinyin-fangan"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/9a7c6efebd8a9ae0.rs"
        )),
        None,
    );
}

// matrix-resplit-surfaces-pinyin-xian: MATCH.
#[test]
fn matrix_resplit_surfaces_pinyin_xian() {
    run_j(
        "matrix_resplit_windows::matrix_resplit_surfaces_pinyin_xian",
        Expect::RESULT,
        |k| b::case(k, "matrix-resplit-surfaces-pinyin-xian"),
        raw!(include!(
            "fixtures/matrix_resplit_windows/e18201452797f915.rs"
        )),
        None,
    );
}
