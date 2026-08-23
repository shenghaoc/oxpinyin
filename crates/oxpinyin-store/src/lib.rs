//! Backend-agnostic ordered key–value store for oxpinyin data tables.
//!
//! This crate defines the [`OrderedStore`] trait — a read-only scan over
//! every `(key, value)` pair in ascending key-byte order — and provides a
//! [`RedbStore`] implementation backed by redb.  Consumers depend on the
//! trait; the concrete backend is selected by the [`DefaultStore`] alias.

use std::fmt;
use std::path::Path;

use redb::{ReadableDatabase, ReadableTable};

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

/// Row visitor passed to [`OrderedStore::for_each`].
pub type Visitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), StoreError> + 'a;

/// A read-only ordered key–value store.
///
/// Implementations visit every `(key, value)` pair of a named table in
/// ascending key-byte order, borrowing each row without a per-row clone.
pub trait OrderedStore {
    /// Visit every `(key, value)` of `table` in ascending key-byte order,
    /// borrowing each row (no per-row clone).  Stops early on visitor `Err`.
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError>;
}

/// A redb-backed [`OrderedStore`].
pub struct RedbStore {
    db: redb::ReadOnlyDatabase,
}

impl RedbStore {
    /// Opens a redb database file in read-only mode.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError::Io`] when the file cannot be opened due to an
    /// I/O error, or [`StoreError::Backend`] for other redb failures.
    pub fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let db = redb::Builder::new()
            .open_read_only(path)
            .map_err(map_database_error)?;
        Ok(Self { db })
    }
}

impl OrderedStore for RedbStore {
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        let definition: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new(table);
        let txn = self.db.begin_read().map_err(map_transaction_error)?;
        let tbl = txn.open_table(definition).map_err(map_table_error)?;
        for item in tbl.iter().map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }
}

/// The default store backend.
pub type DefaultStore = RedbStore;

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
