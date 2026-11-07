//! A stored library total near the guint32 wrap (#802) and chained MODIFY_HEADER logs. Generated from the frozen pin observations.

use crate::bodies::stored_total_overflow as b;
use crate::support::{Expect, NO_LOGS, run};

// stored-total-overflow-below: MATCH.
// GAP - 1 is the largest delta library 1's total takes (phrase_index.cpp:170-174).
#[test]
fn stored_total_overflow_below() {
    run(
        "stored_total_overflow::stored_total_overflow_below",
        Expect::RESULT,
        |k| b::stored_total_overflow(k, false, false),
        include!("fixtures/stored_total_overflow/6cfa72aa4eb51c4c.rs"),
        Some(NO_LOGS),
    );
}

// stored-total-overflow-above-saved: MATCH.
// Refused (phrase_index.cpp:170-171) though the facade total rises (phrase_index.h:633); after the
// save compacts the totals (pinyin.cpp:1139) the same delta is taken.
#[test]
fn stored_total_overflow_above_saved() {
    run(
        "stored_total_overflow::stored_total_overflow_above_saved",
        Expect::RESULT,
        |k| b::stored_total_overflow(k, true, true),
        include!("fixtures/stored_total_overflow/3adfa794975521f0.rs"),
        Some(NO_LOGS),
    );
}

// stored-total-overflow-below-saved: MATCH.
// Kept: the accepted add leaves a changed item, which the save logs as LOG_MODIFY_RECORD
// (phrase_index.cpp:420).
#[test]
fn stored_total_overflow_below_saved() {
    run(
        "stored_total_overflow::stored_total_overflow_below_saved",
        Expect::RESULT,
        |k| b::stored_total_overflow(k, false, true),
        include!("fixtures/stored_total_overflow/f694a1bd18d5af7b.rs"),
        Some(NO_LOGS),
    );
}

// log-headers-chained-both-apply: MATCH.
// The second header's old total is the running total (phrase_index.cpp:503-512).
#[test]
fn log_headers_chained_both_apply() {
    run(
        "stored_total_overflow::log_headers_chained_both_apply",
        Expect::RESULT,
        |k| b::log_headers(k, true),
        include!("fixtures/stored_total_overflow/d30a273076ff1e82.rs"),
        Some(NO_LOGS),
    );
}

// log-headers-second-stops-merge: MATCH.
// The second header's old total is the original, so the merge stops (phrase_index.cpp:508-509).
#[test]
fn log_headers_second_stops_merge() {
    run(
        "stored_total_overflow::log_headers_second_stops_merge",
        Expect::RESULT,
        |k| b::log_headers(k, false),
        include!("fixtures/stored_total_overflow/d30a273076ff1e82.rs"),
        Some(NO_LOGS),
    );
}
