//! Missing system DBM attachments: one system table removed per case, then init, exercise and fini (#1516). Generated from the frozen pin observations.

use crate::bodies::missing_system_dbm as b;
use crate::support::{Expect, run_j};

// missing-system-dbm/zhuyin-bigram-operations: MATCH.
#[test]
fn missing_system_dbm_zhuyin_bigram_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_bigram_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-bigram-operations"),
        raw!(include!("fixtures/missing_system_dbm/bab69e01e8211e47.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-bigram-overlay: MATCH.
#[test]
fn missing_system_dbm_zhuyin_bigram_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_bigram_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-bigram-overlay"),
        raw!(include!("fixtures/missing_system_dbm/141327c84aaf0744.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-intact: MATCH.
#[test]
fn missing_system_dbm_zhuyin_intact() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_intact",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-intact"),
        raw!(include!("fixtures/missing_system_dbm/9d8efbb5c475054f.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-phrase-lifecycle: MATCH.
#[test]
fn missing_system_dbm_zhuyin_phrase_lifecycle() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_phrase_lifecycle",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-phrase-lifecycle"),
        raw!(M(&[("fini", B(true)), ("init", B(true)), ("logs", L(&[]))])),
        None,
    );
}

// missing-system-dbm/zhuyin-phrase-operations: MATCH.
#[test]
fn missing_system_dbm_zhuyin_phrase_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_phrase_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-phrase-operations"),
        raw!(include!("fixtures/missing_system_dbm/f7fee28f74feb899.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-phrase-overlay: MATCH.
#[test]
fn missing_system_dbm_zhuyin_phrase_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_phrase_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-phrase-overlay"),
        raw!(include!("fixtures/missing_system_dbm/a5c40ca8b605b405.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-pinyin-operations: MATCH.
#[test]
fn missing_system_dbm_zhuyin_pinyin_operations() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_pinyin_operations",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-pinyin-operations"),
        raw!(include!("fixtures/missing_system_dbm/6f8e05d03290d8d6.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-pinyin-overlay: MATCH.
#[test]
fn missing_system_dbm_zhuyin_pinyin_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_pinyin_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-pinyin-overlay"),
        raw!(include!("fixtures/missing_system_dbm/195ace75d2c63234.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-pinyin-unloaded: MATCH.
#[test]
fn missing_system_dbm_zhuyin_pinyin_unloaded() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_pinyin_unloaded",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-pinyin-unloaded"),
        raw!(include!("fixtures/missing_system_dbm/4c776bfa47d61145.rs")),
        None,
    );
}

// missing-system-dbm/zhuyin-punctuation-overlay: MATCH.
#[test]
fn missing_system_dbm_zhuyin_punctuation_overlay() {
    run_j(
        "missing_system_dbm::missing_system_dbm_zhuyin_punctuation_overlay",
        Expect::stderr(b""),
        |k| b::case(k, "missing-system-dbm/zhuyin-punctuation-overlay"),
        raw!(include!("fixtures/missing_system_dbm/c7dfc46174f38db3.rs")),
        None,
    );
}
