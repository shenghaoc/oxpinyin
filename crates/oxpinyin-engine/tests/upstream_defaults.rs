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

use oxpinyin_engine::{
    Config, ConfigLayer, ConfigSource, ConfigValue, UPSTREAM_DEFAULT_COUNT, merge,
};

const SCHEMA: &str = include_str!("../../../docs/findings/upstream-schema.md");
const SCHEMA_ID: &str = "com.github.libpinyin.ibus-libpinyin.libpinyin";
const EMPTY_CONFIG: Config = Config::from_map(BTreeMap::new());

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

/// Turns a `GSettings` default literal into the value it denotes.
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

    // Check the typed path before any generic read materializes the map.
    assert_typed_defaults(&config, &captured);

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

    assert_typed_defaults(&config, &captured);
}

fn assert_typed_defaults(config: &Config, captured: &BTreeMap<String, ConfigValue>) {
    for (key, value) in captured {
        assert_eq!(config.get_bool(key), value.as_bool(), "bool {key:?}");
        assert_eq!(config.get_int(key), value.as_int(), "int {key:?}");
        assert_eq!(config.get_int64(key), value.as_int64(), "int64 {key:?}");
        assert_eq!(config.get_text(key), value.as_text(), "text {key:?}");
    }
    assert_eq!(config.get_bool("absent"), None);
    assert_eq!(config.get_int("absent"), None);
    assert_eq!(config.get_int64("absent"), None);
    assert_eq!(config.get_text("absent"), None);
}

#[test]
fn default_and_explicit_maps_have_the_same_value_semantics() {
    let captured = captured_defaults();
    let config = Config::default();
    let unmaterialized_clone = config.clone();
    let explicit = Config::from_map(captured.clone());

    assert_eq!(config, explicit);
    assert_eq!(explicit, unmaterialized_clone);
    assert_eq!(config.clone(), unmaterialized_clone);
    assert_eq!(
        format!("{config:?}"),
        format!("Config {{ values: {captured:?} }}")
    );

    // Keep the concrete iterator API, including reverse and exact-size use.
    let iter: std::collections::btree_map::Iter<'_, String, ConfigValue> = config.iter();
    assert_eq!(iter.len(), captured.len());
    assert!(iter.eq(captured.iter()));
    assert!(config.into_iter().rev().eq(captured.iter().rev()));
    assert!(EMPTY_CONFIG.is_empty());
    assert_eq!(EMPTY_CONFIG.len(), 0);
    assert_eq!(EMPTY_CONFIG.get_bool("incomplete-pinyin"), None);
}

#[test]
fn editing_a_default_before_or_after_materialization_matches_an_explicit_map() {
    let materialized = Config::default();
    assert_eq!(
        materialized.get("incomplete-pinyin"),
        Some(&ConfigValue::Bool(true))
    );
    for mut config in [Config::default(), materialized.clone()] {
        let untouched = config.clone();
        let mut explicit = Config::from_map(captured_defaults());
        for (key, value) in [
            ("incomplete-pinyin", ConfigValue::Bool(false)),
            ("lookup-table-page-size", ConfigValue::Int(9)),
            ("network-dictionary-start-timestamp", ConfigValue::Int64(-1)),
            ("opencc-config", ConfigValue::Text(String::new())),
            ("future-key", ConfigValue::Text("kept".to_owned())),
            // Direct set permits type changes; merge alone rejects them.
            (
                "incomplete-pinyin",
                ConfigValue::Text("changed type".to_owned()),
            ),
        ] {
            assert_eq!(config.set(key, value.clone()), explicit.set(key, value));
            assert_eq!(config, explicit);
        }
        assert_eq!(untouched, Config::default());
        assert_eq!(config.get_bool("incomplete-pinyin"), None);
        assert_eq!(config.get_text("incomplete-pinyin"), Some("changed type"));
        assert_eq!(config.get_int("lookup-table-page-size"), Some(9));
        assert_eq!(
            config.get_int64("network-dictionary-start-timestamp"),
            Some(-1)
        );
        assert_eq!(config.get_text("opencc-config"), Some(""));
        assert_eq!(config.get_text("future-key"), Some("kept"));
    }
}

#[test]
fn merging_default_and_explicit_maps_preserves_results_and_errors() {
    let default = Config::default();
    let explicit = Config::from_map(captured_defaults());
    let layers = [
        ConfigLayer::new("system")
            .with("lookup-table-page-size", ConfigValue::Int(9))
            .with("future-key", ConfigValue::Text("kept".to_owned())),
        ConfigLayer::new("user").with("lookup-table-page-size", ConfigValue::Int(7)),
    ];
    assert_eq!(merge(&default, &layers), merge(&explicit, &layers));
    let mismatched = [ConfigLayer::new("user").with(
        "lookup-table-page-size",
        ConfigValue::Text("nine".to_owned()),
    )];
    assert_eq!(merge(&default, &mismatched), merge(&explicit, &mismatched));
    assert_eq!(default, explicit);
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
