//! Layered configuration: typed settings read by key.
//!
//! Layers run defaults → system → user, and [`merge`] is a pure function over
//! them. [`Config::default`] is the pinned upstream default for every key of
//! the `com.github.libpinyin.ibus-libpinyin.libpinyin` schema frozen in
//! `docs/findings/upstream-schema.md` — which is also the parity
//! configuration, so Stage 1 runs under the sane default rather than a special
//! one.
//!
//! A GSettings, registry or plist backend is a shell concern and lives
//! outside. What a shell has to produce is a [`ConfigLayer`], which it can
//! also read from a file in the format [`ConfigLayer::parse`] documents.
//!
//! # Two halves of one seam
//!
//! This module and [`crate::StoragePaths`] are the whole of "platform facts
//! arrive as data", and they only work as a pair — keep them co-documented,
//! and change neither without looking at the other:
//!
//! - **Configuration is injected, not discovered.** Nothing here opens a
//!   settings store or reads an environment variable to find one, and
//!   [`Config::default`] equals the captured upstream defaults key for key, so
//!   the sane default *is* the parity configuration.
//! - **Locations are injected, not discovered.** `StoragePaths` never calls
//!   `XDG_DATA_HOME`, `%APPDATA%` or `NSSearchPathForDirectoriesInDomains`,
//!   and depends on no directories crate.
//!
//! [`crate::StoragePaths`] states its own half in its module documentation;
//! this is the tie between them. Together they are what lets
//! `.kiro/steering/structure.md`'s portability seam hold: a session reads no
//! environment, discovers no path, and needs no `cfg(target_os)`. Adding
//! discovery to *either* half breaks the claim for both, so a change to one
//! wants a look at the other.

use core::fmt;
use std::collections::{BTreeMap, btree_map};

/// One configuration value.
///
/// The four variants mirror the GSettings types the frozen upstream schema in
/// `docs/findings/upstream-schema.md` actually uses: `b`, `i`, `x` and `s`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum ConfigValue {
    /// GSettings `b`.
    Bool(bool),
    /// GSettings `i`.
    Int(i32),
    /// GSettings `x`.
    Int64(i64),
    /// GSettings `s`.
    Text(String),
}

impl ConfigValue {
    /// The value as a boolean, or `None` for another type.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// The value as a 32-bit integer, or `None` for another type.
    #[must_use]
    pub const fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(value) => Some(*value),
            _ => None,
        }
    }

    /// The value as a 64-bit integer, or `None` for another type.
    #[must_use]
    pub const fn as_int64(&self) -> Option<i64> {
        match self {
            Self::Int64(value) => Some(*value),
            _ => None,
        }
    }

    /// The value as text, or `None` for another type.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            _ => None,
        }
    }

    /// The GSettings type letter this value corresponds to.
    #[must_use]
    pub const fn type_letter(&self) -> char {
        // Exhaustive on purpose, with no catch-all arm: a new ConfigValue
        // variant must update this. The enum is #[non_exhaustive] for
        // downstream crates, which does not relax matching inside this one, so
        // adding a variant breaks this match at compile time — which is the
        // point. A `_ =>` arm here would let a new type silently report
        // someone else's letter, and `merge` rejects a layer by comparing
        // exactly these letters.
        match self {
            Self::Bool(_) => 'b',
            Self::Int(_) => 'i',
            Self::Int64(_) => 'x',
            Self::Text(_) => 's',
        }
    }
}

/// A default value as it appears in the frozen upstream schema.
///
/// A separate type from [`ConfigValue`] because a `static` cannot hold an
/// owned `String`; [`Config::default`] turns each seed into a value.
#[derive(Clone, Copy, Debug)]
enum Seed {
    Bool(bool),
    Int(i32),
    Int64(i64),
    Text(&'static str),
}

impl Seed {
    fn value(self) -> ConfigValue {
        match self {
            Self::Bool(value) => ConfigValue::Bool(value),
            Self::Int(value) => ConfigValue::Int(value),
            Self::Int64(value) => ConfigValue::Int64(value),
            Self::Text(value) => ConfigValue::Text(value.to_owned()),
        }
    }
}

/// Number of keys [`Config::default`] carries.
///
/// The whole `com.github.libpinyin.ibus-libpinyin.libpinyin` schema. The table
/// behind it is private, so this is the public handle on its size.
pub const UPSTREAM_DEFAULT_COUNT: usize = 69;

/// The pinned upstream defaults, verbatim.
///
/// Every key of the `com.github.libpinyin.ibus-libpinyin.libpinyin` schema
/// frozen in `docs/findings/upstream-schema.md`, in that document's order.
/// `tests/upstream_defaults.rs` re-reads the schema and asserts this table
/// matches it key for key, so the two cannot drift apart.
///
/// The sibling `libbopomofo` schema is deliberately absent: Stage 1 is full
/// pinyin, and Zhuyin is out of scope.
static UPSTREAM_DEFAULTS: [(&str, Seed); UPSTREAM_DEFAULT_COUNT] = [
    ("auto-commit", Seed::Bool(false)),
    ("comma-period-page", Seed::Bool(false)),
    ("square-bracket-page", Seed::Bool(false)),
    ("correct-pinyin", Seed::Bool(true)),
    ("correct-pinyin-gn-ng", Seed::Bool(true)),
    ("correct-pinyin-iou-iu", Seed::Bool(true)),
    ("correct-pinyin-mg-ng", Seed::Bool(true)),
    ("correct-pinyin-on-ong", Seed::Bool(true)),
    ("correct-pinyin-v-u", Seed::Bool(true)),
    ("correct-pinyin-uei-ui", Seed::Bool(true)),
    ("correct-pinyin-uen-un", Seed::Bool(true)),
    ("correct-pinyin-ue-ve", Seed::Bool(true)),
    ("dictionaries", Seed::Text("")),
    ("double-pinyin", Seed::Bool(false)),
    ("double-pinyin-schema", Seed::Int(0)),
    ("double-pinyin-show-raw", Seed::Bool(false)),
    ("dynamic-adjust", Seed::Bool(true)),
    ("fuzzy-pinyin", Seed::Bool(false)),
    ("fuzzy-pinyin-an-ang", Seed::Bool(true)),
    ("fuzzy-pinyin-en-eng", Seed::Bool(true)),
    ("fuzzy-pinyin-in-ing", Seed::Bool(true)),
    ("fuzzy-pinyin-f-h", Seed::Bool(true)),
    ("fuzzy-pinyin-g-k", Seed::Bool(false)),
    ("fuzzy-pinyin-l-n", Seed::Bool(true)),
    ("fuzzy-pinyin-l-r", Seed::Bool(false)),
    ("fuzzy-pinyin-c-ch", Seed::Bool(true)),
    ("fuzzy-pinyin-s-sh", Seed::Bool(true)),
    ("fuzzy-pinyin-z-zh", Seed::Bool(true)),
    ("incomplete-pinyin", Seed::Bool(true)),
    ("init-chinese", Seed::Bool(true)),
    ("init-full", Seed::Bool(false)),
    ("init-full-punct", Seed::Bool(true)),
    ("init-simplified-chinese", Seed::Bool(true)),
    ("main-switch", Seed::Text("<Shift>")),
    ("letter-switch", Seed::Text("")),
    ("punct-switch", Seed::Text("<Control>period")),
    ("both-switch", Seed::Text("")),
    ("trad-switch", Seed::Text("<Control><Shift>f")),
    ("lookup-table-orientation", Seed::Int(0)),
    ("lookup-table-page-size", Seed::Int(5)),
    ("display-style", Seed::Int(0)),
    ("minus-equal-page", Seed::Bool(true)),
    ("remember-every-input", Seed::Bool(false)),
    ("shift-select-candidate", Seed::Bool(false)),
    ("sort-candidate-option", Seed::Int(1)),
    ("import-dictionary", Seed::Text("")),
    ("export-dictionary", Seed::Text("")),
    ("clear-user-data", Seed::Text("")),
    ("lua-converter", Seed::Text("")),
    ("opencc-config", Seed::Text("s2tw.json")),
    ("emoji-candidate", Seed::Bool(true)),
    ("english-candidate", Seed::Bool(true)),
    ("suggestion-candidate", Seed::Bool(false)),
    ("network-dictionary-start-timestamp", Seed::Int64(0)),
    ("network-dictionary-end-timestamp", Seed::Int64(0)),
    ("lua-extension", Seed::Bool(true)),
    ("english-input-mode", Seed::Bool(true)),
    ("table-input-mode", Seed::Bool(true)),
    ("use-custom-table", Seed::Bool(false)),
    ("import-custom-table", Seed::Text("")),
    ("export-custom-table", Seed::Text("")),
    ("clear-custom-table", Seed::Text("")),
    ("enable-cloud-input", Seed::Bool(false)),
    ("cloud-input-source", Seed::Int(0)),
    ("cloud-candidates-number", Seed::Int(1)),
    ("cloud-request-delay-time", Seed::Int(600)),
    ("export-user-phrase", Seed::Bool(true)),
    ("export-bigram-phrase", Seed::Bool(true)),
    ("keyboard-layout", Seed::Text("default")),
];

/// Anything a session can read settings from.
///
/// Object-safe on purpose: a session takes `&dyn ConfigSource`, so a shell can
/// pass a layered `Config`, a file-backed test source or its own adapter
/// without the engine growing a type parameter. The trait grows only by
/// methods with default implementations.
/// A source of truth for session settings, exposed as optional typed
/// reads: each `get_*` method answers `None` when the key is absent or
/// holds a value of another type. No fallbacks live here — substituting a
/// default is the caller's decision, and [`Config::default`] is the
/// simplest source, a map pre-filled with the pinned upstream values.
///
/// ```
/// use oxpinyin_engine::{Config, ConfigSource, ConfigValue};
///
/// let mut config = Config::default();
/// assert_eq!(config.get_int("lookup-table-page-size"), Some(5));
///
/// config.set("lookup-table-page-size", ConfigValue::Int(9));
/// assert_eq!(config.get_int("lookup-table-page-size"), Some(9));
///
/// // A missing key reads as `None`, not as a fabricated default.
/// assert_eq!(config.get_int("no-such-key"), None);
/// ```
///
pub trait ConfigSource {
    /// The value stored for `key`, or `None` when the source does not carry
    /// it.
    fn get(&self, key: &str) -> Option<&ConfigValue>;

    /// Reads `key` as a boolean.
    fn get_bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(ConfigValue::as_bool)
    }

    /// Reads `key` as a 32-bit integer.
    fn get_int(&self, key: &str) -> Option<i32> {
        self.get(key).and_then(ConfigValue::as_int)
    }

    /// Reads `key` as a 64-bit integer.
    fn get_int64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(ConfigValue::as_int64)
    }

    /// Reads `key` as text.
    fn get_text(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(ConfigValue::as_text)
    }
}

/// A source that carries nothing.
///
/// Every session setting falls back to its documented default, which is what
/// a caller wants when the setting under test is not configuration.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct EmptyConfigSource;

impl ConfigSource for EmptyConfigSource {
    fn get(&self, _key: &str) -> Option<&ConfigValue> {
        None
    }
}

/// A complete settings map.
///
/// [`Config::default`] is the pinned upstream default for every key, which is
/// also the parity configuration: Stage 1 runs under it unchanged.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    values: BTreeMap<String, ConfigValue>,
}

impl Config {
    /// Builds a configuration from an explicit map.
    #[must_use]
    pub const fn from_map(values: BTreeMap<String, ConfigValue>) -> Self {
        Self { values }
    }

    /// Number of keys carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether no key is carried.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Every key and value, in ascending key order.
    pub fn iter(&self) -> btree_map::Iter<'_, String, ConfigValue> {
        self.values.iter()
    }

    /// Sets `key`, returning what it held.
    pub fn set(&mut self, key: impl Into<String>, value: ConfigValue) -> Option<ConfigValue> {
        self.values.insert(key.into(), value)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            values: UPSTREAM_DEFAULTS
                .iter()
                .map(|(key, seed)| ((*key).to_owned(), seed.value()))
                .collect(),
        }
    }
}

impl ConfigSource for Config {
    fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values.get(key)
    }
}

impl<'a> IntoIterator for &'a Config {
    type IntoIter = btree_map::Iter<'a, String, ConfigValue>;
    type Item = (&'a String, &'a ConfigValue);

    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}

/// One overlay applied over the defaults.
///
/// Named so a rejected value can say which layer supplied it. Layers are
/// applied in order, so a later one wins.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ConfigLayer {
    name: String,
    values: BTreeMap<String, ConfigValue>,
}

impl ConfigLayer {
    /// Builds an empty layer.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: BTreeMap::new(),
        }
    }

    /// Adds a setting, builder style.
    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: ConfigValue) -> Self {
        self.values.insert(key.into(), value);
        self
    }

    /// Reads a layer from the line-oriented overlay format.
    ///
    /// One record per line, TAB-separated `key=value` fields, with `key`,
    /// `type` and `value` required. A blank line and a line starting with `#`
    /// are comments. `type` is the GSettings letter: `b`, `i`, `x` or `s`.
    ///
    /// ```text
    /// key=lookup-table-page-size <TAB> type=i <TAB> value=9
    /// ```
    ///
    /// This is the file backend the replay harness uses for test scenarios,
    /// and the shape a system drop-in takes on a platform without GSettings.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for a record that is missing a field, names an
    /// unknown type letter, or carries a value the letter cannot hold.
    pub fn parse(name: impl Into<String>, text: &str) -> Result<Self, ConfigError> {
        let name = name.into();
        let mut values = BTreeMap::new();

        for (raw, line) in text.lines().zip(1..) {
            let trimmed = raw.trim_end_matches(['\r', ' ']);
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let fields: Vec<(&str, &str)> = trimmed
                .split('\t')
                .filter_map(|field| field.split_once('='))
                .collect();
            let field = |wanted: &'static str| {
                fields
                    .iter()
                    .find(|(key, _)| *key == wanted)
                    .map(|(_, value)| *value)
                    .ok_or_else(|| ConfigError::MissingField {
                        layer: name.clone(),
                        line,
                        field: wanted,
                    })
            };

            let key = field("key")?;
            let letter = field("type")?;
            let text = field("value")?;
            values.insert(key.to_owned(), parse_value(&name, line, letter, text)?);
        }

        Ok(Self { name, values })
    }

    /// The layer's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of settings carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Whether the layer carries nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl ConfigSource for ConfigLayer {
    fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values.get(key)
    }
}

fn parse_value(
    layer: &str,
    line: usize,
    letter: &str,
    text: &str,
) -> Result<ConfigValue, ConfigError> {
    let invalid = || ConfigError::InvalidValue {
        layer: layer.to_owned(),
        line,
        value: text.to_owned(),
    };

    match letter {
        "b" => match text {
            "true" => Ok(ConfigValue::Bool(true)),
            "false" => Ok(ConfigValue::Bool(false)),
            _ => Err(invalid()),
        },
        "i" => text.parse().map(ConfigValue::Int).map_err(|_| invalid()),
        "x" => text.parse().map(ConfigValue::Int64).map_err(|_| invalid()),
        "s" => Ok(ConfigValue::Text(text.to_owned())),
        _ => Err(ConfigError::UnknownType {
            layer: layer.to_owned(),
            line,
            letter: letter.to_owned(),
        }),
    }
}

/// Merges `layers` over `base`, in order.
///
/// Pure: no I/O, no environment, no clock. The last layer to set a key wins.
///
/// A layer may introduce a key `base` does not have — a newer schema on an
/// older engine must not be a hard failure — but it may not change a known
/// key's type, because a shell reading that key with the wrong accessor would
/// silently get `None` instead of a diagnosis.
///
/// # Errors
///
/// Returns [`ConfigError::TypeMismatch`] when a layer supplies a known key
/// with a different type.
pub fn merge(base: &Config, layers: &[ConfigLayer]) -> Result<Config, ConfigError> {
    let mut merged = base.clone();

    for layer in layers {
        for (key, value) in &layer.values {
            if let Some(existing) = base.get(key)
                && existing.type_letter() != value.type_letter()
            {
                return Err(ConfigError::TypeMismatch {
                    layer: layer.name.clone(),
                    key: key.clone(),
                    expected: existing.type_letter(),
                    found: value.type_letter(),
                });
            }
            merged.values.insert(key.clone(), value.clone());
        }
    }

    Ok(merged)
}

/// Why a configuration could not be read or merged.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ConfigError {
    /// An overlay record is missing a required field.
    MissingField {
        /// Layer name.
        layer: String,
        /// One-based line number.
        line: usize,
        /// Field name.
        field: &'static str,
    },
    /// An overlay record names a type letter that is not `b`, `i`, `x` or `s`.
    UnknownType {
        /// Layer name.
        layer: String,
        /// One-based line number.
        line: usize,
        /// The offending letter.
        letter: String,
    },
    /// An overlay value does not fit the type its record claims.
    InvalidValue {
        /// Layer name.
        layer: String,
        /// One-based line number.
        line: usize,
        /// The offending text.
        value: String,
    },
    /// A layer changed a known key's type.
    TypeMismatch {
        /// Layer name.
        layer: String,
        /// The key.
        key: String,
        /// Type letter the base carries.
        expected: char,
        /// Type letter the layer supplied.
        found: char,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { layer, line, field } => {
                write!(formatter, "{layer}:{line}: missing field {field}")
            }
            Self::UnknownType {
                layer,
                line,
                letter,
            } => write!(formatter, "{layer}:{line}: unknown type {letter:?}"),
            Self::InvalidValue { layer, line, value } => {
                write!(formatter, "{layer}:{line}: invalid value {value:?}")
            }
            Self::TypeMismatch {
                layer,
                key,
                expected,
                found,
            } => write!(
                formatter,
                "{layer}: key {key:?} is type {expected}, layer supplied {found}"
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        Config, ConfigError, ConfigLayer, ConfigSource, ConfigValue, EmptyConfigSource, merge,
    };

    struct MapSource(BTreeMap<String, ConfigValue>);

    impl ConfigSource for MapSource {
        fn get(&self, key: &str) -> Option<&ConfigValue> {
            self.0.get(key)
        }
    }

    #[test]
    fn typed_reads_reject_a_mismatched_type() {
        let mut values = BTreeMap::new();
        values.insert("incomplete-pinyin".to_owned(), ConfigValue::Bool(true));
        values.insert("lookup-table-page-size".to_owned(), ConfigValue::Int(5));
        values.insert(
            "network-dictionary-start-timestamp".to_owned(),
            ConfigValue::Int64(0),
        );
        values.insert(
            "opencc-config".to_owned(),
            ConfigValue::Text("s2tw.json".to_owned()),
        );
        let source = MapSource(values);

        assert_eq!(source.get_bool("incomplete-pinyin"), Some(true));
        assert_eq!(source.get_int("lookup-table-page-size"), Some(5));
        assert_eq!(
            source.get_int64("network-dictionary-start-timestamp"),
            Some(0)
        );
        assert_eq!(source.get_text("opencc-config"), Some("s2tw.json"));

        assert_eq!(source.get_int("incomplete-pinyin"), None);
        assert_eq!(source.get_bool("lookup-table-page-size"), None);
        assert_eq!(source.get_text("absent"), None);
    }

    #[test]
    fn type_letters_match_the_upstream_schema_spelling() {
        assert_eq!(ConfigValue::Bool(false).type_letter(), 'b');
        assert_eq!(ConfigValue::Int(0).type_letter(), 'i');
        assert_eq!(ConfigValue::Int64(0).type_letter(), 'x');
        assert_eq!(ConfigValue::Text(String::new()).type_letter(), 's');
    }

    #[test]
    fn the_empty_source_carries_nothing() {
        let source = EmptyConfigSource;
        assert!(source.get("incomplete-pinyin").is_none());
        assert_eq!(source.get_bool("incomplete-pinyin"), None);
    }

    const SYSTEM: &str = include_str!("../../../fixtures/w4/config-system.txt");
    const USER: &str = include_str!("../../../fixtures/w4/config-user.txt");

    fn layers() -> Vec<ConfigLayer> {
        vec![
            ConfigLayer::parse("system", SYSTEM).expect("the committed fixture parses"),
            ConfigLayer::parse("user", USER).expect("the committed fixture parses"),
        ]
    }

    #[test]
    fn the_committed_layers_parse_every_type() {
        let layers = layers();
        let system = &layers[0];
        assert_eq!(system.name(), "system");
        assert_eq!(system.len(), 3);
        assert!(!system.is_empty());
        assert_eq!(system.get_int("lookup-table-page-size"), Some(9));
        assert_eq!(system.get_bool("fuzzy-pinyin"), Some(true));
        assert_eq!(system.get_text("opencc-config"), Some("s2t.json"));

        let user = &layers[1];
        assert_eq!(user.len(), 4);
        assert_eq!(
            user.get_int64("network-dictionary-start-timestamp"),
            Some(1_754_697_600)
        );
        assert!(ConfigLayer::new("empty").is_empty());
    }

    #[test]
    fn later_layers_win_and_untouched_keys_keep_the_default() {
        let merged = merge(&Config::default(), &layers()).expect("the fixtures are well typed");

        // user over system over defaults
        assert_eq!(merged.get_int("lookup-table-page-size"), Some(7));
        // system only
        assert_eq!(merged.get_bool("fuzzy-pinyin"), Some(true));
        assert_eq!(merged.get_text("opencc-config"), Some("s2t.json"));
        // user only
        assert_eq!(merged.get_bool("incomplete-pinyin"), Some(false));
        // neither layer
        assert_eq!(merged.get_bool("dynamic-adjust"), Some(true));
        assert_eq!(merged.get_text("keyboard-layout"), Some("default"));
    }

    #[test]
    fn a_key_the_engine_does_not_know_is_preserved() {
        let merged = merge(&Config::default(), &layers()).expect("the fixtures are well typed");
        assert_eq!(
            merged.get_text("future-key-this-engine-does-not-know"),
            Some("kept"),
            "a newer schema on an older engine must not be a hard failure"
        );
        assert_eq!(merged.len(), Config::default().len() + 1);
    }

    #[test]
    fn merging_is_pure_and_order_dependent() {
        let base = Config::default();
        let ordered = layers();
        let mut reversed = ordered.clone();
        reversed.reverse();

        assert_eq!(
            merge(&base, &ordered).expect("well typed"),
            merge(&base, &ordered).expect("well typed"),
            "merging the same inputs gives the same output"
        );
        assert_eq!(
            merge(&base, &[]).expect("no layers"),
            base,
            "no layers leaves the base untouched"
        );
        assert_eq!(base, Config::default(), "the base is not mutated");
        assert_eq!(
            merge(&base, &reversed)
                .expect("well typed")
                .get_int("lookup-table-page-size"),
            Some(9),
            "reversing the layers reverses which one wins"
        );
    }

    #[test]
    fn changing_a_known_key_type_is_rejected() {
        let layer = ConfigLayer::new("user").with(
            "lookup-table-page-size",
            ConfigValue::Text("nine".to_owned()),
        );
        assert_eq!(
            merge(&Config::default(), &[layer]).expect_err("the type changed"),
            ConfigError::TypeMismatch {
                layer: "user".to_owned(),
                key: "lookup-table-page-size".to_owned(),
                expected: 'i',
                found: 's',
            }
        );
    }

    #[test]
    fn malformed_overlay_records_are_reported_with_their_line() {
        assert_eq!(
            ConfigLayer::parse("user", "key=a\ttype=b").expect_err("no value"),
            ConfigError::MissingField {
                layer: "user".to_owned(),
                line: 1,
                field: "value",
            }
        );
        assert_eq!(
            ConfigLayer::parse("user", "# comment\nkey=a\ttype=q\tvalue=1")
                .expect_err("unknown type"),
            ConfigError::UnknownType {
                layer: "user".to_owned(),
                line: 2,
                letter: "q".to_owned(),
            }
        );
        assert_eq!(
            ConfigLayer::parse("user", "key=a\ttype=b\tvalue=yes").expect_err("not a boolean"),
            ConfigError::InvalidValue {
                layer: "user".to_owned(),
                line: 1,
                value: "yes".to_owned(),
            }
        );
        assert_eq!(
            ConfigLayer::parse("user", "key=a\ttype=i\tvalue=1.5").expect_err("not an int"),
            ConfigError::InvalidValue {
                layer: "user".to_owned(),
                line: 1,
                value: "1.5".to_owned(),
            }
        );

        for error in [
            ConfigError::MissingField {
                layer: "user".to_owned(),
                line: 1,
                field: "value",
            },
            ConfigError::UnknownType {
                layer: "user".to_owned(),
                line: 2,
                letter: "q".to_owned(),
            },
            ConfigError::InvalidValue {
                layer: "user".to_owned(),
                line: 3,
                value: "yes".to_owned(),
            },
            ConfigError::TypeMismatch {
                layer: "user".to_owned(),
                key: "a".to_owned(),
                expected: 'i',
                found: 's',
            },
        ] {
            assert!(error.to_string().starts_with("user"), "{error}");
        }
    }

    #[test]
    fn an_empty_text_value_stays_empty_rather_than_absent() {
        let layer = ConfigLayer::parse("user", "key=dictionaries\ttype=s\tvalue=")
            .expect("an empty string is a value");
        assert_eq!(layer.get_text("dictionaries"), Some(""));

        let merged = merge(&Config::default(), &[layer]).expect("well typed");
        assert_eq!(merged.get_text("dictionaries"), Some(""));
    }
}
