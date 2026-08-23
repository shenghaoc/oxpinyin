//! Backend-agnostic ordered key–value store for oxpinyin tables.
//!
//! This crate defines the [`OrderedStore`] trait — an ordered byte-KV
//! interface with point get, ranged scan, atomic multi-table writes,
//! emptiness check, and compaction — and provides a [`RedbStore`]
//! implementation backed by redb.  Consumers depend on the trait; the
//! concrete backend is selected by the [`DefaultStore`] alias.

use std::fmt;
use std::ops::Bound;
use std::path::Path;

use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata};

/// Errors that can occur when opening or scanning a store.
///
/// The variants preserve the I/O-vs-other distinction without naming any
/// backend type, so callers can branch on I/O failures independently of
/// which backend produced them.
#[derive(Debug)]
pub enum StoreError {
    /// An I/O error from the underlying storage layer.
    Io(std::io::Error),
    /// A non-I/O error from the storage backend (type-erased).
    Backend(Box<dyn std::error::Error + Send + Sync>),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "store I/O error: {e}"),
            Self::Backend(e) => write!(f, "store error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Backend(e) => Some(e.as_ref()),
        }
    }
}

/// Row visitor passed to store scan methods.
pub type Visitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), StoreError> + 'a;

// ── traits ─────────────────────────────────────────────────────────

/// An atomic write transaction over one or more tables.
///
/// Operations see their own writes (read-your-writes within the closure).
/// All modifications land atomically on commit or are fully rolled back.
/// Used as `&mut dyn WriteTxn`.
pub trait WriteTxn {
    /// Read a single key from `table`.  Returns `None` if absent.
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Insert or overwrite `key` → `value` in `table`.
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError>;

    /// Remove `key` from `table` (no-op if absent).
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError>;

    /// Visit rows of `table` whose keys fall in the `[lo, hi]` range,
    /// ascending key-byte order.
    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError>;

    /// Visit every row of `table` in ascending key-byte order.
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError>;
}

/// An ordered byte-KV store.
///
/// Implementations provide both read-only and read-write access, point
/// get, ranged scans, atomic multi-table writes, and compaction.
pub trait OrderedStore {
    /// Open an existing store file in read-only mode.
    fn open_read_only(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Open or create a store file in read-write mode.
    fn create(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Read a single key from `table`.  Returns `None` if absent or the
    /// table does not exist.
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Visit every `(key, value)` of `table` in ascending key-byte order,
    /// borrowing each row (no per-row clone).  Stops early on visitor `Err`.
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError>;

    /// Visit rows of `table` whose keys fall in the `[lo, hi]` range,
    /// ascending.  An absent table is treated as empty.
    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError>;

    /// Whether `table` has no rows (an absent table counts as empty).
    fn is_empty(&self, table: &str) -> Result<bool, StoreError>;

    /// Run `f` inside an atomic write transaction.  All puts/removes in
    /// `f` land together on `Ok`, or none land on `Err` (full rollback).
    /// The closure sees its own writes.
    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError>;

    /// Compact the database file, reclaiming free pages.
    fn compact(&mut self) -> Result<(), StoreError>;
}

// ── redb backend ───────────────────────────────────────────────────

enum RedbInner {
    ReadOnly(redb::ReadOnlyDatabase),
    ReadWrite(redb::Database),
}

/// A redb-backed [`OrderedStore`].
pub struct RedbStore {
    inner: RedbInner,
}

impl RedbStore {
    fn begin_read(&self) -> Result<redb::ReadTransaction, StoreError> {
        match &self.inner {
            RedbInner::ReadOnly(db) => db.begin_read().map_err(map_transaction_error),
            RedbInner::ReadWrite(db) => db.begin_read().map_err(map_transaction_error),
        }
    }
}

impl OrderedStore for RedbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let db = redb::Builder::new()
            .open_read_only(path)
            .map_err(map_database_error)?;
        Ok(Self {
            inner: RedbInner::ReadOnly(db),
        })
    }

    fn create(path: &Path) -> Result<Self, StoreError> {
        let db = redb::Database::create(path).map_err(map_database_error)?;
        Ok(Self {
            inner: RedbInner::ReadWrite(db),
        })
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let txn = self.begin_read()?;
        match txn.open_table(def) {
            Ok(tbl) => Ok(tbl
                .get(key)
                .map_err(map_storage_error)?
                .map(|g| g.value().to_vec())),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
            Err(e) => Err(map_table_error(e)),
        }
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let txn = self.begin_read()?;
        let tbl = txn.open_table(def).map_err(map_table_error)?;
        for item in tbl.iter().map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let txn = self.begin_read()?;
        match txn.open_table(def) {
            Ok(tbl) => {
                for item in tbl.range::<&[u8]>((lo, hi)).map_err(map_storage_error)? {
                    let (key, value) = item.map_err(map_storage_error)?;
                    visit(key.value(), value.value())?;
                }
                Ok(())
            }
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(()),
            Err(e) => Err(map_table_error(e)),
        }
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let txn = self.begin_read()?;
        match txn.open_table(def) {
            Ok(tbl) => tbl.is_empty().map_err(map_storage_error),
            Err(redb::TableError::TableDoesNotExist(_)) => Ok(true),
            Err(e) => Err(map_table_error(e)),
        }
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let txn = match &self.inner {
            RedbInner::ReadWrite(db) => db.begin_write().map_err(map_transaction_error)?,
            RedbInner::ReadOnly(_) => {
                return Err(StoreError::Backend("store is read-only".into()));
            }
        };
        let mut wtxn = RedbWriteTxn { txn: &txn };
        let result = f(&mut wtxn)?;
        txn.commit().map_err(map_commit_error)?;
        Ok(result)
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        match &mut self.inner {
            RedbInner::ReadWrite(db) => {
                let _ = db.compact().map_err(map_compaction_error)?;
                Ok(())
            }
            RedbInner::ReadOnly(_) => Err(StoreError::Backend("store is read-only".into())),
        }
    }
}

// ── redb write-transaction wrapper ─────────────────────────────────

struct RedbWriteTxn<'txn> {
    txn: &'txn redb::WriteTransaction,
}

impl WriteTxn for RedbWriteTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        Ok(tbl
            .get(key)
            .map_err(map_storage_error)?
            .map(|g| g.value().to_vec()))
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let mut tbl = self.txn.open_table(def).map_err(map_table_error)?;
        tbl.insert(key, value).map_err(map_storage_error)?;
        Ok(())
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let mut tbl = self.txn.open_table(def).map_err(map_table_error)?;
        tbl.remove(key).map_err(map_storage_error)?;
        Ok(())
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        for item in tbl.range::<&[u8]>((lo, hi)).map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        let def: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        for item in tbl.iter().map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }
}

/// The default store backend.
pub type DefaultStore = RedbStore;

// ── error mapping ──────────────────────────────────────────────────

fn map_database_error(e: redb::DatabaseError) -> StoreError {
    match e {
        redb::DatabaseError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

fn map_transaction_error(e: redb::TransactionError) -> StoreError {
    match e {
        redb::TransactionError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

fn map_table_error(e: redb::TableError) -> StoreError {
    match e {
        redb::TableError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

fn map_storage_error(e: redb::StorageError) -> StoreError {
    match e {
        redb::StorageError::Io(io) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

fn map_commit_error(e: redb::CommitError) -> StoreError {
    match e {
        redb::CommitError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

fn map_compaction_error(e: redb::CompactionError) -> StoreError {
    match e {
        redb::CompactionError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("oxpinyin-store-{tag}-{}.redb", std::process::id(),));
        let _ = std::fs::remove_file(&path);
        path
    }

    #[test]
    fn multi_table_write() {
        let path = temp_path("multi-table");
        let store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("alpha", b"k1", b"v1")?;
                txn.put("beta", b"k2", b"v2")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(store.get("alpha", b"k1").unwrap(), Some(b"v1".to_vec()));
        assert_eq!(store.get("beta", b"k2").unwrap(), Some(b"v2".to_vec()));
        assert_eq!(store.get("alpha", b"k2").unwrap(), None);
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn atomic_rollback() {
        let path = temp_path("rollback");
        let store = RedbStore::create(&path).unwrap();
        let result: Result<(), _> = store.write(|txn| {
            txn.put("t", b"a", b"1")?;
            txn.put("t", b"b", b"2")?;
            txn.put("u", b"c", b"3")?;
            Err(StoreError::Backend("deliberate rollback".into()))
        });
        assert!(result.is_err());
        assert_eq!(store.get("t", b"a").unwrap(), None);
        assert_eq!(store.get("t", b"b").unwrap(), None);
        assert_eq!(store.get("u", b"c").unwrap(), None);
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_your_writes() {
        let path = temp_path("ryw");
        let store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"key", b"val")?;
                assert_eq!(txn.get("t", b"key")?, Some(b"val".to_vec()));
                Ok(())
            })
            .unwrap();
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn range_included_included() {
        let path = temp_path("range-ii");
        let store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"a", b"1")?;
                txn.put("t", b"b", b"2")?;
                txn.put("t", b"c", b"3")?;
                txn.put("t", b"d", b"4")?;
                Ok(())
            })
            .unwrap();
        let mut rows = Vec::new();
        store
            .range(
                "t",
                Bound::Included(b"b".as_slice()),
                Bound::Included(b"c".as_slice()),
                &mut |k, v| {
                    rows.push((k.to_vec(), v.to_vec()));
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, b"b");
        assert_eq!(rows[0].1, b"2");
        assert_eq!(rows[1].0, b"c");
        assert_eq!(rows[1].1, b"3");
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn range_included_unbounded() {
        let path = temp_path("range-iu");
        let store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"a", b"1")?;
                txn.put("t", b"b", b"2")?;
                txn.put("t", b"c", b"3")?;
                Ok(())
            })
            .unwrap();
        let mut keys = Vec::new();
        store
            .range(
                "t",
                Bound::Included(b"b".as_slice()),
                Bound::Unbounded,
                &mut |k, _v| {
                    keys.push(k.to_vec());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(keys, vec![b"b".to_vec(), b"c".to_vec()]);
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn remove_in_write() {
        let path = temp_path("remove");
        let store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"k", b"v")?;
                txn.remove("t", b"k")?;
                Ok(())
            })
            .unwrap();
        assert_eq!(store.get("t", b"k").unwrap(), None);
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn is_empty_lifecycle() {
        let path = temp_path("empty");
        let store = RedbStore::create(&path).unwrap();
        assert!(store.is_empty("t").unwrap());
        store
            .write(|txn| {
                txn.put("t", b"k", b"v")?;
                Ok(())
            })
            .unwrap();
        assert!(!store.is_empty("t").unwrap());
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_returns_closure_value() {
        let path = temp_path("write-ret");
        let store = RedbStore::create(&path).unwrap();
        let val: u32 = store
            .write(|txn| {
                txn.put("t", b"k", b"v")?;
                Ok(42u32)
            })
            .unwrap();
        assert_eq!(val, 42);
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_only_reopen_and_rejection() {
        let path = temp_path("read-only");
        {
            let store = RedbStore::create(&path).unwrap();
            store
                .write(|txn| {
                    txn.put("t", b"a", b"1")?;
                    txn.put("t", b"b", b"2")?;
                    Ok(())
                })
                .unwrap();
        }
        let mut ro = RedbStore::open_read_only(&path).unwrap();
        assert_eq!(ro.get("t", b"a").unwrap(), Some(b"1".to_vec()));
        assert_eq!(ro.get("t", b"zz").unwrap(), None);
        assert!(!ro.is_empty("t").unwrap());
        let rejected: Result<(), _> = ro.write(|_txn| Ok(()));
        assert!(rejected.is_err());
        assert!(ro.compact().is_err());
        drop(ro);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn store_for_each_rows_in_order() {
        let path = temp_path("for-each");
        let store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"c", b"3")?;
                txn.put("t", b"a", b"1")?;
                txn.put("t", b"b", b"2")?;
                Ok(())
            })
            .unwrap();
        let mut rows = Vec::new();
        store
            .for_each("t", &mut |k, v| {
                rows.push((k.to_vec(), v.to_vec()));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            rows,
            vec![
                (b"a".to_vec(), b"1".to_vec()),
                (b"b".to_vec(), b"2".to_vec()),
                (b"c".to_vec(), b"3".to_vec()),
            ]
        );
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn write_txn_scans_see_own_writes_in_order() {
        let path = temp_path("wtxn-scan");
        let store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"d", b"4")?;
                txn.put("t", b"b", b"2")?;
                txn.put("t", b"a", b"1")?;
                txn.put("t", b"c", b"3")?;
                let mut mid = Vec::new();
                txn.range(
                    "t",
                    Bound::Included(b"b".as_slice()),
                    Bound::Included(b"c".as_slice()),
                    &mut |k, v| {
                        mid.push((k.to_vec(), v.to_vec()));
                        Ok(())
                    },
                )?;
                assert_eq!(
                    mid,
                    vec![
                        (b"b".to_vec(), b"2".to_vec()),
                        (b"c".to_vec(), b"3".to_vec())
                    ]
                );
                let mut keys = Vec::new();
                txn.for_each("t", &mut |k, _v| {
                    keys.push(k.to_vec());
                    Ok(())
                })?;
                assert_eq!(
                    keys,
                    vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec(), b"d".to_vec()]
                );
                Ok(())
            })
            .unwrap();
        drop(store);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn compact_preserves_data() {
        let path = temp_path("compact");
        let mut store = RedbStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"k", b"v")?;
                Ok(())
            })
            .unwrap();
        store.compact().unwrap();
        assert_eq!(store.get("t", b"k").unwrap(), Some(b"v".to_vec()));
        drop(store);
        let _ = std::fs::remove_file(&path);
    }
}
