//! System dictionary backed by the oracle's pinyin index and phrase index tables.
//!
//! Implements [`pinyin_core::Dictionary`] for the libpinyin system dictionary
//! loaded from redb tables. Syllables are identified by [`pinyin_core::SyllableKey`]
//! (the decoder's u16 ids); the `SyllableKey → TableKey` encoding is handled
//! internally via [`crate::encoder`]. Phrase entries are resolved via
//! `phrase_index` (token → text), so `Dictionary::Entry` is [`pinyin_core::PhraseEntry`]
//! as W4's `Session` expects.

use std::fmt;
use std::path::Path;

use pinyin_core::{Dictionary, PhraseEntry, PhraseToken, SyllableKey};

use crate::encoder::{EncoderError, encode};
use crate::table::{LookupTable, TableError};

/// Error conditions for system dictionary lookups.
#[derive(Debug)]
pub enum DictError {
    /// A table-level error (I/O, redb, etc.).
    Table(TableError),
    /// Value bytes did not parse as expected.
    Parse(String),
    /// SyllableKey → TableKey encoding is blocked pending oracle FFI.
    Encoder(EncoderError),
}

impl fmt::Display for DictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::Encoder(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for DictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
            Self::Parse(_) => None,
            Self::Encoder(e) => Some(e),
        }
    }
}

impl From<TableError> for DictError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

impl From<EncoderError> for DictError {
    fn from(e: EncoderError) -> Self {
        Self::Encoder(e)
    }
}

/// The system dictionary, backed by `pinyin_index.redb` and `phrase_index.redb`.
///
/// Each syllable key maps to an array of `phrase_token_t` values (u32 LE).
/// The dictionary returns [`PhraseEntry`] (token + text) by resolving each
/// token through `phrase_index`.
pub struct SystemDictionary {
    pinyin_index: LookupTable,
    /// Opened for token → text resolution (PhraseEntry).
    /// Currently blocked by the syllable encoder stub;
    /// see blocked:syllable-encoder.
    phrase_index: LookupTable,
}

impl SystemDictionary {
    /// Open the system dictionary from redb table files.
    ///
    /// `pinyin_index_path` should point to `pinyin_index.redb`.
    /// `phrase_index_path` should point to `phrase_index.redb`.
    pub fn open(pinyin_index_path: &Path, phrase_index_path: &Path) -> Result<Self, DictError> {
        let pinyin_index = LookupTable::open(pinyin_index_path)?;
        let phrase_index = LookupTable::open(phrase_index_path)?;
        Ok(Self {
            pinyin_index,
            phrase_index,
        })
    }

    /// Return the number of syllable entries in the pinyin index.
    pub fn syllable_count(&self) -> Result<u64, DictError> {
        Ok(self.pinyin_index.len()?)
    }
}

impl Dictionary for SystemDictionary {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        for syllable in syllables {
            let table_key = encode(*syllable)?;
            if let Some(raw) = self.pinyin_index.get(&table_key)? {
                let tokens = parse_phrase_tokens(&raw);
                for token in tokens {
                    // Resolve token → text via phrase_index.
                    // Key is token value as 4-byte LE; value is UTF-8 phrase text.
                    let key_bytes = token.value().to_le_bytes();
                    if let Some(text_bytes) = self.phrase_index.get(&key_bytes)? {
                        // Oracle stores phrase text as UTF-8; lossy fallback is safe
                        // for the redb dump which is already validated by oracle cross-check.
                        let text = String::from_utf8(text_bytes)
                            .unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned());
                        entries.push(PhraseEntry::new(token, text));
                    } else {
                        // Token without a phrase_index entry is skipped — the tables
                        // are truncated fixtures (50 records) and not every token
                        // is present in the mini dump.
                    }
                }
            }
        }
        Ok(entries)
    }
}

/// Parse a value blob as an array of u32 LE phrase tokens.
///
/// The value from `pinyin_index` is a contiguous array of `phrase_token_t`
/// (u32 LE). The first word is typically zero (sentinel/header) and is
/// skipped.
fn parse_phrase_tokens(data: &[u8]) -> Vec<PhraseToken> {
    if data.len() < 4 {
        return Vec::new();
    }
    let words: Vec<u32> = data
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    // Skip the leading zero sentinel.
    let start = if words.first() == Some(&0) { 1 } else { 0 };
    words[start..]
        .iter()
        .filter(|&&w| w != 0)
        .map(|&w| PhraseToken::new(w))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinyin_core::SyllableKey;

    fn fixtures_dir() -> std::path::PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        std::path::PathBuf::from(manifest)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("w3")
    }

    #[test]
    fn system_dict_opens_from_fixtures() {
        let dict = SystemDictionary::open(
            &fixtures_dir().join("pinyin_index.redb"),
            &fixtures_dir().join("phrase_index.redb"),
        )
        .unwrap();
        // 50 records in the mini fixture.
        assert_eq!(dict.syllable_count().unwrap(), 50);
    }

    #[test]
    fn lookup_is_blocked_without_encoder() {
        let dict = SystemDictionary::open(
            &fixtures_dir().join("pinyin_index.redb"),
            &fixtures_dir().join("phrase_index.redb"),
        )
        .unwrap();
        let key = SyllableKey::from_text("ni").expect("ni is a frozen key");
        let err = dict.lookup(&[key]).expect_err("encoder is blocked");
        assert!(matches!(err, DictError::Encoder(_)));
        assert!(err.to_string().contains("blocked:syllable-encoder"));
    }

    #[test]
    fn missing_syllable_returns_blocked_not_empty() {
        // Even a missing syllable is blocked at the encoder layer — the
        // 6-byte TableKey layout is unknown without oracle FFI, so we cannot
        // probe pinyin_index at all.
        let dict = SystemDictionary::open(
            &fixtures_dir().join("pinyin_index.redb"),
            &fixtures_dir().join("phrase_index.redb"),
        )
        .unwrap();
        let key = SyllableKey::from_text("zhuan").expect("zhuan is a frozen key");
        let err = dict.lookup(&[key]).expect_err("blocked");
        assert!(err.to_string().contains("blocked:syllable-encoder"));
    }

    #[test]
    fn parse_phrase_tokens_skips_sentinel() {
        let data: Vec<u8> = [0u32, 42, 0, 7]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let tokens = parse_phrase_tokens(&data);
        assert_eq!(tokens.len(), 2);
        assert_eq!(tokens[0].value(), 42);
        assert_eq!(tokens[1].value(), 7);
    }
}
