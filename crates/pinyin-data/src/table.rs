//! Loader for redb-backed lookup tables (converted from oracle Tkrzw files).
//!
//! Each table is a redb database with a single `data` table mapping raw
//! `&[u8]` keys to raw `&[u8]` values.  These are produced by
//! `pinyin-migrate` from the oracle's installed Tkrzw HashDB files.
//!
//! # Portability
//!
//! redb is a pure-Rust embedded database.  Tables produced on Linux by
//! the migrator can be read on any platform redb supports.

use std::fmt;
use std::path::Path;

use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};

const DATA_TABLE: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("data");

/// Errors that can occur when opening or querying a table.
#[derive(Debug)]
pub enum TableError {
    /// The redb file could not be opened (I/O error).
    Io(std::io::Error),
    /// redb reported a database-level error.
    Db(redb::DatabaseError),
    /// redb reported a table-level error.
    Table(redb::TableError),
    /// redb reported a transaction-level error.
    Transaction(redb::TransactionError),
    /// redb reported a storage-level error.
    Storage(redb::StorageError),
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::Db(e) => write!(f, "database error: {e}"),
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Transaction(e) => write!(f, "transaction error: {e}"),
            Self::Storage(e) => write!(f, "storage error: {e}"),
        }
    }
}

impl std::error::Error for TableError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Db(e) => Some(e),
            Self::Table(e) => Some(e),
            Self::Transaction(e) => Some(e),
            Self::Storage(e) => Some(e),
        }
    }
}

impl From<redb::DatabaseError> for TableError {
    fn from(e: redb::DatabaseError) -> Self {
        Self::Db(e)
    }
}

impl From<redb::TableError> for TableError {
    fn from(e: redb::TableError) -> Self {
        Self::Table(e)
    }
}

impl From<redb::TransactionError> for TableError {
    fn from(e: redb::TransactionError) -> Self {
        Self::Transaction(e)
    }
}

impl From<redb::StorageError> for TableError {
    fn from(e: redb::StorageError) -> Self {
        Self::Storage(e)
    }
}

/// A read-only lookup table backed by a redb database.
///
/// Keys and values are opaque byte slices.  Interpretation (e.g. as
/// `phrase_token_t[]` arrays or UTF-8 text) is the caller's responsibility.
pub struct LookupTable {
    db: redb::ReadOnlyDatabase,
}

impl LookupTable {
    /// Open a redb table file for reading.
    pub fn open(path: &Path) -> Result<Self, TableError> {
        let db = redb::Builder::new()
            .open_read_only(path)
            .map_err(|e| match e {
                redb::DatabaseError::Storage(redb::StorageError::Io(io)) => TableError::Io(io),
                other => TableError::Db(other),
            })?;
        Ok(Self { db })
    }

    /// Look up a key in the table.
    ///
    /// Returns `None` if the key is not present.
    pub fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, TableError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(DATA_TABLE)?;
        let val = table.get(key)?;
        let result = val.map(|v| v.value().to_vec());
        Ok(result)
    }

    /// Return the number of entries in the table.
    pub fn len(&self) -> Result<u64, TableError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(DATA_TABLE)?;
        Ok(table.len()?)
    }

    /// Returns `true` if the table is empty.
    pub fn is_empty(&self) -> Result<bool, TableError> {
        Ok(self.len()? == 0)
    }

    /// Iterate over all (key, value) pairs.
    #[allow(clippy::type_complexity)]
    pub fn iter(&self) -> Result<Vec<(Vec<u8>, Vec<u8>)>, TableError> {
        let txn = self.db.begin_read()?;
        let table = txn.open_table(DATA_TABLE)?;
        let mut entries = Vec::new();
        for item in table.iter()? {
            let (k, v) = item?;
            entries.push((k.value().to_vec(), v.value().to_vec()));
        }
        Ok(entries)
    }
}

impl fmt::Debug for LookupTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LookupTable")
            .field("len", &self.len().unwrap_or(0))
            .finish()
    }
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixtures_dir() -> PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        PathBuf::from(manifest)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("w3")
    }

    #[test]
    fn open_punct_fixture() {
        let table = LookupTable::open(&fixtures_dir().join("punct.redb")).unwrap();
        assert_eq!(table.len().unwrap(), 1);
        assert!(!table.is_empty().unwrap());
    }

    #[test]
    fn punct_key_matches_oracle() {
        // The oracle's punct.bin has exactly one record with the key
        // [0x00, 0x00, 0x00, 0x00, 0x00, 0x01], verified via C++ API.
        let table = LookupTable::open(&fixtures_dir().join("punct.redb")).unwrap();
        let val = table.get(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x01]).unwrap();
        assert!(
            val.is_some(),
            "expected key [00 00 00 00 00 01] to be present"
        );
        let val = val.unwrap();
        // Value should be non-empty (4604 bytes in the full oracle file).
        assert!(!val.is_empty());
        // Check the first 8 bytes: all zeros in the oracle data.
        assert_eq!(&val[..8], &[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]);
    }

    #[test]
    fn missing_key_returns_none() {
        let table = LookupTable::open(&fixtures_dir().join("punct.redb")).unwrap();
        let val = table.get(b"nonexistent").unwrap();
        assert!(val.is_none());
    }

    #[test]
    fn pinyin_index_fixture_record_count() {
        let table = LookupTable::open(&fixtures_dir().join("pinyin_index.redb")).unwrap();
        // Mini fixture: 50 records truncated from 928.
        assert_eq!(table.len().unwrap(), 50);
    }

    #[test]
    fn phrase_index_fixture_record_count() {
        let table = LookupTable::open(&fixtures_dir().join("phrase_index.redb")).unwrap();
        // Mini fixture: 50 records truncated from 590.
        assert_eq!(table.len().unwrap(), 50);
    }

    #[test]
    fn addon_phrase_index_full_fixture() {
        let table = LookupTable::open(&fixtures_dir().join("addon_phrase_index.redb")).unwrap();
        // Full fixture: 68 records.
        assert_eq!(table.len().unwrap(), 68);
    }

    #[test]
    fn addon_pinyin_index_full_fixture() {
        let table = LookupTable::open(&fixtures_dir().join("addon_pinyin_index.redb")).unwrap();
        // Full fixture: 117 records.
        assert_eq!(table.len().unwrap(), 117);
    }

    #[test]
    fn bigram_fixture_record_count() {
        let table = LookupTable::open(&fixtures_dir().join("bigram.redb")).unwrap();
        // Mini fixture: 50 records truncated from 56359.
        assert_eq!(table.len().unwrap(), 50);
    }

    #[test]
    fn iter_all_fixture_files() {
        let dir = fixtures_dir();
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().and_then(|s| s.to_str()) != Some("redb") {
                continue;
            }
            let table = LookupTable::open(&path).unwrap_or_else(|e| {
                panic!("failed to open {}: {e}", path.display());
            });
            let count = table.len().unwrap();
            assert!(count > 0, "{} should have records", path.display());

            // Verify iteration matches count.
            let entries = table.iter().unwrap();
            assert_eq!(entries.len() as u64, count);
        }
    }
}
