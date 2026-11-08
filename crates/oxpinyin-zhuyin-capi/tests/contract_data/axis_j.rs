//! Axis J: an n-best choice and the multi-key chewing keyboards. Generated from the frozen pin observations.

use crate::bodies::axis_j::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// j-chewing-multi-key-zhuyin-2: MATCH.
// The keyboard and the option word: zhuyin.cpp:708-739, :1058-1070.
#[test]
fn j_chewing_multi_key_zhuyin_2() {
    run(
        "axis_j::j_chewing_multi_key_zhuyin_2",
        Expect::RESULT,
        |k| b::multi_chewing(k, 2),
        MultiKey {
            scheme: 2,
            selected: true,
            parsed: 0,
            length: 0,
        },
        Some(NO_LOGS),
    );
}

// j-chewing-multi-key-zhuyin-9: MATCH.
// The keyboard and the option word: zhuyin.cpp:708-739, :1058-1070.
#[test]
fn j_chewing_multi_key_zhuyin_9() {
    run(
        "axis_j::j_chewing_multi_key_zhuyin_9",
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
