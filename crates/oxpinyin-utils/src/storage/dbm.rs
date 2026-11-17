//! The pin's two DBM container kinds through `oxpinyin-store`'s raw
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

use oxpinyin_store::{
    DefaultStore, RawReadStore, StoreError, SystemAttachment, UnopenedSystemDbm, WriteStore,
    WriteTxn as _,
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
        self.store.native_keys()
    }

    /// `attach(filename, ATTACH_READONLY)`.
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            store: <DefaultStore as oxpinyin_store::RawReadStore>::open_read_only(path)?,
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

/// One writable hash container, or the object a failed attach keeps. The
/// pin keeps `m_db` after a failed open and its later calls reach it
/// (`ngram_kyotodb.cpp:110-120`, `ngram_tkrzwdb.cpp:86-99` at 074a2219):
/// on Kyoto Cabinet and tkrzw this is that object, refusing each call, and
/// its drop is the owner's `reset()`. On Berkeley DB the pin's calls on the
/// failed handle print `BDB1565`; Berkeley DB requires `DB->close` after a
/// failed open, so that handle is closed and its calls have no counterpart
/// (compatibility register, row 113).
///
/// Only a failed native open is an unopened container
/// (`SystemAttachment::attach_bigram`). On Berkeley DB a failed `db_create`
/// panics there, where the pin's `Bigram::attach` asserts
/// (`ngram_bdb.cpp:116-117` at 074a2219). Any other failure before the
/// open, such as an unsupported linked library, is a [`StoreError`], which
/// the utility reports with `oxpinyin_input::tool::exit_on_store_error`.
pub(crate) struct Hash {
    backend: HashBackend,
}

enum HashBackend {
    Open(DefaultStore),
    Unopened(UnopenedSystemDbm),
}

impl Hash {
    /// `attach(filename, ATTACH_READWRITE|ATTACH_CREATE)` on the hash
    /// class.
    pub(crate) fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            backend: HashBackend::Open(<DefaultStore as WriteStore>::create_hash(path)?),
        })
    }

    /// `attach(..., ATTACH_READWRITE|ATTACH_CREATE)` whose answer the pin
    /// ignores: after a failed open the later calls reach the unopened
    /// object and answer as on an empty container.
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
            backend: match SystemAttachment::attach_bigram(path, read_only)? {
                SystemAttachment::Open(store) => HashBackend::Open(store),
                SystemAttachment::Unopened(db) => HashBackend::Unopened(db),
            },
        })
    }

    /// `Bigram::load`'s `m_db->Get(key, &value)`.
    pub(crate) fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        match &self.backend {
            HashBackend::Open(store) => {
                let mut value = Vec::new();
                store.get_bigram_raw(key, &mut value).ok()?.then_some(value)
            }
            HashBackend::Unopened(db) => {
                db.read(key);
                None
            }
        }
    }

    /// `Bigram::store`'s `m_db->Set(key, value)`: on an unopened container
    /// the object's refusal, as `store` returns false.
    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        match &self.backend {
            HashBackend::Open(store) => store.write(|txn| {
                txn.put_raw(key, value)?;
                Ok(())
            }),
            HashBackend::Unopened(db) => db.write(key, value),
        }
    }

    /// `Bigram::get_all_items`'s key collector: every raw key in the
    /// container's own walk order. An unopened container has no key, and
    /// its answer is the pin's: Kyoto Cabinet's true, tkrzw's and Berkeley
    /// DB's false (`UnopenedSystemDbm::walk`).
    pub(crate) fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        match &self.backend {
            HashBackend::Open(store) => store.native_keys(),
            HashBackend::Unopened(db) => db.walk().map(|()| Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests;
