//! The full-pinyin input length cap (#540, #1501). Generated from the frozen pin observations.

use crate::bodies::input_length_cap::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};
use std::collections::BTreeMap;

// long-input-sentence: MATCH.
#[test]
fn long_input_sentence() {
    run(
        "input_length_cap::long_input_sentence",
        Expect::RESULT,
        b::long_input_sentence,
        LongInput {
            guess: true,
            parsed: 4800,
            sentence: (true, Some(2400)),
        },
        Some(NO_LOGS),
    );
}

// input-length-boundary-32767: MATCH.
// At the pin's gint16 bound both sides parse the whole input (pinyin_parser2.cpp:67-70, :355).
#[test]
fn input_length_boundary_32767() {
    run(
        "input_length_cap::input_length_boundary_32767",
        Expect::RESULT,
        |k| b::boundary(k, 32767),
        BTreeMap::from([("32767".into(), (32767, 32767))]),
        Some(NO_LOGS),
    );
}

// input-length-boundary-32768: POLICY-ACCEPTED.
// Policy: docs/findings/1501-gint16-input-cap.md. The pin stops at 32767 bytes
// (its result is {"32768": [32767, 32767]}); oxpinyin parses the whole input.
#[test]
fn input_length_boundary_32768() {
    run(
        "input_length_cap::input_length_boundary_32768",
        Expect::stderr(if cfg!(windows) {
            b"open SCRATCH\\user-X\\user.conf failed."
        } else {
            b"open SCRATCH/user-X/user.conf failed."
        }),
        |k| b::boundary(k, 32768),
        BTreeMap::from([("32768".into(), (32768, 32768))]),
        Some(NO_LOGS),
    );
}
