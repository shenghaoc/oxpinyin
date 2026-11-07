//! Invalid UTF-8 bytes in parse and import arguments (#587). Generated from the frozen pin observations.

use crate::bodies::invalid_input_bytes as b;
use crate::support::{Expect, NO_LOGS, run};
use std::collections::BTreeMap;

// bytes-zhuyin-parse-more: MATCH.
#[test]
fn bytes_zhuyin_parse_more() {
    run(
        "invalid_input_bytes::bytes_zhuyin_parse_more",
        Expect::RESULT,
        b::zhuyin_parse_more,
        include!("fixtures/invalid_input_bytes/503c4ae136fa6b65.rs"),
        Some(NO_LOGS),
    );
}

// bytes-zhuyin-add-phrase: MATCH.
#[test]
fn bytes_zhuyin_add_phrase() {
    run(
        "invalid_input_bytes::bytes_zhuyin_add_phrase",
        Expect::RESULT,
        b::zhuyin_add_phrase,
        BTreeMap::from([
            ("mid-invalid".into(), false),
            ("tail-invalid".into(), false),
            ("whole".into(), true),
        ]),
        Some(NO_LOGS),
    );
}
