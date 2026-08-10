//! System dictionary backed by the exported pinyin index and phrase index.
//!
//! Implements [`pinyin_core::Dictionary`] over the tables that
//! `pinyin-migrate export` derives from the pinned oracle's public ABI
//! (`docs/findings/data-layer-export.md`). The index is keyed by the
//! pinyin spelling itself — syllables joined by `'` — so a lookup for
//! `[ni, hao]` is a single get on `ni'hao`; there is no per-syllable
//! binary encoder and no compound binary key. Entries come back in the
//! stored order, which the exporter froze as frequency-descending.

use std::fmt;
use std::path::Path;

use pinyin_core::{Dictionary, PhraseEntry, PhraseToken, SyllableKey};

use crate::table::{LookupTable, TableError};

/// Error conditions for system dictionary lookups.
#[derive(Debug)]
pub enum DictError {
    /// A table-level error (I/O, redb, etc.).
    Table(TableError),
    /// Value bytes did not parse as `{token, freq}` records.
    Parse(String),
}

impl fmt::Display for DictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for DictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
            Self::Parse(_) => None,
        }
    }
}

impl From<TableError> for DictError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

/// The system dictionary, backed by `pinyin_index.redb` and
/// `phrase_index.redb` from `pinyin-migrate export`.
pub struct SystemDictionary {
    pinyin_index: LookupTable,
    phrase_index: LookupTable,
}

impl SystemDictionary {
    /// Opens the system dictionary from the two exported table files.
    pub fn open(pinyin_index_path: &Path, phrase_index_path: &Path) -> Result<Self, DictError> {
        Ok(Self {
            pinyin_index: LookupTable::open(pinyin_index_path)?,
            phrase_index: LookupTable::open(phrase_index_path)?,
        })
    }

    /// Number of pinyin keys in the index.
    pub fn key_count(&self) -> Result<u64, DictError> {
        Ok(self.pinyin_index.len()?)
    }

    /// The frozen index key for a syllable sequence: texts joined by `'`.
    fn index_key(syllables: &[SyllableKey]) -> String {
        let mut key = String::new();
        for (position, syllable) in syllables.iter().enumerate() {
            if position > 0 {
                key.push('\'');
            }
            key.push_str(syllable.text());
        }
        key
    }
}

impl Dictionary for SystemDictionary {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        if syllables.is_empty() {
            return Ok(Vec::new());
        }
        let key = Self::index_key(syllables);
        let Some(raw) = self.pinyin_index.get(key.as_bytes())? else {
            return Ok(Vec::new());
        };

        let mut entries = Vec::new();
        for (token, _freq) in parse_index_records(&raw)? {
            // Token → text through phrase_index. The full export resolves
            // every token; a mini fixture may omit some, and those records
            // contribute no candidate rather than failing the lookup.
            let key_bytes = token.to_le_bytes();
            if let Some(text_bytes) = self.phrase_index.get(&key_bytes)? {
                let text = String::from_utf8(text_bytes).map_err(|_| {
                    DictError::Parse(format!("phrase text for token {token:#010x} is not UTF-8"))
                })?;
                entries.push(PhraseEntry::new(PhraseToken::new(token), text));
            }
        }
        Ok(entries)
    }
}

/// Parses an index value as `{token: u32 LE, freq: u32 LE}` records.
fn parse_index_records(data: &[u8]) -> Result<Vec<(u32, u32)>, DictError> {
    if !data.len().is_multiple_of(8) {
        return Err(DictError::Parse(format!(
            "index value length {} is not a multiple of 8",
            data.len()
        )));
    }
    Ok(data
        .chunks_exact(8)
        .map(|chunk| {
            (
                u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
            )
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn dict() -> SystemDictionary {
        SystemDictionary::open(
            &fixtures_dir().join("pinyin_index.redb"),
            &fixtures_dir().join("phrase_index.redb"),
        )
        .unwrap()
    }

    fn key(text: &str) -> SyllableKey {
        SyllableKey::from_text(text).expect("frozen syllable")
    }

    #[test]
    fn mini_fixture_opens() {
        assert_eq!(dict().key_count().unwrap(), 10);
    }

    #[test]
    fn single_syllable_is_frequency_ranked() {
        let entries = dict().lookup(&[key("ni")]).unwrap();
        assert!(!entries.is_empty());
        // 你 dominates the pin's ni column; the exporter froze
        // frequency-descending order.
        assert_eq!(entries[0].text(), "你");
    }

    #[test]
    fn multi_syllable_lookup_is_one_string_key() {
        let entries = dict().lookup(&[key("ni"), key("hao")]).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text(), "你好");

        let entries = dict().lookup(&[key("zhong"), key("guo")]).unwrap();
        assert!(entries.iter().any(|entry| entry.text() == "中国"));
    }

    #[test]
    fn apostrophe_keeps_xian_and_xi_an_apart() {
        let xian = dict().lookup(&[key("xian")]).unwrap();
        assert!(xian.iter().any(|entry| entry.text() == "现"));
        assert!(!xian.iter().any(|entry| entry.text() == "西安"));

        let xi_an = dict().lookup(&[key("xi"), key("an")]).unwrap();
        assert!(xi_an.iter().any(|entry| entry.text() == "西安"));
        assert!(!xi_an.iter().any(|entry| entry.text() == "现"));
    }

    #[test]
    fn unknown_sequence_is_empty_not_an_error() {
        let entries = dict().lookup(&[key("zhuang"), key("zhuang")]).unwrap();
        assert!(entries.is_empty());
        let entries = dict().lookup(&[]).unwrap();
        assert!(entries.is_empty());
    }
}
