//! Duplicate-text candidates before the cursor, LONGER against NORMAL rows (#913) and addon promotion (#762). Generated from the frozen pin observations.

use crate::bodies::before_cursor_dedup as b;
use crate::support::{Expect, NO_LOGS, run};

// before-cursor-frequency-dedup: MATCH.
// The later slice's greater frequency wins (zhuyin.cpp:1408-1412).
#[test]
fn before_cursor_frequency_dedup() {
    run(
        "before_cursor_dedup::before_cursor_frequency_dedup",
        Expect::RESULT,
        |k| b::before_cursor_listing(k, true),
        include!("fixtures/before_cursor_dedup/8105dc18bf5b2c0f.rs"),
        Some(NO_LOGS),
    );
}

// before-cursor-frequency-tie: MATCH.
// Equal frequencies at the > bound keep the earlier row (zhuyin.cpp:1413-1415).
#[test]
fn before_cursor_frequency_tie() {
    run(
        "before_cursor_dedup::before_cursor_frequency_tie",
        Expect::RESULT,
        |k| b::before_cursor_listing(k, false),
        include!("fixtures/before_cursor_dedup/63d018e3ca103498.rs"),
        Some(NO_LOGS),
    );
}
