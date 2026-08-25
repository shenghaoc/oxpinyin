//! Backend-agnostic ordered key–value store for oxpinyin tables.
//!
//! This crate defines an ordered byte-KV interface split into two
//! capability tiers — [`ReadStore`] (point get, ranged scan, full scan,
//! emptiness check) and [`WriteStore`] (creation, atomic multi-table
//! writes, compaction) — and provides a [`RedbStore`] implementation
//! backed by redb that offers both.  Consumers depend on the narrowest
//! tier they need; the concrete backend is selected by the
//! [`DefaultStore`] alias.

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
    ///
    /// Backends may also bound the number of distinct tables: the LMDB
    /// backend caps a store at 32 named tables and rejects writes to a 33rd
    /// with [`StoreError::InvalidInput`], while the redb backend has no such
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

/// The read tier of an ordered byte-KV store.
///
/// This is the base capability every backend provides: open an existing
/// file read-only, point get, ranged scan, full scan, and an emptiness
/// check.  A backend that can only read — no writer at all —
/// implements this and nothing else, and the system-table loader binds
/// to exactly this tier.
pub trait ReadStore {
    /// Open an existing store file in read-only mode.
    fn open_read_only(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

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

    /// Visit every `(key, value)` of `table` in ascending key-byte order,
    /// borrowing each row (no per-row clone).  Stops early on visitor
    /// `Err`.  An absent table is treated as empty.
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError>;

    /// Whether `table` has no rows (an absent table counts as empty).
    fn is_empty(&self, table: &str) -> Result<bool, StoreError>;
}

/// The write tier: creation, atomic multi-table writes, and compaction.
///
/// Adds mutation on top of [`ReadStore`], whose reads a writable backend
/// necessarily also provides.
pub trait WriteStore: ReadStore {
    /// Open or create a store file in read-write mode.
    fn create(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Run `f` inside an atomic write transaction.  All puts/removes in
    /// `f` land together on `Ok`, or none land on `Err` (full rollback).
    /// The closure sees its own writes.
    ///
    /// The closure must not call [`WriteStore::write`] again. Backends may
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

/// A redb-backed store implementing both capability tiers.
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

impl ReadStore for RedbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let db = redb::Builder::new()
            .open_read_only(path)
            .map_err(map_database_error)?;
        Ok(Self {
            inner: RedbInner::ReadOnly(db),
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
}

impl WriteStore for RedbStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        let db = redb::Database::create(path).map_err(map_database_error)?;
        Ok(Self {
            inner: RedbInner::ReadWrite(db),
        })
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
pub use lmdb::LmdbStore;

#[cfg(feature = "tkrzw")]
mod tkrzw;
#[cfg(feature = "tkrzw")]
pub use tkrzw::TkrzwStore;

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
    // `WriteStore` provides `create`/`write`, used by the always-built
    // `redb_is_empty_probe_does_not_create_tables` below; keep it in scope
    // regardless of the backend features. Everything else here is only
    // referenced by a feature-gated backend test, so each import is gated
    // the same way to avoid an unused-import warning when it is off.
    #[cfg(feature = "tkrzw")]
    use std::ops::Bound;

    #[cfg(feature = "lmdb")]
    use super::LmdbStore;
    #[cfg(any(feature = "lmdb", feature = "tkrzw"))]
    use super::ReadStore;
    #[cfg(any(feature = "lmdb", feature = "tkrzw"))]
    use super::StoreError;
    #[cfg(feature = "tkrzw")]
    use super::TkrzwStore;
    use super::WriteStore;

    /// Emits the temp-path plumbing every tier group needs.
    ///
    /// Invoked from inside each group's generated module, so the three
    /// groups do not have to triplicate it.
    macro_rules! store_test_paths {
        ($mod:ident, $ext:literal) => {
            /// Owns a temporary store path and removes both the data
            /// file and its `-lock` sidecar when it drops — including
            /// when a test panics, so a failed assertion leaves no
            /// state behind in `std::env::temp_dir()`. Derefs to
            /// `Path`, so `&guard` passes anywhere a `&Path` is wanted.
            struct TempPath(std::path::PathBuf);

            impl std::ops::Deref for TempPath {
                type Target = std::path::Path;
                fn deref(&self) -> &std::path::Path {
                    &self.0
                }
            }

            impl Drop for TempPath {
                fn drop(&mut self) {
                    cleanup(&self.0);
                }
            }

            fn temp_path(tag: &str) -> TempPath {
                let path = std::env::temp_dir().join(format!(
                    "oxpinyin-store-{}-{tag}-{}.{}",
                    stringify!($mod),
                    std::process::id(),
                    $ext,
                ));
                cleanup(&path);
                TempPath(path)
            }

            fn cleanup(path: &std::path::Path) {
                let _ = std::fs::remove_file(path);
                let lock = format!("{}-lock", path.display());
                let _ = std::fs::remove_file(&lock);
            }
        };
    }

    /// The read tier: everything a bare [`ReadStore`] must answer.
    ///
    /// `$store` is exercised through [`ReadStore`] alone. `$writer` is a
    /// [`WriteStore`] used only to lay down the fixture file that `$store`
    /// then opens read-only — a read-only backend supplies whichever
    /// writer produces its file format, and runs this group unchanged
    /// without gaining a write capability.
    macro_rules! store_read_tests {
        ($mod:ident, $store:ty, $writer:ty, $ext:literal) => {
            mod $mod {
                use super::super::*;

                store_test_paths!($mod, $ext);

                /// Lays down a fixture with `$writer`, then returns a
                /// read-only `$store` handle over the same file.
                fn seeded(
                    path: &std::path::Path,
                    build: impl FnOnce(&mut dyn WriteTxn) -> Result<(), StoreError>,
                ) -> $store {
                    let writer = <$writer>::create(path).unwrap();
                    writer.write(build).unwrap();
                    drop(writer);
                    <$store>::open_read_only(path).unwrap()
                }

                #[test]
                fn multi_table_get() {
                    let path = temp_path("multi-table-get");
                    let store = seeded(&path, |txn| {
                        txn.put("alpha", b"k1", b"v1")?;
                        txn.put("beta", b"k2", b"v2")?;
                        Ok(())
                    });
                    assert_eq!(store.get("alpha", b"k1").unwrap(), Some(b"v1".to_vec()));
                    assert_eq!(store.get("beta", b"k2").unwrap(), Some(b"v2".to_vec()));
                    assert_eq!(store.get("alpha", b"k2").unwrap(), None);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn range_included_included() {
                    let path = temp_path("range-ii");
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        txn.put("t", b"c", b"3")?;
                        txn.put("t", b"d", b"4")?;
                        Ok(())
                    });
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
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        txn.put("t", b"c", b"3")?;
                        Ok(())
                    });
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
                fn is_empty_lifecycle() {
                    let path = temp_path("empty");
                    let store = seeded(&path, |_| Ok(()));
                    assert!(store.is_empty("t").unwrap());
                    drop(store);

                    let store = seeded(&path, |txn| {
                        txn.put("t", b"k", b"v")?;
                        Ok(())
                    });
                    assert!(!store.is_empty("t").unwrap());
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn missing_table_scans_are_empty_and_excluded_empty_bound_is_safe() {
                    let path = temp_path("missing-scan");
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        Ok(())
                    });
                    let mut rows = Vec::new();
                    store
                        .for_each("missing", &mut |key, value| {
                            rows.push((key.to_vec(), value.to_vec()));
                            Ok(())
                        })
                        .unwrap();
                    assert!(rows.is_empty());

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
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        Ok(())
                    });

                    let mut keys = Vec::new();
                    store
                        .range("t", Bound::Included(&[]), Bound::Unbounded, &mut |k, _| {
                            keys.push(k.to_vec());
                            Ok(())
                        })
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
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn empty_and_nul_table_names_are_rejected_by_reads() {
                    fn assert_invalid<T>(result: Result<T, StoreError>) {
                        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
                    }

                    let path = temp_path("invalid-table");
                    let store = seeded(&path, |_| Ok(()));
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
                    }
                    drop(store);
                    cleanup(&path);
                }
            }
        };
    }

    /// The write tier: creation, atomic multi-table writes, compaction,
    /// and the read-only handle's refusal to mutate.
    macro_rules! store_write_tests {
        ($mod:ident, $store:ty, $ext:literal) => {
            mod $mod {
                use super::super::*;

                store_test_paths!($mod, $ext);

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
                fn write_txn_empty_bounds_never_match_or_error() {
                    let path = temp_path("wtxn-empty-bounds");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"a", b"1")?;
                            txn.put("t", b"b", b"2")?;
                            Ok(())
                        })
                        .unwrap();

                    let mut visited = Vec::new();
                    store
                        .write(|txn| {
                            txn.range(
                                "t",
                                Bound::Included(&[]),
                                Bound::Excluded(&[]),
                                &mut |key, _| {
                                    visited.push(key.to_vec());
                                    Ok(())
                                },
                            )
                        })
                        .unwrap();
                    assert!(
                        visited.is_empty(),
                        "empty bounds matched {visited:?}"
                    );
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn empty_and_nul_table_names_are_rejected_by_writes() {
                    fn assert_invalid<T>(result: Result<T, StoreError>) {
                        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
                    }

                    let path = temp_path("invalid-table");
                    let store = <$store>::create(&path).unwrap();
                    for table in ["", "bad\0name"] {
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
            }
        };
    }

    // Every backend offers both tiers, so each runs both groups and is
    // its own fixture writer. A read-only backend would invoke only
    // `store_read_tests!`.
    store_read_tests!(redb_read, RedbStore, RedbStore, "redb");
    store_write_tests!(redb_write, RedbStore, "redb");

    #[cfg(feature = "lmdb")]
    store_read_tests!(lmdb_read, LmdbStore, LmdbStore, "lmdb");
    #[cfg(feature = "lmdb")]
    store_write_tests!(lmdb_write, LmdbStore, "lmdb");

    #[cfg(feature = "tkrzw")]
    store_read_tests!(tkrzw_read, TkrzwStore, TkrzwStore, "tkrzw");
    #[cfg(feature = "tkrzw")]
    store_write_tests!(tkrzw_write, TkrzwStore, "tkrzw");

    /// Removes the borrowed path on drop, so a panicking test leaves no
    /// file behind in `std::env::temp_dir()`. redb keeps no `-lock` sidecar,
    /// so the single data file is all that needs removing.
    struct RemoveOnDrop<'a>(&'a std::path::Path);

    impl Drop for RemoveOnDrop<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }

    #[test]
    fn redb_is_empty_probe_does_not_create_tables() {
        use ::redb::ReadableDatabase;
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-wtxn-probe-{}.redb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveOnDrop(&path);
        let store = crate::RedbStore::create(&path).unwrap();
        assert!(store.write(|txn| txn.is_empty("t")).unwrap());
        drop(store);
        // A committed write transaction that only probed emptiness must
        // leave the database with zero tables; assert it against redb
        // directly because the store traits cannot distinguish an absent
        // table from an empty one.  (`::redb` names the crate
        // unambiguously alongside the generated per-tier test modules.)
        let db = ::redb::ReadOnlyDatabase::open(&path).unwrap();
        let txn = db.begin_read().unwrap();
        assert_eq!(txn.list_tables().unwrap().count(), 0);
        drop(txn);
        drop(db);
    }

    /// Removes a tkrzw store file and its `-lock` sidecar on drop, so a
    /// panicking test leaves nothing behind in `std::env::temp_dir()`.
    #[cfg(feature = "tkrzw")]
    struct RemoveTkrzw(std::path::PathBuf);

    #[cfg(feature = "tkrzw")]
    impl Drop for RemoveTkrzw {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let mut lock = self.0.clone().into_os_string();
            lock.push("-lock");
            let _ = std::fs::remove_file(std::path::PathBuf::from(lock));
        }
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_orders_keys_as_unsigned_bytes() {
        // The backend installs no custom comparator, so TreeDBM sorts by
        // its default LexicalKeyComparator. That has to be plain
        // *unsigned* byte order for oxpinyin's big-endian key codec to
        // keep the order it has under redb and LMDB — a signed-char
        // comparison would sort 0x80.. before 0x00.., silently reversing
        // every high-token range scan. ASCII fixtures cannot tell the
        // two apart, so probe the high half explicitly.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-order-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();
        let keys: [&[u8]; 5] = [&[0x00], &[0x7f], &[0x80], &[0xfe], &[0xff]];
        store
            .write(|txn| {
                // Inserted out of order, so the walk order is the store's
                // and not the insertion sequence.
                for key in [keys[2], keys[4], keys[0], keys[3], keys[1]] {
                    txn.put("t", key, b"x")?;
                }
                Ok(())
            })
            .unwrap();

        let mut walked = Vec::new();
        store
            .for_each("t", &mut |key, _value| {
                walked.push(key.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(
            walked,
            keys.iter().map(|key| key.to_vec()).collect::<Vec<_>>(),
            "TreeDBM must walk keys in ascending unsigned byte order"
        );

        // The same order must hold through a bounded scan: 0x80..=0xfe
        // is the half a signed comparison would misplace.
        let mut ranged = Vec::new();
        store
            .range(
                "t",
                Bound::Included(&[0x80][..]),
                Bound::Included(&[0xfe][..]),
                &mut |key, _value| {
                    ranged.push(key.to_vec());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(ranged, vec![vec![0x80], vec![0xfe]]);
        drop(store);
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_keeps_tables_apart_when_one_name_prefixes_another() {
        // TreeDBM is one keyspace, so the backend frames records as
        // `table || 0x00 || key`. The framing is prefix-free only
        // because table names are validated NUL-free; pin that, since a
        // regression would silently merge `a` into `ab`'s scans.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-prefix-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("a", b"k", b"in-a")?;
                txn.put("ab", b"k", b"in-ab")?;
                txn.put("a\u{1}b", b"k", b"in-a1b")?;
                Ok(())
            })
            .unwrap();

        for (table, expected) in [("a", "in-a"), ("ab", "in-ab"), ("a\u{1}b", "in-a1b")] {
            assert_eq!(
                store.get(table, b"k").unwrap(),
                Some(expected.as_bytes().to_vec())
            );
            let mut rows = 0_usize;
            store
                .for_each(table, &mut |_key, value| {
                    assert_eq!(value, expected.as_bytes());
                    rows += 1;
                    Ok(())
                })
                .unwrap();
            assert_eq!(rows, 1, "{table} must scan only its own rows");
        }
        drop(store);
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_write_txn_scan_merges_buffer_over_stored_rows() {
        // `merged_visit` lays a transaction's buffer over stored rows and
        // streams the merge in ascending key order: a buffered value
        // overrides its stored twin, a buffered tombstone hides it, and
        // buffered inserts slot in around both. Seed three rows, then
        // buffer one of each mutation — insert before a stored key, update,
        // tombstone, insert after the last stored key — and scan inside the
        // transaction before anything commits.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-merge-scan-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"b", b"stored-b")?;
                txn.put("t", b"d", b"stored-d")?;
                txn.put("t", b"f", b"stored-f")?;
                Ok(())
            })
            .unwrap();

        store
            .write(|txn| {
                txn.put("t", b"a", b"insert-a")?; // before the first stored key
                txn.put("t", b"b", b"update-b")?; // overwrites a stored key
                txn.remove("t", b"d")?; // tombstones a stored key
                txn.put("t", b"g", b"insert-g")?; // after the last stored key

                let mut rows = Vec::new();
                txn.for_each("t", &mut |key, value| {
                    rows.push((key.to_vec(), value.to_vec()));
                    Ok(())
                })?;
                assert_eq!(
                    rows,
                    vec![
                        (b"a".to_vec(), b"insert-a".to_vec()),
                        (b"b".to_vec(), b"update-b".to_vec()),
                        (b"f".to_vec(), b"stored-f".to_vec()),
                        (b"g".to_vec(), b"insert-g".to_vec()),
                    ],
                    "merged scan must be ascending, apply buffered puts, \
                     and hide the tombstoned row"
                );
                Ok(())
            })
            .unwrap();
        drop(store);
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_scan_keeps_every_borrowed_record_intact() {
        // The zero-copy walk hands the visitor a pointer into tkrzw's
        // record buffer that is valid only for that record's callback. A
        // use-after-free of that borrow corrupts LATER records, not the
        // first — the iterator invalidates or reuses the memory a prior
        // callback read. Walk many records and verify each key and value
        // in place as it is visited, so a stale read fails the assertion
        // rather than passing silently.
        fn expected_value(i: u32) -> [u8; 32] {
            let mut value = [0u8; 32];
            for (j, byte) in value.iter_mut().enumerate() {
                *byte = i.wrapping_add(j as u32) as u8;
            }
            value
        }

        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-scan-many-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();

        const RECORDS: u32 = 10_000;
        store
            .write(|txn| {
                for i in 0..RECORDS {
                    txn.put("t", &i.to_be_bytes(), &expected_value(i))?;
                }
                Ok(())
            })
            .unwrap();

        let mut seen = 0u32;
        store
            .for_each("t", &mut |key, value| {
                let i = u32::from_be_bytes(key.try_into().unwrap());
                assert_eq!(i, seen, "scan must visit keys in ascending order");
                assert_eq!(
                    value,
                    &expected_value(i),
                    "record {i} came back corrupt — borrowed buffer invalidated early"
                );
                seen += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, RECORDS, "every record must be visited exactly once");
        drop(store);
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

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_rejects_nul_path() {
        let path = std::path::PathBuf::from("oxpinyin-store\0invalid.tkrzw");
        assert!(matches!(
            TkrzwStore::create(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
        assert!(matches!(
            TkrzwStore::open_read_only(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_more_than_max_named_tables() {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-maxdbs-{}.mdb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let lock: std::path::PathBuf = {
            let mut l = path.clone().into_os_string();
            l.push("-lock");
            l.into()
        };
        let store = LmdbStore::create(&path).unwrap();
        // LMDB caps the environment at 32 named tables; writing past that
        // must surface as InvalidInput, not an opaque backend error. The
        // loop runs well beyond 32 so the exact off-by-one does not matter.
        let result = store.write(|txn| {
            for i in 0..40 {
                txn.put(&format!("t{i}"), b"k", b"v")?;
            }
            Ok(())
        });
        assert!(matches!(
            result,
            Err(StoreError::InvalidInput(
                "too many distinct tables (LMDB caps a store at 32)"
            ))
        ));
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_open_read_only_with_map_size_roundtrips() {
        // A store created with a non-default ceiling reopens read-only with
        // the same ceiling — the path a store grown past the 1 GiB default
        // needs, since the trait `open_read_only` hardcodes that default.
        // (Address space is committed sparsely, so the 2 GiB ceiling costs
        // nothing on disk for one tiny record.)
        const BIG_MAP: usize = 2 << 30; // 2 GiB, a multiple of the page size
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-romap-{}.mdb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let lock: std::path::PathBuf = {
            let mut l = path.clone().into_os_string();
            l.push("-lock");
            l.into()
        };
        let store = LmdbStore::create_with_map_size(&path, BIG_MAP).unwrap();
        store.write(|txn| txn.put("t", b"k", b"v")).unwrap();
        drop(store);
        let readonly = LmdbStore::open_read_only_with_map_size(&path, BIG_MAP).unwrap();
        assert_eq!(readonly.get("t", b"k").unwrap(), Some(b"v".to_vec()));
        assert!(matches!(
            readonly.write(|txn| txn.put("t", b"k2", b"v2")),
            Err(StoreError::ReadOnly)
        ));
        drop(readonly);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }
}
