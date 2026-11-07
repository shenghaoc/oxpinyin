//! Duplicate-text candidates before the cursor, LONGER against NORMAL rows (#913) and addon promotion (#762). Generated from the frozen pin observations.

use crate::bodies::before_cursor_dedup as b;
use crate::support::{Expect, NO_LOGS, run};

// longer-normal-frequency-dedup: MATCH.
// NORMAL (bigram) beats the prepended LONGER (unigram) (pinyin.cpp:2130-2134).
#[test]
fn longer_normal_frequency_dedup() {
    run(
        "before_cursor_dedup::longer_normal_frequency_dedup",
        Expect::RESULT,
        |k| b::longer_listing(k, true, true),
        include!("fixtures/before_cursor_dedup/fb38a76675ef8858.rs"),
        Some(NO_LOGS),
    );
}

// longer-normal-frequency-tie: MATCH.
// A tie keeps the prepended LONGER row (pinyin.cpp:2135-2137).
#[test]
fn longer_normal_frequency_tie() {
    run(
        "before_cursor_dedup::longer_normal_frequency_tie",
        Expect::RESULT,
        |k| b::longer_listing(k, false, true),
        include!("fixtures/before_cursor_dedup/b8cc8a889700bc78.rs"),
        Some(NO_LOGS),
    );
}

// longer-normal-disabled: MATCH.
// SORT_WITHOUT_LONGER_CANDIDATE prepends no LONGER row (pinyin.cpp:2292-2293).
#[test]
fn longer_normal_disabled() {
    run(
        "before_cursor_dedup::longer_normal_disabled",
        Expect::RESULT,
        |k| b::longer_listing(k, true, false),
        include!("fixtures/before_cursor_dedup/a97bff3ac6dc090c.rs"),
        Some(NO_LOGS),
    );
}

// issue762-addon-promote-no-mask: MATCH.
// A fresh token at the range end (phrase_index.cpp:636-643).
#[test]
fn issue762_addon_promote_no_mask() {
    run(
        "before_cursor_dedup::issue762_addon_promote_no_mask",
        Expect::RESULT,
        |k| b::addon_promote(k, None),
        include!("fixtures/before_cursor_dedup/cef7a0fbc7702d6e.rs"),
        Some(NO_LOGS),
    );
}

// issue762-addon-promote-mask-highest: MATCH.
// Masking the highest token lowers the range end, so it is reused (phrase_index.cpp:677-690).
#[test]
fn issue762_addon_promote_mask_highest() {
    run(
        "before_cursor_dedup::issue762_addon_promote_mask_highest",
        Expect::RESULT,
        |k| b::addon_promote(k, Some(0x0500_0003)),
        include!("fixtures/before_cursor_dedup/9892ab163195ae52.rs"),
        Some(NO_LOGS),
    );
}

// issue762-addon-promote-mask-middle: MATCH.
// An interior zero is not stripped, so nothing is reused (phrase_index.cpp:637-640).
#[test]
fn issue762_addon_promote_mask_middle() {
    run(
        "before_cursor_dedup::issue762_addon_promote_mask_middle",
        Expect::RESULT,
        |k| b::addon_promote(k, Some(0x0500_0002)),
        include!("fixtures/before_cursor_dedup/443328fca5ff7e99.rs"),
        Some(NO_LOGS),
    );
}
