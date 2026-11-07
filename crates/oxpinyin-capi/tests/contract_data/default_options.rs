//! The default and explicit option words of the full-pinyin parsers (#532). Generated from the frozen pin observations.

use crate::bodies::default_options as b;
use crate::support::{Expect, NO_LOGS, run};
use std::collections::BTreeMap;

// default-option-parse: MATCH.
// pinyin_init stores USE_TONE (pinyin.cpp:329); the batch parse reads it (:1497-1527).
#[test]
fn default_option_parse() {
    run(
        "default_options::default_option_parse",
        Expect::RESULT,
        b::default_parse,
        BTreeMap::from([
            ("n".into(), 0),
            ("ni2hao".into(), 6),
            ("ni3".into(), 3),
            ("nihao".into(), 5),
            ("su3cl3".into(), 3),
            ("zzz".into(), 0),
        ]),
        Some(NO_LOGS),
    );
}

// default-option-single-key: MATCH.
#[test]
fn default_option_single_key() {
    run(
        "default_options::default_option_single_key",
        Expect::RESULT,
        b::default_single_key,
        BTreeMap::from([
            ("a1".into(), (true, 4224)),
            ("n".into(), (false, 0)),
            ("ni".into(), (true, 43)),
            ("ni3".into(), (true, 12331)),
            ("zhang".into(), (true, 535)),
            ("zzz".into(), (false, 0)),
        ]),
        Some(NO_LOGS),
    );
}

// explicit-use-tone-parse: MATCH.
// Kept: oxpinyin's set_options writes the incomplete-pinyin key init leaves at its seed.
#[test]
fn explicit_use_tone_parse() {
    run(
        "default_options::explicit_use_tone_parse",
        Expect::RESULT,
        |k| b::explicit_parse(k, 1 << 5),
        BTreeMap::from([
            ("n".into(), 0),
            ("ni2hao".into(), 6),
            ("nihao".into(), 5),
            ("su3cl3".into(), 3),
            ("zzz".into(), 0),
        ]),
        Some(NO_LOGS),
    );
}

// explicit-incomplete-parse: MATCH.
#[test]
fn explicit_incomplete_parse() {
    run(
        "default_options::explicit_incomplete_parse",
        Expect::RESULT,
        |k| b::explicit_parse(k, 1 << 3),
        BTreeMap::from([
            ("n".into(), 1),
            ("ni2hao".into(), 2),
            ("nihao".into(), 5),
            ("su3cl3".into(), 2),
            ("zzz".into(), 3),
        ]),
        Some(NO_LOGS),
    );
}
