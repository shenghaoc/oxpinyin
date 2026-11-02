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

/// One writable hash container, or none after a failed attach. On Berkeley
/// DB the pin keeps the failed handle and calls it again, which libdb
/// answers with `BDB1565`; Berkeley DB requires `DB->close` after a failed
/// open, so that handle is closed and its calls have no counterpart
/// (compatibility register, row 113).
///
/// Only a failed native open is a missing container
/// (`WriteStore::attach_bigram`). On Berkeley DB a failed `db_create`
/// panics there, where the pin's `Bigram::attach` asserts
/// (`ngram_bdb.cpp:116-117` at 074a2219). Any other failure before the
/// open, such as an unsupported linked library, is a [`StoreError`], which
/// the utility reports with `oxpinyin_input::tool::exit_on_store_error`.
pub(crate) struct Hash {
    backend: HashBackend,
}

enum HashBackend {
    Open(DefaultStore),
    Missing,
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
    /// ignores: a failed open leaves no container, and the later calls
    /// answer as on a missing one.
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

    /// The store's `Bigram::attach`: `None` is a failed native open.
    fn attach(path: &Path, read_only: bool) -> Result<Self, StoreError> {
        Ok(Self {
            backend: <DefaultStore as WriteStore>::attach_bigram(path, read_only)?
                .map_or(HashBackend::Missing, HashBackend::Open),
        })
    }

    /// `Bigram::load`'s `m_db->Get(key, &value)`.
    pub(crate) fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        match &self.backend {
            HashBackend::Open(store) => {
                let mut value = Vec::new();
                store.get_bigram_raw(key, &mut value).ok()?.then_some(value)
            }
            HashBackend::Missing => None,
        }
    }

    /// `Bigram::store`'s `m_db->Set(key, value)`.
    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        match &self.backend {
            HashBackend::Open(store) => store.write(|txn| {
                txn.put_raw(key, value)?;
                Ok(())
            }),
            HashBackend::Missing => Ok(()),
        }
    }

    /// `Bigram::get_all_items`'s key collector: every raw key in the
    /// container's own walk order.
    pub(crate) fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        match &self.backend {
            HashBackend::Open(store) => store.native_keys(),
            HashBackend::Missing => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Hash;
    use std::path::Path;

    #[test]
    fn a_failure_before_the_open_is_an_error_not_a_missing_container() {
        // Every backend rejects a NUL path before its native open.
        let nul = Path::new(std::ffi::OsStr::new("bad\0name.db"));
        assert!(Hash::open_read_only(nul).is_err());
        assert!(Hash::create_or_missing(nul).is_err());
        // A failed native open is a missing container.
        let dir = std::env::temp_dir().join(format!("oxpinyin-utils-hash-{}", std::process::id()));
        let missing = Hash::open_read_only(&dir.join("missing.db")).expect("a failed open");
        assert!(missing.get(&[0; 4]).is_none());
        assert_eq!(missing.native_keys().expect("keys"), Vec::<Vec<u8>>::new());
    }
}
