//! Lazy reader for libpinyin's `bigram.db` — direct DBM consumption.
//!
//! libpinyin stores the bigram as a **HashDB** (KC) / **HashDBM** (Tkrzw),
//! while the other DBM files use TreeDB/TreeDBM. The store's
//! `RawReadStore::open_hash_read_only` selects the correct container class.
//!
//! - **Key:** previous `phrase_token_t` as 4 bytes LE
//! - **Value:** `total:u32` + `{next_token:u32, count:u32}[]`
//!
//! The value schema is byte-identical to what `BigramLanguageModel::open`
//! already parses. The difference is access pattern: `BigramLanguageModel`
//! eagerly slurps every row at init, while this reader does lazy per-key
//! `get_raw` lookups.
//!
//! See `docs/findings/libpinyin-system-data-formats-2026-09-01.md` §1.1
//! and `ngram_tkrzwdb.cpp`.

use std::path::Path;

use oxpinyin_store::{DefaultStore, RawReadStore};

use crate::chewing_table::{ChewingDbm, RawChewingDbm};
use crate::dict::DictError;
use crate::lm::BigramRow;
// The key and value layout this reader shares with `oxpinyin-datagen`'s
// writer: one written copy (`crate::row_format`).
#[cfg(test)]
use crate::row_format::bigram::encode_value as encode_bigram_value;
use crate::row_format::bigram::parse_value as parse_bigram_value;
use crate::row_format::encode_token_key;
use crate::table::TableError;

// ── BigramTable ──────────────────────────────────────────────────

/// A lazy, read-only view over a bigram DBM.
///
/// Does not materialize the entire database at open time. Each
/// `load_successors` call is a single DBM `get_raw`.
pub struct BigramTable {
    dbm: Box<dyn ChewingDbm + Send + Sync>,
}

impl BigramTable {
    pub(crate) fn new(dbm: Box<dyn ChewingDbm + Send + Sync>) -> Self {
        Self { dbm }
    }

    /// Opens a bigram DBM lazily (no scan). libpinyin's `bigram.db` is
    /// a KC **HashDB** / Tkrzw **HashDBM**, so the open goes through
    /// `RawReadStore::open_hash_read_only`.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the file cannot be opened read-only.
    pub fn open(path: &Path) -> Result<Self, DictError> {
        let store = DefaultStore::open_hash_read_only(path).map_err(TableError::from)?;
        Ok(Self::new(Box::new(RawChewingDbm::new(store))))
    }

    /// Loads successor records for `prev_token`.
    ///
    /// Returns `None` when the token has no bigram entry.
    pub fn load_successors(&self, prev_token: u32) -> Result<Option<BigramRow>, DictError> {
        let key = encode_token_key(prev_token);
        match self.dbm.get(&key)? {
            None => Ok(None),
            // A present but empty value is malformed: a stored bigram
            // always carries at least its header. Report the corruption
            // rather than masking it as a clean miss.
            Some(value) if value.is_empty() => Err(DictError::Parse(format!(
                "bigram entry for token {prev_token:#010x} has an empty value"
            ))),
            Some(value) => Ok(Some(parse_bigram_value(&value)?)),
        }
    }

    /// Returns `(count, total)` for the `prev → next` transition.
    pub fn transition(&self, prev: u32, next: u32) -> Result<Option<(u32, u32)>, DictError> {
        let Some(row) = self.load_successors(prev)? else {
            return Ok(None);
        };
        let count = row
            .records
            .iter()
            .find(|(next_token, _)| *next_token == next)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        Ok(Some((count, row.total)))
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
        fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }

        fn walk(
            &self,
            _lo: &[u8],
            _hi: Option<&[u8]>,
            _visit: &mut crate::chewing_table::RowVisitor<'_>,
        ) -> Result<(), DictError> {
            unreachable!("the bigram reader never walks")
        }
    }

    #[test]
    fn load_successors_finds_row() {
        let dbm = MemoryDbm::new();
        let row = BigramRow {
            total: 50,
            records: vec![(0x01000099, 50)],
        };
        dbm.put(
            0x01000010_u32.to_le_bytes().to_vec(),
            encode_bigram_value(&row),
        );
        let table = BigramTable::new(Box::new(dbm));
        let result = table.load_successors(0x01000010).unwrap().unwrap();
        assert_eq!(result.total, 50);
        assert_eq!(result.records.len(), 1);
    }

    #[test]
    fn load_successors_miss_returns_none() {
        let dbm = MemoryDbm::new();
        let table = BigramTable::new(Box::new(dbm));
        assert!(table.load_successors(0x01000010).unwrap().is_none());
    }

    #[test]
    fn transition_finds_count() {
        let dbm = MemoryDbm::new();
        let row = BigramRow {
            total: 100,
            records: vec![(0x01000099, 60), (0x010000A0, 40)],
        };
        dbm.put(
            0x01000010_u32.to_le_bytes().to_vec(),
            encode_bigram_value(&row),
        );
        let table = BigramTable::new(Box::new(dbm));
        let (count, total) = table.transition(0x01000010, 0x01000099).unwrap().unwrap();
        assert_eq!(count, 60);
        assert_eq!(total, 100);
    }

    #[test]
    fn transition_absent_next_returns_zero_count() {
        let dbm = MemoryDbm::new();
        let row = BigramRow {
            total: 100,
            records: vec![(0x01000099, 100)],
        };
        dbm.put(
            0x01000010_u32.to_le_bytes().to_vec(),
            encode_bigram_value(&row),
        );
        let table = BigramTable::new(Box::new(dbm));
        let (count, total) = table.transition(0x01000010, 0xFFFFFFFF).unwrap().unwrap();
        assert_eq!(count, 0);
        assert_eq!(total, 100);
    }

    #[test]
    fn malformed_value_does_not_panic() {
        let dbm = MemoryDbm::new();
        dbm.put(0x01000010_u32.to_le_bytes().to_vec(), vec![0xFF; 5]);
        let table = BigramTable::new(Box::new(dbm));
        assert!(table.load_successors(0x01000010).is_err());
    }

    #[test]
    fn empty_value_is_reported_not_silently_missed() {
        // A present key whose value is empty is corruption, not a miss:
        // libpinyin never stores a zero-length SingleGram. Surface it.
        let dbm = MemoryDbm::new();
        dbm.put(0x01000010_u32.to_le_bytes().to_vec(), Vec::new());
        let table = BigramTable::new(Box::new(dbm));
        assert!(table.load_successors(0x01000010).is_err());
    }
}
