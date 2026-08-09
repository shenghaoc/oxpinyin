//! The configuration seam: typed settings read by key.
//!
//! This module holds only the seam. The concrete layered configuration, its
//! captured upstream defaults and the pure merge function are W4-T0c; a
//! GSettings, registry or plist backend is a shell concern outside both.

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

/// Anything a session can read settings from.
///
/// Object-safe on purpose: a session takes `&dyn ConfigSource`, so a shell can
/// pass a layered `Config`, a file-backed test source or its own adapter
/// without the engine growing a type parameter. The trait grows only by
/// methods with default implementations.
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
/// Every session setting falls back to its documented default, which is what a
/// caller wants before W4-T0c's layered configuration exists and what tests
/// want when the setting under test is not configuration.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct EmptyConfigSource;

impl ConfigSource for EmptyConfigSource {
    fn get(&self, _key: &str) -> Option<&ConfigValue> {
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{ConfigSource, ConfigValue, EmptyConfigSource};

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
}
