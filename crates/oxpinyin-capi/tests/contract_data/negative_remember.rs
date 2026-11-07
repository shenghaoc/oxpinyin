//! `remember_user_input`'s signed count (#540). Generated from the frozen pin observations.

use crate::bodies::negative_remember::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};

// remember-count-default: MATCH.
// -1 is the default count 5 (pinyin.cpp:520-523).
#[test]
fn remember_count_default() {
    run(
        "negative_remember::remember_count_default",
        Expect::RESULT,
        |k| b::remember_count(k, -1),
        Remembered {
            remember: true,
            rows: vec![(Some("你好世界".into()), Some("ni'hao'shi'jie".into()), 5)],
        },
        Some(NO_LOGS),
    );
}

// remember-count-2: MATCH.
// Below the bound the count is stored as its guint32 pattern (pinyin.cpp:600-605).
#[test]
fn remember_count_2() {
    run(
        "negative_remember::remember_count_2",
        Expect::RESULT,
        |k| b::remember_count(k, -2),
        Remembered {
            remember: true,
            rows: vec![(Some("你好世界".into()), Some("ni'hao'shi'jie".into()), -2)],
        },
        Some(NO_LOGS),
    );
}

// remember-count-seven: MATCH.
#[test]
fn remember_count_seven() {
    run(
        "negative_remember::remember_count_seven",
        Expect::RESULT,
        |k| b::remember_count(k, 7),
        Remembered {
            remember: true,
            rows: vec![(Some("你好世界".into()), Some("ni'hao'shi'jie".into()), 7)],
        },
        Some(NO_LOGS),
    );
}

// remember-count-min: MATCH.
// count * 3 in guint32 is 0x80000000 at the guard of phrase_index.cpp:170.
#[test]
fn remember_count_min() {
    run(
        "negative_remember::remember_count_min",
        Expect::RESULT,
        |k| b::remember_count(k, i32::MIN),
        Remembered {
            remember: true,
            rows: vec![(
                Some("你好世界".into()),
                Some("ni'hao'shi'jie".into()),
                -2147483648,
            )],
        },
        Some(NO_LOGS),
    );
}
