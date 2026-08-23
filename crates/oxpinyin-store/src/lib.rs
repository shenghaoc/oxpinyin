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

use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableHandle};

/// Errors that can occur when opening or scanning a store.
///
/// The variants preserve the I/O-vs-other distinction without naming any
/// backend type, so callers can branch on I/O failures independently of
/// which backend produced them.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// An I/O error from the underlying storage layer.
    Io(std::io::Error),
    /// A non-I/O error from the storage backend (type-erased).
    Backend(Box<dyn std::error::Error + Send + Sync>),
    /// A mutation was requested through a read-only store.
    ReadOnly,
    /// A public input cannot be represented by the selected backend.
    InvalidInput(&'static str),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "store I/O error: {e}"),
            Self::Backend(e) => write!(f, "store error: {e}"),
            Self::ReadOnly => f.write_str("store is read-only"),
            Self::InvalidInput(message) => write!(f, "invalid store input: {message}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Backend(e) => Some(e.as_ref()),
            Self::ReadOnly | Self::InvalidInput(_) => None,
        }
    }
}

/// Row visitor passed to store scan methods.
pub type Visitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), StoreError> + 'a;

// ── traits ─────────────────────────────────────────────────────────

/// A consistent point-in-time read view, storable across calls.
///
/// The snapshot is taken from a single read transaction and is
/// MVCC-isolated: writes committed after [`OrderedStore::snapshot`]
/// are invisible through it.  Methods open the named table per call on
/// the held transaction (no pre-opened table set, no owned-copy slurp).
pub trait ReadSnapshot {
    /// Read a single key from `table`.  Returns `None` if absent or the
    /// table does not exist.
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Visit rows of `table` whose keys fall in the `[lo, hi]` range,
    /// ascending.  An absent table is treated as empty.
    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError>;

    /// Visit every row of `table` in ascending key-byte order.  An
    /// absent table is treated as empty.
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError>;

    /// Whether `table` has no rows (an absent table counts as empty).
    fn is_empty(&self, table: &str) -> Result<bool, StoreError>;
}

/// An atomic write transaction over one or more tables.
///
/// Operations see their own writes (read-your-writes within the closure).
/// All modifications land atomically on commit or are fully rolled back.
/// Used as `&mut dyn WriteTxn`.
///
/// Opening a table inside a write transaction creates it when absent, so
/// read-side methods ([`WriteTxn::get`], [`WriteTxn::range`],
/// [`WriteTxn::for_each`], [`WriteTxn::is_empty`]) never observe a
/// missing-table condition: an untouched table reads as empty.
pub trait WriteTxn {
    /// Read a single key from `table`.  Returns `None` if absent.
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Insert or overwrite `key` → `value` in `table`.
    ///
    /// Backends may bound key length by their storage format; the LMDB
    /// backend rejects keys outside 1..=511 bytes with
    /// [`StoreError::InvalidInput`], while the redb backend has no such
    /// limit.
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

    /// Whether `table` has no rows (an absent table counts as empty).
    ///
    /// Implementations must stop at the first row instead of scanning
    /// the whole table.
    fn is_empty(&self, table: &str) -> Result<bool, StoreError>;
}

/// An ordered byte-KV store.
///
/// Implementations provide both read-only and read-write access, point
/// get, ranged scans, atomic multi-table writes, and compaction.
pub trait OrderedStore {
    /// A consistent point-in-time read view, storable across calls.
    type ReadSnapshot: ReadSnapshot;

    /// Open a consistent read snapshot of the current store state.
    ///
    /// The returned handle owns a single read transaction and is
    /// MVCC-isolated: writes committed after this call are invisible
    /// through it.  The caller may store it in a struct field and
    /// reuse it across many calls.
    ///
    /// Retaining the snapshot delays page reclamation: freed pages
    /// cannot be reclaimed and [`OrderedStore::compact`] fails until
    /// the snapshot is dropped.
    fn snapshot(&self) -> Result<Self::ReadSnapshot, StoreError>;

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
    /// borrowing each row (no per-row clone).  Stops early on visitor
    /// `Err`.  An absent table is treated as empty.
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
    ///
    /// The closure must not call [`OrderedStore::write`] again. Backends may
    /// serialize write transactions, so a nested call can block forever.
    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError>;

    /// Perform backend-dependent compaction work.
    ///
    /// redb rewrites the file and reclaims free pages. LMDB reuses freed pages
    /// in place, so its successful implementation does not shrink the file.
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

pub(crate) fn validate_table_name(table: &str) -> Result<(), StoreError> {
    if table.is_empty() {
        return Err(StoreError::InvalidInput("empty table name"));
    }
    if table.contains('\0') {
        return Err(StoreError::InvalidInput("table name contains NUL"));
    }
    Ok(())
}

fn table_def<'a>(
    table: &'a str,
) -> Result<redb::TableDefinition<'a, &'static [u8], &'static [u8]>, StoreError> {
    validate_table_name(table)?;
    Ok(redb::TableDefinition::new(table))
}

impl RedbStore {
    fn begin_read(&self) -> Result<redb::ReadTransaction, StoreError> {
        match &self.inner {
            RedbInner::ReadOnly(db) => db.begin_read().map_err(map_transaction_error),
            RedbInner::ReadWrite(db) => db.begin_read().map_err(map_transaction_error),
        }
    }
}

// ── redb shared read helpers ───────────────────────────────────────

/// An absent table is treated as empty (`None`).
fn read_get(
    txn: &redb::ReadTransaction,
    table: &str,
    key: &[u8],
) -> Result<Option<Vec<u8>>, StoreError> {
    let def = table_def(table)?;
    match txn.open_table(def) {
        Ok(tbl) => Ok(tbl
            .get(key)
            .map_err(map_storage_error)?
            .map(|g| g.value().to_vec())),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
        Err(e) => Err(map_table_error(e)),
    }
}

/// An absent table is treated as empty (no visits).
fn read_range(
    txn: &redb::ReadTransaction,
    table: &str,
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
    visit: &mut Visitor<'_>,
) -> Result<(), StoreError> {
    let def = table_def(table)?;
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

/// An absent table is treated as empty (no visits).
fn read_for_each(
    txn: &redb::ReadTransaction,
    table: &str,
    visit: &mut Visitor<'_>,
) -> Result<(), StoreError> {
    let def = table_def(table)?;
    match txn.open_table(def) {
        Ok(tbl) => {
            for item in tbl.iter().map_err(map_storage_error)? {
                let (key, value) = item.map_err(map_storage_error)?;
                visit(key.value(), value.value())?;
            }
            Ok(())
        }
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(()),
        Err(e) => Err(map_table_error(e)),
    }
}

/// An absent table counts as empty.
fn read_is_empty(txn: &redb::ReadTransaction, table: &str) -> Result<bool, StoreError> {
    let def = table_def(table)?;
    match txn.open_table(def) {
        Ok(tbl) => tbl.is_empty().map_err(map_storage_error),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(true),
        Err(e) => Err(map_table_error(e)),
    }
}

impl OrderedStore for RedbStore {
    type ReadSnapshot = RedbReadSnapshot;

    fn snapshot(&self) -> Result<RedbReadSnapshot, StoreError> {
        let txn = self.begin_read()?;
        Ok(RedbReadSnapshot { txn })
    }

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
        let txn = self.begin_read()?;
        read_get(&txn, table, key)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        let txn = self.begin_read()?;
        read_for_each(&txn, table, visit)
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let txn = self.begin_read()?;
        read_range(&txn, table, lo, hi, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let txn = self.begin_read()?;
        read_is_empty(&txn, table)
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let txn = match &self.inner {
            RedbInner::ReadWrite(db) => db.begin_write().map_err(map_transaction_error)?,
            RedbInner::ReadOnly(_) => return Err(StoreError::ReadOnly),
        };
        let result = {
            let mut wtxn = RedbWriteTxn { txn: &txn };
            f(&mut wtxn)
        };
        match result {
            Ok(result) => {
                txn.commit().map_err(map_commit_error)?;
                Ok(result)
            }
            Err(error) => {
                let _ = txn.abort();
                Err(error)
            }
        }
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        match &mut self.inner {
            RedbInner::ReadWrite(db) => {
                let _ = db.compact().map_err(map_compaction_error)?;
                Ok(())
            }
            RedbInner::ReadOnly(_) => Err(StoreError::ReadOnly),
        }
    }
}

// ── redb read-snapshot ─────────────────────────────────────────────

/// A stored, consistent read snapshot backed by a redb read transaction.
///
/// Created by [`RedbStore::snapshot`].  The held transaction is
/// MVCC-isolated: writes committed after the snapshot was taken are
/// invisible through it.
pub struct RedbReadSnapshot {
    txn: redb::ReadTransaction,
}

impl ReadSnapshot for RedbReadSnapshot {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        read_get(&self.txn, table, key)
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        read_range(&self.txn, table, lo, hi, visit)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        read_for_each(&self.txn, table, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        read_is_empty(&self.txn, table)
    }
}

// ── redb write-transaction wrapper ─────────────────────────────────

struct RedbWriteTxn<'txn> {
    txn: &'txn redb::WriteTransaction,
}

impl WriteTxn for RedbWriteTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let def = table_def(table)?;
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        Ok(tbl
            .get(key)
            .map_err(map_storage_error)?
            .map(|g| g.value().to_vec()))
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        let def = table_def(table)?;
        let mut tbl = self.txn.open_table(def).map_err(map_table_error)?;
        tbl.insert(key, value).map_err(map_storage_error)?;
        Ok(())
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        let def = table_def(table)?;
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
        let def = table_def(table)?;
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        for item in tbl.range::<&[u8]>((lo, hi)).map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        // Write-transaction table opens create the table when absent, so
        // `TableDoesNotExist` cannot occur here (see the trait docs).
        let def = table_def(table)?;
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        for item in tbl.iter().map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        // `open_table` creates an absent table; probe existence first so an
        // emptiness check never changes the schema.
        let exists = self
            .txn
            .list_tables()
            .map_err(map_storage_error)?
            .any(|handle| handle.name() == table);
        let def = table_def(table)?;
        if !exists {
            return Ok(true);
        }
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        tbl.is_empty().map_err(map_storage_error)
    }
}

/// The default store backend.
pub type DefaultStore = RedbStore;

#[cfg(feature = "lmdb")]
mod lmdb;
#[cfg(feature = "lmdb")]
pub use lmdb::{LmdbReadSnapshot, LmdbStore};

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
    #[cfg(feature = "lmdb")]
    use super::{LmdbStore, OrderedStore, StoreError};

    macro_rules! store_tests {
        ($mod:ident, $store:ty, $ext:literal) => {
            mod $mod {
                use super::super::*;

                fn temp_path(tag: &str) -> std::path::PathBuf {
                    let path = std::env::temp_dir().join(format!(
                        "oxpinyin-store-{}-{tag}-{}.{}",
                        stringify!($mod),
                        std::process::id(),
                        $ext,
                    ));
                    cleanup(&path);
                    path
                }

                fn cleanup(path: &std::path::Path) {
                    let _ = std::fs::remove_file(path);
                    let lock = format!("{}-lock", path.display());
                    let _ = std::fs::remove_file(&lock);
                }

                #[test]
                fn multi_table_write() {
                    let path = temp_path("multi-table");
                    let store = <$store>::create(&path).unwrap();
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
                    cleanup(&path);
                }

                #[test]
                fn atomic_rollback() {
                    let path = temp_path("rollback");
                    let store = <$store>::create(&path).unwrap();
                    let result: Result<(), _> = store.write(|txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        txn.put("u", b"c", b"3")?;
                        Err(StoreError::Backend("deliberate rollback".into()))
                    });
                    assert!(result.is_err());
                    assert!(matches!(
                        result,
                        Err(StoreError::Backend(ref e)) if e.to_string() == "deliberate rollback"
                    ));
                    assert_eq!(store.get("t", b"a").unwrap(), None);
                    assert_eq!(store.get("t", b"b").unwrap(), None);
                    assert_eq!(store.get("u", b"c").unwrap(), None);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn read_your_writes() {
                    let path = temp_path("ryw");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"key", b"val")?;
                            assert_eq!(txn.get("t", b"key")?, Some(b"val".to_vec()));
                            Ok(())
                        })
                        .unwrap();
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn compact_fails_while_snapshot_open() {
                    let path = temp_path("compact-snap");
                    let mut store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| txn.put("t", b"k", b"v"))
                        .unwrap();
                    let snap = store.snapshot().unwrap();
                    assert_eq!(snap.get("t", b"k").unwrap(), Some(b"v".to_vec()));
                    assert!(
                        store.compact().is_err(),
                        "compact must fail while a snapshot is open"
                    );
                    drop(snap);
                    store.compact().unwrap();
                    assert_eq!(store.get("t", b"k").unwrap(), Some(b"v".to_vec()));
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn write_txn_is_empty() {
                    let path = temp_path("wtxn-empty");
                    let store = <$store>::create(&path).unwrap();
                    // An absent table counts as empty and must not be created
                    // by the probe.
                    assert!(store.write(|txn| txn.is_empty("t")).unwrap());
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            assert!(!txn.is_empty("t")?);
                            txn.remove("t", b"k")?;
                            assert!(txn.is_empty("t")?);
                            Ok(())
                        })
                        .unwrap();
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn range_included_included() {
                    let path = temp_path("range-ii");
                    let store = <$store>::create(&path).unwrap();
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
                    cleanup(&path);
                }

                #[test]
                fn range_included_unbounded() {
                    let path = temp_path("range-iu");
                    let store = <$store>::create(&path).unwrap();
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
                    cleanup(&path);
                }

                #[test]
                fn remove_in_write() {
                    let path = temp_path("remove");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            txn.remove("t", b"k")?;
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(store.get("t", b"k").unwrap(), None);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn is_empty_lifecycle() {
                    let path = temp_path("empty");
                    let store = <$store>::create(&path).unwrap();
                    assert!(store.is_empty("t").unwrap());
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            Ok(())
                        })
                        .unwrap();
                    assert!(!store.is_empty("t").unwrap());
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn missing_table_scans_are_empty_and_excluded_empty_bound_is_safe() {
                    let path = temp_path("missing-scan");
                    let store = <$store>::create(&path).unwrap();
                    let mut rows = Vec::new();
                    store
                        .for_each("missing", &mut |key, value| {
                            rows.push((key.to_vec(), value.to_vec()));
                            Ok(())
                        })
                        .unwrap();
                    assert!(rows.is_empty());

                    store
                        .write(|txn| {
                            txn.put("t", b"a", b"1")?;
                            Ok(())
                        })
                        .unwrap();
                    store
                        .range(
                            "t",
                            Bound::Excluded(&[]),
                            Bound::Unbounded,
                            &mut |key, _| {
                                rows.push((key.to_vec(), Vec::new()));
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(rows, vec![(b"a".to_vec(), Vec::new())]);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn empty_bounds_never_match_or_error() {
                    let path = temp_path("empty-bounds");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"a", b"1")?;
                            txn.put("t", b"b", b"2")?;
                            Ok(())
                        })
                        .unwrap();

                    let mut keys = Vec::new();
                    store
                        .range(
                            "t",
                            Bound::Included(&[]),
                            Bound::Unbounded,
                            &mut |k, _| {
                                keys.push(k.to_vec());
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);

                    for hi in [Bound::<&[u8]>::Excluded(&[]), Bound::Included(&[])] {
                        let mut upper_empty_keys = Vec::new();
                        store
                            .range("t", Bound::Unbounded, hi, &mut |k, _| {
                                upper_empty_keys.push(k.to_vec());
                                Ok(())
                            })
                            .unwrap();
                        assert!(upper_empty_keys.is_empty());
                    }

                    store
                        .write(|txn| {
                            txn.range(
                                "t",
                                Bound::Included(&[]),
                                Bound::Excluded(&[]),
                                &mut |_, _| Ok(()),
                            )
                        })
                        .unwrap();
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn empty_and_nul_table_names_are_rejected_everywhere() {
                    fn assert_invalid<T>(result: Result<T, StoreError>) {
                        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
                    }

                    let path = temp_path("invalid-table");
                    let store = <$store>::create(&path).unwrap();
                    for table in ["", "bad\0name"] {
                        assert_invalid(store.get(table, b"key"));
                        assert_invalid(store.for_each(table, &mut |_, _| Ok(())));
                        assert_invalid(store.range(
                            table,
                            Bound::Unbounded,
                            Bound::Unbounded,
                            &mut |_, _| Ok(()),
                        ));
                        assert_invalid(store.is_empty(table));

                        let snapshot = store.snapshot().unwrap();
                        assert_invalid(snapshot.get(table, b"key"));
                        assert_invalid(snapshot.for_each(table, &mut |_, _| Ok(())));
                        assert_invalid(snapshot.range(
                            table,
                            Bound::Unbounded,
                            Bound::Unbounded,
                            &mut |_, _| Ok(()),
                        ));
                        assert_invalid(snapshot.is_empty(table));

                        assert_invalid(store.write(|txn| txn.get(table, b"key")));
                        assert_invalid(store.write(|txn| txn.put(table, b"key", b"value")));
                        assert_invalid(store.write(|txn| txn.remove(table, b"key")));
                        assert_invalid(store.write(|txn| {
                            txn.range(
                                table,
                                Bound::Unbounded,
                                Bound::Unbounded,
                                &mut |_, _| Ok(()),
                            )
                        }));
                        assert_invalid(store.write(|txn| txn.for_each(table, &mut |_, _| Ok(()))));
                        assert_invalid(store.write(|txn| txn.is_empty(table)));
                    }
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn read_only_reopen_preserves_reads_and_rejects_mutation() {
                    let path = temp_path("read-only");
                    let store = <$store>::create(&path).unwrap();
                    store.write(|txn| txn.put("t", b"key", b"value")).unwrap();
                    drop(store);

                    let mut readonly = <$store>::open_read_only(&path).unwrap();
                    assert_eq!(readonly.get("t", b"key").unwrap(), Some(b"value".to_vec()));
                    assert!(matches!(
                        readonly.write(|_| Ok(())),
                        Err(StoreError::ReadOnly)
                    ));
                    assert!(matches!(readonly.compact(), Err(StoreError::ReadOnly)));
                    drop(readonly);
                    cleanup(&path);
                }

                #[test]
                fn write_returns_closure_value() {
                    let path = temp_path("write-ret");
                    let store = <$store>::create(&path).unwrap();
                    let val: u32 = store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            Ok(42u32)
                        })
                        .unwrap();
                    assert_eq!(val, 42);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn compact_preserves_data() {
                    let path = temp_path("compact");
                    let mut store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            Ok(())
                        })
                        .unwrap();
                    store.compact().unwrap();
                    assert_eq!(store.get("t", b"k").unwrap(), Some(b"v".to_vec()));
                    drop(store);
                    cleanup(&path);
                }

                // ── snapshot tests ─────────────────────────────────

                #[test]
                fn snapshot_consistency() {
                    let path = temp_path("snap-consist");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v1")?;
                            Ok(())
                        })
                        .unwrap();
                    let snap = store.snapshot().unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v2")?;
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(snap.get("t", b"k").unwrap(), Some(b"v1".to_vec()));
                    assert_eq!(store.get("t", b"k").unwrap(), Some(b"v2".to_vec()));
                    drop(snap);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn snapshot_reuse() {
                    let path = temp_path("snap-reuse");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            Ok(())
                        })
                        .unwrap();
                    let snap = store.snapshot().unwrap();
                    assert_eq!(snap.get("t", b"k").unwrap(), Some(b"v".to_vec()));
                    assert_eq!(snap.get("t", b"k").unwrap(), Some(b"v".to_vec()));
                    drop(snap);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn snapshot_correctness() {
                    let path = temp_path("snap-correct");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"a", b"1")?;
                            txn.put("t", b"b", b"2")?;
                            txn.put("t", b"c", b"3")?;
                            txn.put("u", b"x", b"9")?;
                            Ok(())
                        })
                        .unwrap();
                    let snap = store.snapshot().unwrap();

                    assert_eq!(snap.get("t", b"a").unwrap(), Some(b"1".to_vec()));
                    assert_eq!(snap.get("t", b"missing").unwrap(), None);
                    assert_eq!(snap.get("u", b"x").unwrap(), Some(b"9".to_vec()));

                    assert!(!snap.is_empty("t").unwrap());
                    assert!(snap.is_empty("nonexistent").unwrap());

                    let mut keys = Vec::new();
                    snap.for_each("t", &mut |k, _v| {
                        keys.push(k.to_vec());
                        Ok(())
                    })
                    .unwrap();
                    assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec(), b"c".to_vec()]);

                    let mut range_keys = Vec::new();
                    snap.range(
                        "t",
                        Bound::Included(b"b".as_slice()),
                        Bound::Included(b"c".as_slice()),
                        &mut |k, _v| {
                            range_keys.push(k.to_vec());
                            Ok(())
                        },
                    )
                    .unwrap();
                    assert_eq!(range_keys, vec![b"b".to_vec(), b"c".to_vec()]);

                    let mut range_unbound = Vec::new();
                    snap.range(
                        "t",
                        Bound::Included(b"b".as_slice()),
                        Bound::Unbounded,
                        &mut |k, _v| {
                            range_unbound.push(k.to_vec());
                            Ok(())
                        },
                    )
                    .unwrap();
                    assert_eq!(range_unbound, vec![b"b".to_vec(), b"c".to_vec()]);

                    drop(snap);
                    drop(store);
                    cleanup(&path);
                }
            }
        };
    }

    store_tests!(redb, RedbStore, "redb");

    #[cfg(feature = "lmdb")]
    store_tests!(lmdb, LmdbStore, "lmdb");

    #[test]
    fn redb_is_empty_probe_does_not_create_tables() {
        use ::redb::ReadableDatabase;
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-wtxn-probe-{}.redb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let store = crate::RedbStore::create(&path).unwrap();
        assert!(store.write(|txn| txn.is_empty("t")).unwrap());
        drop(store);
        // A committed write transaction that only probed emptiness must
        // leave the database with zero tables; assert it against redb
        // directly because the OrderedStore API cannot distinguish an
        // absent table from an empty one.  (`::redb` because the shared
        // test suite generates a `tests::redb` submodule that shadows
        // the crate path.)
        let db = ::redb::ReadOnlyDatabase::open(&path).unwrap();
        let txn = db.begin_read().unwrap();
        assert_eq!(txn.list_tables().unwrap().count(), 0);
        drop(txn);
        drop(db);
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_key_lengths_lmdb_cannot_store() {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-keylen-{}.mdb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let lock: std::path::PathBuf = {
            let mut l = path.clone().into_os_string();
            l.push("-lock");
            l.into()
        };
        let store = LmdbStore::create(&path).unwrap();
        assert!(matches!(
            store.write(|txn| txn.put("t", b"", b"v")),
            Err(StoreError::InvalidInput("key length must be 1..=511 bytes"))
        ));
        let long = [b'k'; 512];
        assert!(matches!(
            store.write(|txn| txn.put("t", &long, b"v")),
            Err(StoreError::InvalidInput("key length must be 1..=511 bytes"))
        ));
        let boundary = [b'k'; 511];
        store
            .write(|txn| txn.put("t", &boundary, b"v"))
            .expect("511-byte keys are accepted");
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_unaligned_map_size() {
        // Absolute: heed resolves the path before validating map-size
        // alignment, and a relative path fails with NotFound first.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-unaligned-{}.mdb",
            std::process::id(),
        ));
        // 3 is not a multiple of any real system page size.
        assert!(matches!(
            LmdbStore::create_with_map_size(&path, 3),
            Err(StoreError::InvalidInput(
                "map size must be a multiple of the system page size"
            ))
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_zero_map_size() {
        let path = std::path::PathBuf::from("oxpinyin-store-zero-map.mdb");
        assert!(matches!(
            LmdbStore::create_with_map_size(&path, 0),
            Err(StoreError::InvalidInput("map size must be nonzero"))
        ));
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_nul_path() {
        let path = std::path::PathBuf::from("oxpinyin-store\0invalid.mdb");
        assert!(matches!(
            LmdbStore::create(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
        assert!(matches!(
            LmdbStore::open_read_only(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
    }
}
