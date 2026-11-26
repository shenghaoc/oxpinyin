//! The pin's two DBM container kinds through `oxpinyin`'s raw
//! keyspace: `TreeDB`/`TreeDBM`/`DB_BTREE` for the index and punctuation
//! tables, `HashDB`/`HashDBM`/`DB_HASH` for `bigram.db`.
//!
//! Writable attachments create a missing file and preserve existing records.
//! Each record write uses one native put or set. On close, file-backed trees
//! synchronize first. Bigram hashes do the same on Berkeley DB and Kyoto
//! Cabinet. On tkrzw the bigram owner closes the file directly.

use std::path::Path;

use oxpinyin::store::{
    AccessMode, DbmKind, DefaultStore, RawReadStore, StoreError, SystemAttachment, WriteStore,
};

/// The utility checks the result of this attachment.
fn open_required(
    path: &Path,
    kind: DbmKind,
    access: AccessMode,
) -> Result<DefaultStore, StoreError> {
    match SystemAttachment::attach(path, kind, access)? {
        SystemAttachment::Open(store) => Ok(store),
        SystemAttachment::Unopened => Err(StoreError::Backend("the container did not open".into())),
    }
}

/// One tree container.
pub struct Tree {
    store: DefaultStore,
}

impl Tree {
    /// The pin's default in-memory table constructor.
    pub fn memory() -> Result<Self, StoreError> {
        Ok(Self {
            store: DefaultStore::create_in_memory()?,
        })
    }

    /// mask_out(0,0) leaves every key with an empty value, then synchronizes.
    ///
    /// # Aborts
    /// On Berkeley DB, a read or update failure after the cursor opens aborts.
    pub fn mask_all(&self) {
        #[cfg(feature = "bdb")]
        self.store.mask_all();
        #[cfg(not(feature = "bdb"))]
        {
            if let Ok(keys) = self.native_keys() {
                for key in keys {
                    let _ = self.put(&key, &[]);
                }
            }
            let _ = self.store.flush();
        }
    }

    /// `attach(filename, ATTACH_READWRITE|ATTACH_CREATE)`.
    pub(crate) fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: open_required(path, DbmKind::Tree, AccessMode::ReadWrite)?,
        })
    }

    /// The pin's complete native key walk (`PunctTable::get_all_items`).
    #[cfg(any(test, not(feature = "bdb")))]
    pub(crate) fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut keys = Vec::new();
        self.store.native_keys(&mut keys)?;
        Ok(keys)
    }

    /// `attach(filename, ATTACH_READONLY)`.
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: open_required(path, DbmKind::Tree, AccessMode::ReadOnly)?,
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
        self.store.set_raw(key, value)
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
            store: Some(open_required(path, DbmKind::Hash, AccessMode::ReadWrite)?),
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
        Self::attach(path, AccessMode::ReadWrite)
    }

    /// `attach(filename, ATTACH_READONLY)` on the hash class, whose answer
    /// the pin's utilities ignore.
    ///
    /// # Errors
    ///
    /// As [`Self::create_or_missing`].
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Self::attach(path, AccessMode::ReadOnly)
    }

    /// The store's `Bigram::attach`.
    fn attach(path: &Path, access: AccessMode) -> Result<Self, StoreError> {
        Ok(Self {
            store: oxpinyin_tool_support::data::attach_bigram(path, access)?,
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
        store.set_raw(key, value)
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
