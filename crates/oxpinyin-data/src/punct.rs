//! libpinyin's `punct.bin` — the predicted-punctuation table, read
//! directly.
//!
//! libpinyin stores the punctuation table as a **`TreeDB`** (KC) /
//! **`TreeDBM`** (Tkrzw) — same container class as the pinyin and phrase
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

use oxpinyin_core::PhraseText;
use oxpinyin_store::{DefaultStore, ReadStore};

use crate::chewing_table::{ChewingDbm, DefaultDbm, RawChewingDbm};
use crate::dict::DictError;
// The key and value layout this reader shares with `oxpinyin-datagen`'s
// writer: one written copy (`crate::row_format`).
use crate::row_format::encode_token_key;
use crate::row_format::punct::decode_puncts;
#[cfg(test)]
use crate::row_format::punct::encode_puncts;
use crate::table::TableError;

/// The read-only punctuation table over a DBM handle.
///
/// Does not materialize the table at open time. Each lookup is a single
/// point read.
pub struct PunctTable<D = DefaultDbm> {
    dbm: Option<D>,
}

impl<D: ChewingDbm> PunctTable<D> {
    pub(crate) const fn new(dbm: D) -> Self {
        Self { dbm: Some(dbm) }
    }

    /// An empty table: every lookup returns no punctuation.
    #[must_use]
    pub const fn empty() -> Self {
        Self { dbm: None }
    }

    /// Whether a table file is open behind this handle.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.dbm.is_some()
    }

    /// Punctuations stored for `token`, in table order, as stored words
    /// (`decode_puncts`).
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the read fails.
    pub fn punctuations(&self, token: u32) -> Result<Vec<PhraseText>, DictError> {
        let Some(dbm) = self.dbm.as_ref() else {
            return Ok(Vec::new());
        };
        let key = encode_token_key(token);
        let mut value = Vec::new();
        // Upstream's load_entry conflates absent and empty (0 == value.size());
        // remove_punctuation can leave a stored chunk at size 0. A miss
        // leaves the buffer empty, so one test covers both.
        dbm.get(&key, &mut value)?;
        if value.is_empty() {
            return Ok(Vec::new());
        }
        Ok(decode_puncts(&value))
    }
}

impl PunctTable {
    /// Opens a punct DBM lazily (no scan). `punct.bin` is a KC
    /// **`TreeDB`** / Tkrzw **`TreeDBM`** — the plain read-only open.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the file cannot be opened read-only.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        let store = DefaultStore::open_read_only(path).map_err(TableError::from)?;
        Ok(Self::new(RawChewingDbm::new(store)))
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
}

#[cfg(test)]
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
        fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
            value.clear();
            let data = self.data.lock().unwrap();
            let stored = data.get(key);
            if let Some(stored) = stored {
                value.extend_from_slice(stored);
            }
            Ok(stored.is_some())
        }

        fn contains(&self, key: &[u8]) -> Result<bool, DictError> {
            Ok(self.data.lock().unwrap().contains_key(key))
        }

        fn walk(
            &self,
            _lo: &[u8],
            _hi: Option<&[u8]>,
            _visit: impl FnMut(&[u8], &[u8]) -> Result<bool, DictError>,
        ) -> Result<(), DictError> {
            unreachable!("the punct reader never walks")
        }
    }

    #[test]
    fn an_empty_table_answers_nothing() {
        let table = PunctTable::<MemoryDbm>::empty();
        assert!(!table.is_open());
        assert!(table.punctuations(0x0100_0295).unwrap().is_empty());
        assert!(!PunctTable::open_optional(std::path::Path::new("/no/such/punct.bin")).is_open());
    }

    #[test]
    fn lookup_finds_punctuation() {
        let dbm = MemoryDbm::new();
        let token: u32 = 0x0100_0295;
        let value = encode_puncts(&["，", "。"]);
        dbm.put(token.to_le_bytes().to_vec(), value);

        let table = PunctTable::new(dbm);
        let result = table.punctuations(token).unwrap();
        assert_eq!(result, ["，", "。"]);
    }

    #[test]
    fn lookup_miss_returns_empty() {
        let dbm = MemoryDbm::new();
        let table = PunctTable::new(dbm);
        let result = table.punctuations(0x0100_0295).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn malformed_value_decodes_as_the_pin_walks_it() {
        let dbm = MemoryDbm::new();
        dbm.put(0x0100_0295_u32.to_le_bytes().to_vec(), vec![0xFF; 3]);
        let table = PunctTable::new(dbm);
        assert_eq!(table.punctuations(0x0100_0295).unwrap(), [""]);
    }
}
