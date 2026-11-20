//! The pin's two DBM container kinds through `oxpinyin`'s raw
//! keyspace: `TreeDB`/`TreeDBM`/`DB_BTREE` for the index and punctuation
//! tables, `HashDB`/`HashDBM`/`DB_HASH` for `bigram.db`.
//!
//! The pin's `attach` opens the named file with `ATTACH_READWRITE` (and
//! `ATTACH_CREATE`) or `ATTACH_READONLY`, so an existing container is
//! **appended to**, never replaced; `Tree::create` and
//! `Hash::create` make that same open, and every row is written with the
//! pin's `get` then `put` (or `put` alone) through one committed
//! transaction each.

use std::path::Path;

use oxpinyin::store::{
    DefaultStore, RawReadStore, StoreError, SystemAttachment, WriteStore, WriteTxn as _,
};

/// One writable tree container.
pub struct Tree {
    store: DefaultStore,
    plain_set: bool,
}

impl Tree {
    /// The pin's default in-memory table constructor.
    pub fn memory() -> Result<Self, StoreError> {
        Ok(Self {
            store: DefaultStore::create_in_memory()?,
            plain_set: true,
        })
    }

    /// mask_out(0,0) leaves every key with an empty value, then synchronizes.
    pub fn mask_all(&self) {
        if let Ok(keys) = self.native_keys() {
            for key in keys {
                let _ = self.put(&key, &[]);
            }
        }
        let _ = self.store.flush();
    }

    /// `attach(filename, ATTACH_READWRITE|ATTACH_CREATE)`.
    pub(crate) fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: <DefaultStore as WriteStore>::create(path)?,
            plain_set: false,
        })
    }

    /// The pin's complete native key walk (`PunctTable::get_all_items`).
    pub(crate) fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut keys = Vec::new();
        self.store.native_keys(&mut keys)?;
        Ok(keys)
    }

    /// `attach(filename, ATTACH_READONLY)`.
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: <DefaultStore as oxpinyin::store::RawReadStore>::open_read_only(path)?,
            plain_set: false,
        })
    }

    /// `m_db->get(m_db, NULL, &key, &data, 0)`: `None` is any failure,
    /// which every caller treats as a missing record.
    #[must_use]
    pub fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let mut value = Vec::new();
        self.store.get_raw(key, &mut value).ok()?.then_some(value)
    }

    /// `m_db->put(m_db, NULL, &key, &data, 0)`.
    pub fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        if self.plain_set {
            return self.store.tool_set(key, value);
        }
        self.store.write(|txn| {
            txn.put_raw(key, value)?;
            Ok(())
        })
    }
}

/// One hash container, absent if the native open failed. Failures before
/// the open are returned to the utility.
pub(crate) struct Hash {
    store: Option<DefaultStore>,
}

impl Hash {
    /// `attach(filename, ATTACH_READWRITE|ATTACH_CREATE)` on the hash
    /// class.
    pub(crate) fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: Some(<DefaultStore as WriteStore>::create_hash(path)?),
        })
    }

    /// `attach(..., ATTACH_READWRITE|ATTACH_CREATE)` whose answer the pin
    /// ignores. After a failed open, reads find no records and writes fail.
    ///
    /// # Errors
    ///
    /// A failure before the native open, such as an unsupported linked
    /// library: never a missing container.
    pub(crate) fn create_or_missing(path: &Path) -> Result<Self, StoreError> {
        Self::attach(path, false)
    }

    /// `attach(filename, ATTACH_READONLY)` on the hash class, whose answer
    /// the pin's utilities ignore.
    ///
    /// # Errors
    ///
    /// As [`Self::create_or_missing`].
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Self::attach(path, true)
    }

    /// The store's `Bigram::attach`.
    fn attach(path: &Path, read_only: bool) -> Result<Self, StoreError> {
        Ok(Self {
            store: match SystemAttachment::attach_bigram(path, read_only)? {
                SystemAttachment::Open(store) => Some(store),
                SystemAttachment::Unopened => None,
            },
        })
    }

    /// Reads a record from an available container.
    pub(crate) fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let store = self.store.as_ref()?;
        let mut value = Vec::new();
        store.get_bigram_raw(key, &mut value).ok()?.then_some(value)
    }

    /// Writes a record if the container opened.
    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| StoreError::Backend("the bigram container did not open".into()))?;
        store.write(|txn| txn.put_raw(key, value))
    }

    /// Collects the raw keys in the container's native order.
    pub(crate) fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut keys = Vec::new();
        if let Some(store) = self.store.as_ref() {
            store.native_keys(&mut keys)?;
        }
        Ok(keys)
    }
}

#[cfg(test)]
mod tests;
