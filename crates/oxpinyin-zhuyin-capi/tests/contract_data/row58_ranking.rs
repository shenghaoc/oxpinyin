//! Predicted-candidate ranking under seeded bigrams, PREFIX choices, large counts, wrapped totals and lambda edits (#1175, #697, #712). Generated from the frozen pin observations.

use crate::bodies::row58_ranking as b;
use crate::support::{Expect, run_j};

// row58-system-existing-zhuyin: MATCH.
#[test]
fn row58_system_existing_zhuyin() {
    run_j(
        "row58_ranking::row58_system_existing_zhuyin",
        Expect::RESULT,
        |k| b::case(k, "row58-system-existing-zhuyin"),
        raw!(M(&[
            ("added", B(true)),
            ("imported_count", L(&[B(true), I(44)])),
            ("logs", L(&[])),
            ("tokens_after", L(&[I(50353049)])),
            ("tokens_before", L(&[I(50353049)]))
        ])),
        None,
    );
}

// row58-system-import-zhuyin-1: MATCH.
#[test]
fn row58_system_import_zhuyin_1() {
    run_j(
        "row58_ranking::row58_system_import_zhuyin_1",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-zhuyin-1"),
        raw!(M(&[
            ("added", B(true)),
            ("imported_count", L(&[B(true), I(99)])),
            ("logs", L(&[])),
            ("tokens_after", L(&[I(16870555)])),
            ("tokens_before", L(&[]))
        ])),
        None,
    );
}

// row58-system-import-zhuyin-2: MATCH.
#[test]
fn row58_system_import_zhuyin_2() {
    run_j(
        "row58_ranking::row58_system_import_zhuyin_2",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-zhuyin-2"),
        raw!(M(&[
            ("added", B(true)),
            ("imported_count", L(&[B(true), I(99)])),
            ("logs", L(&[])),
            ("tokens_after", L(&[I(33570045)])),
            ("tokens_before", L(&[]))
        ])),
        None,
    );
}

// row58-system-import-zhuyin-3: MATCH.
#[test]
fn row58_system_import_zhuyin_3() {
    run_j(
        "row58_ranking::row58_system_import_zhuyin_3",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-zhuyin-3"),
        raw!(M(&[
            ("added", B(true)),
            ("imported_count", L(&[B(true), I(99)])),
            ("logs", L(&[])),
            ("tokens_after", L(&[I(50359794)])),
            ("tokens_before", L(&[]))
        ])),
        None,
    );
}

// row58-system-import-zhuyin-4: MATCH.
#[test]
fn row58_system_import_zhuyin_4() {
    run_j(
        "row58_ranking::row58_system_import_zhuyin_4",
        Expect::RESULT,
        |k| b::case(k, "row58-system-import-zhuyin-4"),
        raw!(M(&[
            ("added", B(true)),
            ("imported_count", L(&[B(true), I(99)])),
            ("logs", L(&[])),
            ("tokens_after", L(&[I(67109866)])),
            ("tokens_before", L(&[]))
        ])),
        None,
    );
}
