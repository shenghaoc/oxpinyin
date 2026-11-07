//! A facade total wrapped to zero (#540, register row 61). Generated from the frozen pin observations.

use crate::bodies::facade_total::{self as b, *};
use crate::support::{Expect, NO_LOGS, run, run_aborting};

// facade-total-zero: POLICY-ACCEPTED (pin-abort-assert-line.md).
// Reproduced abort: assert(0 < total_freq) (pinyin.cpp:1857-1859). The pin also wrote glibc's
// assertion line, which oxpinyin does not (pin-abort-assert-line.md).
#[test]
fn facade_total_zero() {
    run_aborting(
        "facade_total::facade_total_zero",
        Expect::aborts(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::zero_total,
    );
}

// facade-total-zero-ranks-nothing-mid-key-sort-3: MATCH.
// An empty mid-key column ranks nothing (pinyin.cpp:2240-2242).
#[test]
fn facade_total_zero_ranks_nothing_mid_key_sort_3() {
    run(
        "facade_total::facade_total_zero_ranks_nothing_mid_key_sort_3",
        Expect::RESULT,
        |k| b::no_rank(k, 3, 3),
        NoRank { ret: true, rows: 0 },
        Some(NO_LOGS),
    );
}

// facade-total-zero-ranks-nothing-end-sort-2: MATCH.
// At the reserved slot the search loop does not run (pinyin.cpp:2230).
#[test]
fn facade_total_zero_ranks_nothing_end_sort_2() {
    run(
        "facade_total::facade_total_zero_ranks_nothing_end_sort_2",
        Expect::RESULT,
        |k| b::no_rank(k, 5, 2),
        NoRank { ret: true, rows: 0 },
        Some(NO_LOGS),
    );
}
