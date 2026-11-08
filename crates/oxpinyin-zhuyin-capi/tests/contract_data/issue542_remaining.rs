//! The remaining #542 C-string and lifetime cases. Generated from the frozen pin observations.

use crate::bodies::issue542_remaining::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};
use std::collections::BTreeMap;

// issue542-phrase-segment-invalid-utf8-zhuyin: POLICY-ACCEPTED.
// Policy: docs/findings/1297-segment-critical.md. The pin logs [None, 8] "... assertion
// 'num_of_chars == ucs4_len' failed" (zhuyin.cpp:965-973); oxpinyin logs nothing.
#[test]
fn issue542_phrase_segment_invalid_utf8_zhuyin() {
    run(
        "issue542_remaining::issue542_phrase_segment_invalid_utf8_zhuyin",
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

// issue542-train-no-choose-save-zhuyin: MATCH.
#[test]
fn issue542_train_no_choose_save_zhuyin() {
    run(
        "issue542_remaining::issue542_train_no_choose_save_zhuyin",
        Expect::RESULT,
        b::train_no_choose_save,
        include!("fixtures/issue542_remaining/ca6bee686f79e947.rs"),
        Some(NO_LOGS),
    );
}

// issue542-parse-full-one-values-zhuyin: MATCH.
#[test]
fn issue542_parse_full_one_values_zhuyin() {
    run(
        "issue542_remaining::issue542_parse_full_one_values_zhuyin",
        Expect::RESULT,
        b::parse_full_one_values,
        BTreeMap::from([("n".into(), (false, 0)), ("ni3".into(), (true, 12331))]),
        Some(NO_LOGS),
    );
}
