//! Lazy reader for libpinyin's `punct.bin` — direct DBM consumption.
//!
//! libpinyin stores the punctuation table as a **TreeDB** (KC) /
//! **TreeDBM** (Tkrzw) — same container class as the pinyin and phrase
//! indexes. The key is a `phrase_token_t` (4 bytes LE); the value is a
//! raw UCS-4 stream (`PunctTableEntry::escape`, `punct_table.cpp:40-54`):
//! each punctuation is its UCS-4 codepoints followed by a u32 zero
//! terminator, with successive punctuations concatenated — e.g. `，` then
//! `、` stores `[0xFF0C, 0][0x3001, 0]` as little-endian u32s.
//!
//! Access pattern: `PunctTable` eagerly walks every row into a
//! `BTreeMap`, while this reader does lazy per-key `get_raw` lookups.
//!
//! See `docs/findings/prediction-punct.md`.

use std::path::Path;

use oxpinyin_store::{DefaultStore, ReadStore};

use crate::chewing_table::{ChewingDbm, RawChewingDbm};
use crate::dict::DictError;
use crate::table::TableError;

/// Decodes a UCS-4 punctuation stream — u32 codepoints, each
/// punctuation zero-terminated, concatenated (`PunctTableEntry::unescape`
/// + `get_all_punctuations`, `punct_table.cpp:56-94`).
///
/// # Errors
///
/// Returns [`DictError::Parse`] when the value is not u32-aligned, ends
/// without a terminator, or holds an undecodable scalar. Upstream reads
/// past the buffer on such input (memory-safety class); the Rust reader
/// refuses it instead (`docs/findings/upstream-divergences.md`).
pub(crate) fn decode_puncts(value: &[u8]) -> Result<Vec<String>, DictError> {
    if !value.len().is_multiple_of(4) {
        return Err(DictError::Parse(
            "punct value is not u32-aligned".to_owned(),
        ));
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut terminated = false;
    for chunk in value.chunks_exact(4) {
        let code = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
        if code == 0 {
            if current.is_empty() {
                return Err(DictError::Parse(
                    "punct value holds an empty punctuation".to_owned(),
                ));
            }
            out.push(std::mem::take(&mut current));
            terminated = true;
            continue;
        }
        terminated = false;
        let Some(scalar) = char::from_u32(code) else {
            return Err(DictError::Parse(
                "punct value holds an invalid UCS-4 scalar".to_owned(),
            ));
        };
        current.push(scalar);
    }
    if !terminated {
        return Err(DictError::Parse(
            "punct value is not zero-terminated".to_owned(),
        ));
    }
    Ok(out)
}

/// Encodes punctuation strings as a UCS-4 stream with u32 zero
/// terminators — `PunctTableEntry::escape`'s layout.
#[cfg(test)]
pub(crate) fn encode_puncts(puncts: &[&str]) -> Vec<u8> {
    let mut buf = Vec::new();
    for punct in puncts {
        for ch in punct.chars() {
            buf.extend_from_slice(&(ch as u32).to_le_bytes());
        }
        buf.extend_from_slice(&0_u32.to_le_bytes());
    }
    buf
}

/// A lazy, read-only punctuation table backed by a DBM.
///
/// Does not materialize the entire table at open time. Each lookup is
/// a single `get_raw`.
pub struct LazyPunctTable {
    dbm: Box<dyn ChewingDbm + Send + Sync>,
}

impl LazyPunctTable {
    pub(crate) fn new(dbm: Box<dyn ChewingDbm + Send + Sync>) -> Self {
        Self { dbm }
    }

    /// Opens a punct DBM lazily (no scan). `punct.bin` is a KC
    /// **TreeDB** / Tkrzw **TreeDBM** — the plain read-only open.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the file cannot be opened read-only.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        let store = DefaultStore::open_read_only(path).map_err(TableError::from)?;
        Ok(Self::new(Box::new(RawChewingDbm::new(store))))
    }

    /// Punctuation strings stored for `token`, in table order.
    pub fn punctuations(&self, token: u32) -> Result<Vec<String>, DictError> {
        let key = token.to_le_bytes();
        match self.dbm.get(&key)? {
            Some(value) if !value.is_empty() => decode_puncts(&value),
            _ => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct MemoryDbm {
        data: Mutex<BTreeMap<Vec<u8>, Vec<u8>>>,
    }

    impl MemoryDbm {
        fn new() -> Self {
            Self {
                data: Mutex::new(BTreeMap::new()),
            }
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) {
            self.data.lock().unwrap().insert(key, value);
        }
    }

    impl ChewingDbm for MemoryDbm {
        fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }
    }

    #[test]
    fn encode_decode_round_trip() {
        let puncts = &["，", "。"];
        let encoded = encode_puncts(puncts);
        let decoded = decode_puncts(&encoded).unwrap();
        assert_eq!(decoded, vec!["，", "。"]);
    }

    #[test]
    fn decode_rejects_unterminated() {
        assert!(decode_puncts("，".as_bytes()).is_err());
    }

    #[test]
    fn decode_rejects_empty() {
        assert!(decode_puncts(b"").is_err());
    }

    #[test]
    fn decode_rejects_empty_field() {
        assert!(decode_puncts(b"\x00").is_err());
        assert!(decode_puncts(b"\xef\xbc\x8c\x00\x00\xe3\x80\x82\x00").is_err());
    }

    #[test]
    fn lookup_finds_punctuation() {
        let dbm = MemoryDbm::new();
        let token: u32 = 0x01000295;
        let value = encode_puncts(&["，", "。"]);
        dbm.put(token.to_le_bytes().to_vec(), value);

        let table = LazyPunctTable::new(Box::new(dbm));
        let result = table.punctuations(token).unwrap();
        assert_eq!(result, vec!["，", "。"]);
    }

    #[test]
    fn lookup_miss_returns_empty() {
        let dbm = MemoryDbm::new();
        let table = LazyPunctTable::new(Box::new(dbm));
        let result = table.punctuations(0x01000295).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn malformed_value_does_not_panic() {
        let dbm = MemoryDbm::new();
        dbm.put(0x01000295_u32.to_le_bytes().to_vec(), vec![0xFF; 3]);
        let table = LazyPunctTable::new(Box::new(dbm));
        assert!(table.punctuations(0x01000295).is_err());
    }
}
