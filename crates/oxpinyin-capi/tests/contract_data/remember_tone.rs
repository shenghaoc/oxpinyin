//! Remembered toned readings and the zhuyin facade's missing API. Generated from the frozen pin observations.

use crate::bodies::remember_tone::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// remember-tones-pinyin: MATCH.
#[test]
fn remember_tones_pinyin() {
    run(
        "remember_tone::remember_tones_pinyin",
        Expect::RESULT,
        b::remember_tones,
        Tones {
            remember_api: true,
            export_api: true,
            add: true,
            parse: 14,
            guess: true,
            remember: true,
            export: vec![(true, "你好你好".into(), "ni3'hao3'ni3'hao3".into(), 100003)],
        },
        Some(NO_LOGS),
    );
}

// remember-cleared-tone-pinyin: MATCH.
#[test]
fn remember_cleared_tone_pinyin() {
    run(
        "remember_tone::remember_cleared_tone_pinyin",
        Expect::RESULT,
        |k| b::remember_review(k, false),
        Review {
            parse: 7,
            guess: true,
            clear_tone: Some(true),
            remember: true,
            export: vec![(true, "你好".into(), "ni3'hao3".into(), 3)],
        },
        Some(NO_LOGS),
    );
}

// remember-double-tone-pinyin: MATCH.
#[test]
fn remember_double_tone_pinyin() {
    run(
        "remember_tone::remember_double_tone_pinyin",
        Expect::RESULT,
        |k| b::remember_review(k, true),
        Review {
            parse: 3,
            guess: true,
            clear_tone: None,
            remember: true,
            export: vec![(true, "你".into(), "ni3".into(), 3)],
        },
        Some(NO_LOGS),
    );
}
