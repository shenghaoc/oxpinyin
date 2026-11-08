//! Invalid UTF-8 bytes in parse and import arguments (#587). Generated from the frozen pin observations.

use crate::bodies::invalid_input_bytes::{self as b, *};
use crate::support::{Expect, NO_LOGS, run};
use std::collections::BTreeMap;

// bytes-parse-more-full: MATCH.
#[test]
fn bytes_parse_more_full() {
    run(
        "invalid_input_bytes::bytes_parse_more_full",
        Expect::RESULT,
        b::parse_more_full,
        BTreeMap::from([
            ("b'\\xffnihao'".into(), 0),
            ("b'ni\\xc0\\x80hao'".into(), 2),
            ("b'ni\\xe4\\xbd'".into(), 2),
            ("b'ni\\xffhao'".into(), 2),
            ("b'nihao\\xff'".into(), 5),
        ]),
        Some(NO_LOGS),
    );
}

// bytes-parse-more-double: MATCH.
#[test]
fn bytes_parse_more_double() {
    run(
        "invalid_input_bytes::bytes_parse_more_double",
        Expect::RESULT,
        b::parse_more_double,
        BTreeMap::from([
            ("b'\\xffnihk'".into(), 0),
            ("b'ni\\xffhk'".into(), 2),
            ("b'nihk\\xff'".into(), 4),
        ]),
        Some(NO_LOGS),
    );
}

// bytes-parse-more-chewing: MATCH.
#[test]
fn bytes_parse_more_chewing() {
    run(
        "invalid_input_bytes::bytes_parse_more_chewing",
        Expect::RESULT,
        b::parse_more_chewing,
        BTreeMap::from([
            ("b'\\xffsu3'".into(), 0),
            ("b'su3\\xff'".into(), 3),
            ("b'su\\xff3'".into(), 2),
        ]),
        Some(NO_LOGS),
    );
}

// bytes-add-phrase-pinyin: MATCH.
#[test]
fn bytes_add_phrase_pinyin() {
    run(
        "invalid_input_bytes::bytes_add_phrase_pinyin",
        Expect::RESULT,
        b::add_phrase_pinyin,
        AddPhrase {
            added: vec![true, true],
            rows: vec![
                (Some("你好".into()), Some("ni'ha".into()), 1),
                (Some("世界".into()), Some("shi'jie".into()), 1),
            ],
        },
        Some(NO_LOGS),
    );
}

// bytes-whole-argument: MATCH.
#[test]
fn bytes_whole_argument() {
    run(
        "invalid_input_bytes::bytes_whole_argument",
        Expect::RESULT,
        b::whole_argument,
        WholeArgument {
            parse_full_pinyin: (false, 0),
            lookup_tokens: false,
            remember: false,
            add_bad_phrase: false,
            parsed: 11,
        },
        Some(NO_LOGS),
    );
}
