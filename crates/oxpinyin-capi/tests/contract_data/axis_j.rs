//! Axis J: an n-best choice and the multi-key chewing keyboards. Generated from the frozen pin observations.

use crate::bodies::axis_j::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// j-choose-nbest-diff-result: MATCH.
// Choosing an NBEST_MATCH row (pinyin.cpp:2515-2521).
#[test]
fn j_choose_nbest_diff_result() {
    run(
        "axis_j::j_choose_nbest_diff_result",
        Expect::RESULT,
        b::choose_nbest,
        NbestChoice {
            ranked: true,
            rank: 0,
            chosen: 5,
        },
        Some(NO_LOGS),
    );
}

// j-chewing-multi-key-pinyin-2: MATCH.
// The keyboard and the option word: pinyin.cpp:1161-1191, :1582-1609.
#[test]
fn j_chewing_multi_key_pinyin_2() {
    run(
        "axis_j::j_chewing_multi_key_pinyin_2",
        Expect::RESULT,
        |k| b::multi_chewing(k, 2),
        MultiKey {
            scheme: 2,
            selected: true,
            parsed: 1,
            length: 1,
        },
        Some(NO_LOGS),
    );
}

// j-chewing-multi-key-pinyin-9: MATCH.
// The keyboard and the option word: pinyin.cpp:1161-1191, :1582-1609.
#[test]
fn j_chewing_multi_key_pinyin_9() {
    run(
        "axis_j::j_chewing_multi_key_pinyin_9",
        Expect::RESULT,
        |k| b::multi_chewing(k, 9),
        MultiKey {
            scheme: 9,
            selected: true,
            parsed: 0,
            length: 0,
        },
        Some(NO_LOGS),
    );
}
