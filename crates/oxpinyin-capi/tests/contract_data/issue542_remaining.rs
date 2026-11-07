//! The remaining #542 C-string and lifetime cases. Generated from the frozen pin observations.

use crate::bodies::issue542_remaining::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};
use std::collections::BTreeMap;

// issue542-alias-prefix-borrowed: MATCH.
#[test]
fn issue542_alias_prefix_borrowed() {
    run(
        "issue542_remaining::issue542_alias_prefix_borrowed",
        Expect::RESULT,
        b::alias_prefix_borrowed,
        include!("fixtures/issue542_remaining/09b6060a2ada385a.rs"),
        Some(NO_LOGS),
    );
}

// issue542-phrase-segment-invalid-utf8-pinyin: POLICY-ACCEPTED.
// Policy: docs/findings/1297-segment-critical.md. The pin logs [None, 8] "... assertion
// 'num_of_chars == ucs4_len' failed" (pinyin.cpp:1446-1454); oxpinyin logs nothing.
#[test]
fn issue542_phrase_segment_invalid_utf8_pinyin() {
    run(
        "issue542_remaining::issue542_phrase_segment_invalid_utf8_pinyin",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        b::segment_invalid_utf8,
        Segment {
            ret: false,
            messages: vec![],
        },
        Some(NO_LOGS),
    );
}

// issue542-train-no-choose-save-pinyin: MATCH.
#[test]
fn issue542_train_no_choose_save_pinyin() {
    run(
        "issue542_remaining::issue542_train_no_choose_save_pinyin",
        Expect::RESULT,
        b::train_no_choose_save,
        include!("fixtures/issue542_remaining/65f8d82ee3d37fd4.rs"),
        Some(NO_LOGS),
    );
}

// issue542-parse-full-one-values-pinyin: MATCH.
#[test]
fn issue542_parse_full_one_values_pinyin() {
    run(
        "issue542_remaining::issue542_parse_full_one_values_pinyin",
        Expect::RESULT,
        b::parse_full_one_values,
        BTreeMap::from([("n".into(), (false, 0)), ("ni3".into(), (true, 12331))]),
        Some(NO_LOGS),
    );
}
