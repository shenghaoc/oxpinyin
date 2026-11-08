//! Missing system DBM attachments: one system table removed per case, then init, exercise and fini (#1516). Generated from the frozen pin observations.

use crate::bodies::missing_system_dbm as b;
use crate::support::{Expect, run_j};

// missing-system-dbm/pinyin-addon-phrase-operations: MATCH.
#[test]
fn missing_system_dbm_pinyin_addon_phrase_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_addon_phrase_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-addon-phrase-operations"),
        raw!(include!("fixtures/missing_system_dbm/060815d3b4fb586f.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-addon-phrase-overlay: MATCH.
#[test]
fn missing_system_dbm_pinyin_addon_phrase_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_addon_phrase_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-addon-phrase-overlay"),
        raw!(include!("fixtures/missing_system_dbm/233b1344ef7b4a3c.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-addon-pinyin-operations: MATCH.
#[test]
fn missing_system_dbm_pinyin_addon_pinyin_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_addon_pinyin_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-addon-pinyin-operations"),
        raw!(include!("fixtures/missing_system_dbm/060815d3b4fb586f.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-addon-pinyin-overlay: MATCH.
#[test]
fn missing_system_dbm_pinyin_addon_pinyin_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_addon_pinyin_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-addon-pinyin-overlay"),
        raw!(include!("fixtures/missing_system_dbm/233b1344ef7b4a3c.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-bigram-lifecycle: MATCH.
#[test]
fn missing_system_dbm_pinyin_bigram_lifecycle() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_bigram_lifecycle",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-bigram-lifecycle"),
        raw!(M(&[("fini", B(true)), ("init", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// missing-system-dbm/pinyin-bigram-operations: MATCH.
#[test]
fn missing_system_dbm_pinyin_bigram_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_bigram_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-bigram-operations"),
        raw!(include!("fixtures/missing_system_dbm/2a4694bb799539b3.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-bigram-overlay: MATCH.
#[test]
fn missing_system_dbm_pinyin_bigram_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_bigram_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-bigram-overlay"),
        raw!(include!("fixtures/missing_system_dbm/3a57d1226f11564a.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-intact: MATCH.
#[test]
fn missing_system_dbm_pinyin_intact() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_intact",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-intact"),
        raw!(include!("fixtures/missing_system_dbm/060815d3b4fb586f.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-phrase-operations: MATCH.
#[test]
fn missing_system_dbm_pinyin_phrase_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_phrase_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-phrase-operations"),
        raw!(include!("fixtures/missing_system_dbm/e8cd58dba6897330.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-phrase-overlay: MATCH.
#[test]
fn missing_system_dbm_pinyin_phrase_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_phrase_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-phrase-overlay"),
        raw!(include!("fixtures/missing_system_dbm/7529a115bc31d87a.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-pinyin-operations: MATCH.
#[test]
fn missing_system_dbm_pinyin_pinyin_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_pinyin_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-pinyin-operations"),
        raw!(include!("fixtures/missing_system_dbm/6435e87721d4a151.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-pinyin-overlay: MATCH.
#[test]
fn missing_system_dbm_pinyin_pinyin_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_pinyin_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-pinyin-overlay"),
        raw!(include!("fixtures/missing_system_dbm/3b582283b941caeb.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-pinyin-unloaded: MATCH.
#[test]
fn missing_system_dbm_pinyin_pinyin_unloaded() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_pinyin_unloaded",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-pinyin-unloaded"),
        raw!(include!("fixtures/missing_system_dbm/df6c1c7964781fad.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-punctuation-operations: MATCH.
#[test]
fn missing_system_dbm_pinyin_punctuation_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_punctuation_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-punctuation-operations"),
        raw!(include!("fixtures/missing_system_dbm/060815d3b4fb586f.rs")),
        None,
    );
}

// missing-system-dbm/pinyin-punctuation-overlay: MATCH.
#[test]
fn missing_system_dbm_pinyin_punctuation_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_pinyin_punctuation_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/pinyin-punctuation-overlay"),
        raw!(include!("fixtures/missing_system_dbm/233b1344ef7b4a3c.rs")),
        None,
    );
}
