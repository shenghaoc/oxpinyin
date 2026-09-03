//! libpinyin's `punct.bin` — the predicted-punctuation table, read
//! directly.
//!
//! libpinyin stores the punctuation table as a **TreeDB** (KC) /
//! **TreeDBM** (Tkrzw) — same container class as the pinyin and phrase
//! indexes. The key is a `phrase_token_t` (4 bytes LE); the value is a
//! raw UCS-4 stream (`PunctTableEntry::escape`, `punct_table.cpp:40-54`):
//! each punctuation is its UCS-4 codepoints followed by a u32 zero
//! terminator, with successive punctuations concatenated — e.g. `，` then
//! `、` stores `[0xFF0C, 0][0x3001, 0]` as little-endian u32s.
//!
//! Every lookup is one point read; nothing is walked at open. Upstream's
//! `pinyin_init` ignores a failed `PunctTable::attach`, so a missing or
//! unopenable file is an empty table ([`PunctTable::open_optional`]).
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

/// The read-only punctuation table over a DBM handle.
///
/// Does not materialize the table at open time. Each lookup is a single
/// point read.
pub struct PunctTable {
    dbm: Option<Box<dyn ChewingDbm + Send + Sync>>,
}

impl PunctTable {
    pub(crate) fn new(dbm: Box<dyn ChewingDbm + Send + Sync>) -> Self {
        Self { dbm: Some(dbm) }
    }

    /// An empty table: every lookup returns no punctuation.
    #[must_use]
    pub const fn empty() -> Self {
        Self { dbm: None }
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

    /// Opens `path` when it is a readable table; otherwise empty —
    /// upstream `pinyin_init` ignores a failed `PunctTable::attach`.
    #[must_use]
    pub fn open_optional(path: &Path) -> Self {
        if !path.is_file() {
            return Self::empty();
        }
        Self::open(path).unwrap_or_else(|_| Self::empty())
    }

    /// Whether a table file is open behind this handle.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.dbm.is_some()
    }

    /// Punctuation strings stored for `token`, in table order.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the read fails or the value is malformed.
    pub fn punctuations(&self, token: u32) -> Result<Vec<String>, DictError> {
        let Some(dbm) = self.dbm.as_ref() else {
            return Ok(Vec::new());
        };
        let key = token.to_le_bytes();
        match dbm.get(&key)? {
            // Upstream's load_entry conflates absent and empty (0 == value.size());
            // remove_punctuation can leave a stored chunk at size 0.
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

        fn walk(
            &self,
            _lo: &[u8],
            _hi: Option<&[u8]>,
            _visit: &mut crate::chewing_table::RowVisitor<'_>,
        ) -> Result<(), DictError> {
            unreachable!("the punct reader never walks")
        }
    }

    #[test]
    fn an_empty_table_answers_nothing() {
        let table = PunctTable::empty();
        assert!(!table.is_open());
        assert!(table.punctuations(0x01000295).unwrap().is_empty());
        assert!(!PunctTable::open_optional(std::path::Path::new("/no/such/punct.bin")).is_open());
    }

    #[test]
    fn encode_decode_round_trip() {
        let puncts = &["，", "。"];
        let encoded = encode_puncts(puncts);
        let decoded = decode_puncts(&encoded).unwrap();
        assert_eq!(decoded, vec!["，", "。"]);
    }

    #[test]
    fn decode_rejects_unaligned() {
        assert!(decode_puncts("，".as_bytes()).is_err());
        assert!(decode_puncts(b"\x00").is_err());
        assert!(decode_puncts(b"").is_err());
    }

    #[test]
    fn decode_rejects_unterminated() {
        let one_codepoint = 0xFF0Cu32.to_le_bytes();
        assert!(decode_puncts(&one_codepoint).is_err());
    }

    #[test]
    fn decode_rejects_empty_field() {
        let mut value = Vec::new();
        value.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_puncts(&value).is_err());

        let mut value = Vec::new();
        value.extend_from_slice(&0xFF0Cu32.to_le_bytes());
        value.extend_from_slice(&0u32.to_le_bytes());
        value.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_puncts(&value).is_err());
    }

    #[test]
    fn lookup_finds_punctuation() {
        let dbm = MemoryDbm::new();
        let token: u32 = 0x01000295;
        let value = encode_puncts(&["，", "。"]);
        dbm.put(token.to_le_bytes().to_vec(), value);

        let table = PunctTable::new(Box::new(dbm));
        let result = table.punctuations(token).unwrap();
        assert_eq!(result, vec!["，", "。"]);
    }

    #[test]
    fn lookup_miss_returns_empty() {
        let dbm = MemoryDbm::new();
        let table = PunctTable::new(Box::new(dbm));
        let result = table.punctuations(0x01000295).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn malformed_value_does_not_panic() {
        let dbm = MemoryDbm::new();
        dbm.put(0x01000295_u32.to_le_bytes().to_vec(), vec![0xFF; 3]);
        let table = PunctTable::new(Box::new(dbm));
        assert!(table.punctuations(0x01000295).is_err());
    }
}
