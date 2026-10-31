//! The pin's two DBM container kinds through `oxpinyin-store`'s raw
//! keyspace: `TreeDB`/`TreeDBM`/`DB_BTREE` for the index and punctuation
//! tables, `HashDB`/`HashDBM`/`DB_HASH` for `bigram.db`.
//!
//! The pin's `attach` opens the named file with `ATTACH_READWRITE` (and
//! `ATTACH_CREATE`) or `ATTACH_READONLY`, so an existing container is
//! **appended to**, never replaced; [`Tree::create`] and
//! [`Hash::create`] make that same open, and every row is written with the
//! pin's `get` then `put` (or `put` alone) through one committed
//! transaction each.

use std::path::Path;

use oxpinyin_store::{DefaultStore, RawReadStore, StoreError, WriteStore, WriteTxn as _};

/// One writable tree container.
pub(crate) struct Tree {
    store: DefaultStore,
}

impl Tree {
    /// `attach(filename, ATTACH_READWRITE|ATTACH_CREATE)`.
    pub(crate) fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: <DefaultStore as WriteStore>::create(path)?,
        })
    }

    /// `attach(filename, ATTACH_READONLY)`.
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: <DefaultStore as oxpinyin_store::ReadStore>::open_read_only(path)?,
        })
    }

    /// `m_db->get(m_db, NULL, &key, &data, 0)`: `None` is any failure,
    /// which every caller treats as a missing record.
    pub(crate) fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let mut value = Vec::new();
        self.store.get_raw(key, &mut value).ok()?.then_some(value)
    }

    /// `m_db->put(m_db, NULL, &key, &data, 0)`.
    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.store.write(|txn| {
            txn.put_raw(key, value)?;
            Ok(())
        })
    }
}

/// One writable hash container.
pub(crate) struct Hash {
    store: DefaultStore,
}

impl Hash {
    /// `attach(filename, ATTACH_READWRITE|ATTACH_CREATE)` on the hash
    /// class.
    pub(crate) fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: <DefaultStore as WriteStore>::create_hash(path)?,
        })
    }

    /// `attach(filename, ATTACH_READONLY)` on the hash class.
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: <DefaultStore as RawReadStore>::open_hash_read_only(path)?,
        })
    }

    /// `Bigram::load`'s `m_db->Get(key, &value)`.
    pub(crate) fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let mut value = Vec::new();
        self.store
            .get_bigram_raw(key, &mut value)
            .ok()?
            .then_some(value)
    }

    /// `Bigram::store`'s `m_db->Set(key, value)`.
    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.store.write(|txn| {
            txn.put_raw(key, value)?;
            Ok(())
        })
    }

    /// `Bigram::get_all_items`'s key collector: every raw key in the
    /// container's own walk order.
    pub(crate) fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        self.store.native_keys()
    }
}
