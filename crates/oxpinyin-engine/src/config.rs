//! Typed configuration with the frozen upstream defaults.
//!
//! C adapters keep their configuration here and pass option words to sessions.
//! No settings store or platform path discovery lives in the engine.

use core::fmt;
use std::collections::{BTreeMap, btree_map};
use std::sync::OnceLock;

/// One configuration value.
///
/// The four variants mirror the `GSettings` types the frozen upstream schema in
/// `docs/findings/upstream-schema.md` actually uses: `b`, `i`, `x` and `s`.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ConfigValue {
    /// `GSettings` `b`.
    Bool(bool),
    /// `GSettings` `i`.
    Int(i32),
    /// `GSettings` `x`.
    Int64(i64),
    /// `GSettings` `s`.
    Text(String),
}

impl ConfigValue {
    /// The value as a boolean, or `None` for another type.
    #[must_use]
    pub const fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            Self::Int(_) | Self::Int64(_) | Self::Text(_) => None,
        }
    }

    /// The value as a 32-bit integer, or `None` for another type.
    #[must_use]
    pub const fn as_int(&self) -> Option<i32> {
        match self {
            Self::Int(value) => Some(*value),
            Self::Bool(_) | Self::Int64(_) | Self::Text(_) => None,
        }
    }

    /// The value as a 64-bit integer, or `None` for another type.
    #[must_use]
    pub const fn as_int64(&self) -> Option<i64> {
        match self {
            Self::Int64(value) => Some(*value),
            Self::Bool(_) | Self::Int(_) | Self::Text(_) => None,
        }
    }

    /// The value as text, or `None` for another type.
    #[must_use]
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text(value) => Some(value),
            Self::Bool(_) | Self::Int(_) | Self::Int64(_) => None,
        }
    }

    /// The `GSettings` type letter this value corresponds to.
    #[must_use]
    pub const fn type_letter(&self) -> char {
        // Exhaustive on purpose, with no catch-all arm: a new ConfigValue
        // variant must update this. Adding a variant breaks this match at
        // compile time. A catch-all arm would let a new type silently report
        // another type's letter.
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
/// The default configuration reads these typed values directly. Its owned
/// map is built only when a caller requests values by reference, iterates or
/// edits the configuration.
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

fn default_seed(key: &str) -> Option<Seed> {
    let index = UPSTREAM_DEFAULTS
        .binary_search_by_key(&key, |&(name, _)| name)
        .ok()?;
    UPSTREAM_DEFAULTS.get(index).map(|&(_, seed)| seed)
}

fn default_values() -> BTreeMap<String, ConfigValue> {
    UPSTREAM_DEFAULTS
        .iter()
        .map(|(key, seed)| ((*key).to_owned(), seed.value()))
        .collect()
}

/// Number of keys [`Config::default`] carries.
///
/// The whole `com.github.libpinyin.ibus-libpinyin.libpinyin` schema. The table
/// behind it is private, so this is the public handle on its size.
pub const UPSTREAM_DEFAULT_COUNT: usize = 69;

/// The pinned upstream defaults, verbatim.
///
/// Every key of the `com.github.libpinyin.ibus-libpinyin.libpinyin` schema
/// frozen in `docs/findings/upstream-schema.md`, sorted by key so typed
/// reads use binary search without building the owned map.
/// `tests/upstream_defaults.rs` re-reads the schema and asserts this table
/// matches it key for key, so the two cannot drift apart.
///
/// The sibling `libbopomofo` schema is deliberately absent: Stage 1 is full
/// pinyin, and Zhuyin is out of scope.
static UPSTREAM_DEFAULTS: [(&str, Seed); UPSTREAM_DEFAULT_COUNT] = [
    ("auto-commit", Seed::Bool(false)),
    ("both-switch", Seed::Text("")),
    ("clear-custom-table", Seed::Text("")),
    ("clear-user-data", Seed::Text("")),
    ("cloud-candidates-number", Seed::Int(1)),
    ("cloud-input-source", Seed::Int(0)),
    ("cloud-request-delay-time", Seed::Int(600)),
    ("comma-period-page", Seed::Bool(false)),
    ("correct-pinyin", Seed::Bool(true)),
    ("correct-pinyin-gn-ng", Seed::Bool(true)),
    ("correct-pinyin-iou-iu", Seed::Bool(true)),
    ("correct-pinyin-mg-ng", Seed::Bool(true)),
    ("correct-pinyin-on-ong", Seed::Bool(true)),
    ("correct-pinyin-ue-ve", Seed::Bool(true)),
    ("correct-pinyin-uei-ui", Seed::Bool(true)),
    ("correct-pinyin-uen-un", Seed::Bool(true)),
    ("correct-pinyin-v-u", Seed::Bool(true)),
    ("dictionaries", Seed::Text("")),
    ("display-style", Seed::Int(0)),
    ("double-pinyin", Seed::Bool(false)),
    ("double-pinyin-schema", Seed::Int(0)),
    ("double-pinyin-show-raw", Seed::Bool(false)),
    ("dynamic-adjust", Seed::Bool(true)),
    ("emoji-candidate", Seed::Bool(true)),
    ("enable-cloud-input", Seed::Bool(false)),
    ("english-candidate", Seed::Bool(true)),
    ("english-input-mode", Seed::Bool(true)),
    ("export-bigram-phrase", Seed::Bool(true)),
    ("export-custom-table", Seed::Text("")),
    ("export-dictionary", Seed::Text("")),
    ("export-user-phrase", Seed::Bool(true)),
    ("fuzzy-pinyin", Seed::Bool(false)),
    ("fuzzy-pinyin-an-ang", Seed::Bool(true)),
    ("fuzzy-pinyin-c-ch", Seed::Bool(true)),
    ("fuzzy-pinyin-en-eng", Seed::Bool(true)),
    ("fuzzy-pinyin-f-h", Seed::Bool(true)),
    ("fuzzy-pinyin-g-k", Seed::Bool(false)),
    ("fuzzy-pinyin-in-ing", Seed::Bool(true)),
    ("fuzzy-pinyin-l-n", Seed::Bool(true)),
    ("fuzzy-pinyin-l-r", Seed::Bool(false)),
    ("fuzzy-pinyin-s-sh", Seed::Bool(true)),
    ("fuzzy-pinyin-z-zh", Seed::Bool(true)),
    ("import-custom-table", Seed::Text("")),
    ("import-dictionary", Seed::Text("")),
    ("incomplete-pinyin", Seed::Bool(true)),
    ("init-chinese", Seed::Bool(true)),
    ("init-full", Seed::Bool(false)),
    ("init-full-punct", Seed::Bool(true)),
    ("init-simplified-chinese", Seed::Bool(true)),
    ("keyboard-layout", Seed::Text("default")),
    ("letter-switch", Seed::Text("")),
    ("lookup-table-orientation", Seed::Int(0)),
    ("lookup-table-page-size", Seed::Int(5)),
    ("lua-converter", Seed::Text("")),
    ("lua-extension", Seed::Bool(true)),
    ("main-switch", Seed::Text("<Shift>")),
    ("minus-equal-page", Seed::Bool(true)),
    ("network-dictionary-end-timestamp", Seed::Int64(0)),
    ("network-dictionary-start-timestamp", Seed::Int64(0)),
    ("opencc-config", Seed::Text("s2tw.json")),
    ("punct-switch", Seed::Text("<Control>period")),
    ("remember-every-input", Seed::Bool(false)),
    ("shift-select-candidate", Seed::Bool(false)),
    ("sort-candidate-option", Seed::Int(1)),
    ("square-bracket-page", Seed::Bool(false)),
    ("suggestion-candidate", Seed::Bool(false)),
    ("table-input-mode", Seed::Bool(true)),
    ("trad-switch", Seed::Text("<Control><Shift>f")),
    ("use-custom-table", Seed::Bool(false)),
];

/// Anything a session can read settings from.
///
/// Object-safe on purpose: a session takes `&dyn ConfigSource`, so a shell can
/// pass a `Config`, a file-backed test source or its own adapter
/// without the engine growing a type parameter. The trait grows only by
/// methods with default implementations.
/// A source of truth for session settings, exposed as optional typed
/// reads: each `get_*` method answers `None` when the key is absent or
/// holds a value of another type. No fallbacks live here — substituting a
/// default is the caller's decision, and [`Config::default`] is the
/// simplest source, carrying the pinned upstream values.
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
#[derive(Clone)]
pub struct Config {
    values: ConfigValues,
}

#[derive(Clone)]
enum ConfigValues {
    Defaults(OnceLock<BTreeMap<String, ConfigValue>>),
    Map(BTreeMap<String, ConfigValue>),
}

impl Config {
    /// Builds a configuration from an explicit map.
    #[must_use]
    pub const fn from_map(values: BTreeMap<String, ConfigValue>) -> Self {
        Self {
            values: ConfigValues::Map(values),
        }
    }

    fn materialized(&self) -> Option<&BTreeMap<String, ConfigValue>> {
        match &self.values {
            ConfigValues::Defaults(values) => values.get(),
            ConfigValues::Map(values) => Some(values),
        }
    }

    fn values(&self) -> &BTreeMap<String, ConfigValue> {
        match &self.values {
            ConfigValues::Defaults(values) => values.get_or_init(default_values),
            ConfigValues::Map(values) => values,
        }
    }

    /// Number of keys carried.
    #[must_use]
    pub fn len(&self) -> usize {
        self.materialized()
            .map_or(UPSTREAM_DEFAULT_COUNT, BTreeMap::len)
    }

    /// Whether no key is carried.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.materialized().is_some_and(BTreeMap::is_empty)
    }

    /// Every key and value, in ascending key order.
    pub fn iter(&self) -> btree_map::Iter<'_, String, ConfigValue> {
        self.values().iter()
    }

    /// Sets `key`, returning what it held.
    pub fn set(&mut self, key: impl Into<String>, value: ConfigValue) -> Option<ConfigValue> {
        match &mut self.values {
            ConfigValues::Defaults(values) => {
                let mut values = values.take().unwrap_or_else(default_values);
                let previous = values.insert(key.into(), value);
                self.values = ConfigValues::Map(values);
                previous
            }
            ConfigValues::Map(values) => values.insert(key.into(), value),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            // Facade init only needs the default settings to exist. The pin
            // seeds its option word directly (074a2219 pinyin.cpp:329,
            // zhuyin.cpp:273); no String keys or sorted map are needed here.
            values: ConfigValues::Defaults(OnceLock::new()),
        }
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("values", self.values())
            .finish()
    }
}

impl PartialEq for Config {
    fn eq(&self, other: &Self) -> bool {
        match (self.materialized(), other.materialized()) {
            (None, None) => true,
            _ => self.values() == other.values(),
        }
    }
}

impl Eq for Config {}

impl ConfigSource for Config {
    fn get(&self, key: &str) -> Option<&ConfigValue> {
        self.values().get(key)
    }

    fn get_bool(&self, key: &str) -> Option<bool> {
        match self.materialized() {
            Some(values) => values.get(key).and_then(ConfigValue::as_bool),
            None => match default_seed(key) {
                Some(Seed::Bool(value)) => Some(value),
                _ => None,
            },
        }
    }

    fn get_int(&self, key: &str) -> Option<i32> {
        match self.materialized() {
            Some(values) => values.get(key).and_then(ConfigValue::as_int),
            None => match default_seed(key) {
                Some(Seed::Int(value)) => Some(value),
                _ => None,
            },
        }
    }

    fn get_int64(&self, key: &str) -> Option<i64> {
        match self.materialized() {
            Some(values) => values.get(key).and_then(ConfigValue::as_int64),
            None => match default_seed(key) {
                Some(Seed::Int64(value)) => Some(value),
                _ => None,
            },
        }
    }

    fn get_text(&self, key: &str) -> Option<&str> {
        match self.materialized() {
            Some(values) => values.get(key).and_then(ConfigValue::as_text),
            None => match default_seed(key) {
                Some(Seed::Text(value)) => Some(value),
                _ => None,
            },
        }
    }
}

impl<'a> IntoIterator for &'a Config {
    type IntoIter = btree_map::Iter<'a, String, ConfigValue>;
    type Item = (&'a String, &'a ConfigValue);

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Config, ConfigSource, ConfigValue, EmptyConfigSource};

    struct MapSource(BTreeMap<String, ConfigValue>);

    impl ConfigSource for MapSource {
        fn get(&self, key: &str) -> Option<&ConfigValue> {
            self.0.get(key)
        }
    }

    #[test]
    fn default_typed_reads_and_clones_do_not_materialize_the_map() {
        let config = Config::default();
        let source: &dyn ConfigSource = &config;
        assert_eq!(source.get_bool("incomplete-pinyin"), Some(true));
        assert_eq!(source.get_int("lookup-table-page-size"), Some(5));
        assert_eq!(
            source.get_int64("network-dictionary-start-timestamp"),
            Some(0)
        );
        assert_eq!(source.get_text("opencc-config"), Some("s2tw.json"));
        assert_eq!(source.get_text("dictionaries"), Some(""));
        assert_eq!(source.get_int("incomplete-pinyin"), None);
        assert_eq!(source.get_text("absent"), None);
        assert_eq!(config.len(), super::UPSTREAM_DEFAULT_COUNT);
        assert!(!config.is_empty());
        let cloned = config.clone();
        assert_eq!(config, cloned);
        assert!(config.materialized().is_none());
        assert!(cloned.materialized().is_none());
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
}
