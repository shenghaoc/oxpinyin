//! `Config::default()` is the captured upstream default, key for key.
//!
//! The assertion is made against the frozen XML in
//! `docs/findings/upstream-schema.md` itself rather than against a
//! hand-transcribed copy of it, so the table in `config.rs` cannot drift away
//! from the document that freezes it. `.kiro/steering/structure.md` states the
//! rule this test enforces: the sane default *is* the parity configuration.
//!
//! Only the `com.github.libpinyin.ibus-libpinyin.libpinyin` schema is in
//! scope. The sibling `libbopomofo` schema is Zhuyin, which Stage 1 excludes.

use std::collections::BTreeMap;

use pinyin_engine::{Config, ConfigSource, ConfigValue, UPSTREAM_DEFAULT_COUNT};

const SCHEMA: &str = include_str!("../../../docs/findings/upstream-schema.md");
const SCHEMA_ID: &str = "com.github.libpinyin.ibus-libpinyin.libpinyin";

/// Every `name -> default` of the pinyin schema, read out of the frozen XML.
fn captured_defaults() -> BTreeMap<String, ConfigValue> {
    let start = SCHEMA
        .find(&format!("id=\"{SCHEMA_ID}\""))
        .expect("the pinyin schema is in the finding");
    let end = SCHEMA[start..]
        .find("</schema>")
        .map(|offset| start + offset)
        .expect("the schema block is closed");

    let mut defaults = BTreeMap::new();
    let mut pending: Option<(String, String)> = None;

    for line in SCHEMA[start..end].lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("<key name=\"") {
            let (name, rest) = rest.split_once('"').expect("a quoted key name");
            let letter = rest
                .split_once("type=\"")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(letter, _)| letter)
                .expect("a quoted type letter");
            pending = Some((name.to_owned(), letter.to_owned()));
        } else if let Some(rest) = line.strip_prefix("<default>") {
            let text = rest.strip_suffix("</default>").expect("a closed default");
            let (name, letter) = pending.take().expect("a default follows its key");
            defaults.insert(name, decode(&letter, &unescape(text)));
        }
    }

    defaults
}

fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Turns a GSettings default literal into the value it denotes.
fn decode(letter: &str, text: &str) -> ConfigValue {
    match letter {
        "b" => ConfigValue::Bool(text.parse().expect("a GSettings boolean")),
        "i" => ConfigValue::Int(text.parse().expect("a GSettings int32")),
        "x" => ConfigValue::Int64(text.parse().expect("a GSettings int64")),
        "s" => {
            let quoted = text
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
                .expect("a GSettings string is single-quoted");
            ConfigValue::Text(quoted.to_owned())
        }
        other => panic!("the schema uses an unexpected type letter {other:?}"),
    }
}

#[test]
fn the_schema_block_parses() {
    let captured = captured_defaults();
    assert_eq!(captured.len(), UPSTREAM_DEFAULT_COUNT);
    assert_eq!(
        captured.get("incomplete-pinyin"),
        Some(&ConfigValue::Bool(true))
    );
    assert_eq!(
        captured.get("main-switch"),
        Some(&ConfigValue::Text("<Shift>".to_owned())),
        "XML entities are decoded"
    );
    assert_eq!(
        captured.get("cloud-request-delay-time"),
        Some(&ConfigValue::Int(600)),
        "a <range> before <default> does not confuse the reader"
    );
}

#[test]
fn config_default_equals_the_captured_upstream_defaults() {
    let captured = captured_defaults();
    let config = Config::default();

    assert_eq!(
        config.len(),
        captured.len(),
        "the default carries exactly the captured key set"
    );

    for (key, value) in &captured {
        assert_eq!(
            config.get(key),
            Some(value),
            "key {key:?} does not match the frozen schema"
        );
    }

    for (key, value) in &config {
        assert_eq!(
            captured.get(key),
            Some(value),
            "key {key:?} is not in the frozen schema"
        );
    }
}

#[test]
fn the_parity_relevant_defaults_are_what_stage_one_runs_under() {
    let config = Config::default();

    // The F-A profile is IS_PINYIN | PINYIN_INCOMPLETE | USE_DIVIDED_TABLE |
    // USE_RESPLIT_TABLE with dynamic adjustment off at capture time. The two
    // settings the decoder reads directly are pinned here so a change to the
    // table is visible as a parity change rather than a silent one.
    assert_eq!(config.get_bool("incomplete-pinyin"), Some(true));
    assert_eq!(config.get_bool("fuzzy-pinyin"), Some(false));
    assert_eq!(config.get_int("lookup-table-page-size"), Some(5));
    assert_eq!(config.get_int("sort-candidate-option"), Some(1));
}
