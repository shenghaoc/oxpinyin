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

/// One writable hash container, or the pin's still-open handle after a
/// failed attach. The pin ignores a failed attach and keeps the handle;
/// every later member call prints libdb's own `BDB1565` line.
pub(crate) struct Hash {
    backend: HashBackend,
}

enum HashBackend {
    Open(DefaultStore),
    Missing,
    #[cfg(feature = "bdb")]
    Unopened(oxpinyin_store::BdbUnopenedHandle),
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
    /// ignores: a failed open leaves its still-open handle, not a missing
    /// one, so later `get`/`put` and the destructor's sync print libdb's
    /// diagnostics.
    pub(crate) fn create_or_unopened(path: &Path) -> Self {
        #[cfg(feature = "bdb")]
        if let Ok(opened) = oxpinyin_store::open_hash_pin(path, false) {
            return match opened {
                oxpinyin_store::BdbOpen::Open(store) => Self {
                    backend: HashBackend::Open(store),
                },
                oxpinyin_store::BdbOpen::Unopened(db) => Self {
                    backend: HashBackend::Unopened(db),
                },
            };
        }
        match <DefaultStore as WriteStore>::create_hash(path) {
            Ok(store) => Self {
                backend: HashBackend::Open(store),
            },
            Err(_) => Self {
                backend: HashBackend::Missing,
            },
        }
    }

    /// `attach(filename, ATTACH_READONLY)` on the hash class.
    pub(crate) fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        #[cfg(feature = "bdb")]
        if let Ok(opened) = oxpinyin_store::open_hash_pin(path, true) {
            return Ok(match opened {
                oxpinyin_store::BdbOpen::Open(store) => Self {
                    backend: HashBackend::Open(store),
                },
                oxpinyin_store::BdbOpen::Unopened(db) => Self {
                    backend: HashBackend::Unopened(db),
                },
            });
        }
        match <DefaultStore as RawReadStore>::open_hash_read_only(path) {
            Ok(store) => Ok(Self {
                backend: HashBackend::Open(store),
            }),
            Err(error) => Err(error),
        }
    }

    /// `Bigram::load`'s `m_db->Get(key, &value)`.
    pub(crate) fn get(&self, key: &[u8]) -> Option<Vec<u8>> {
        match &self.backend {
            HashBackend::Open(store) => {
                let mut value = Vec::new();
                store.get_bigram_raw(key, &mut value).ok()?.then_some(value)
            }
            HashBackend::Missing => None,
            #[cfg(feature = "bdb")]
            HashBackend::Unopened(db) => {
                let _ = db.get(key);
                None
            }
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
            #[cfg(feature = "bdb")]
            HashBackend::Unopened(db) => {
                db.put(key, value);
                Ok(())
            }
        }
    }

    /// Whether `attach` failed and left the pin's unopened handle. The
    /// pin then answers each `load` with libdb's `BDB1565` rather than
    /// walking anything.
    pub(crate) fn is_unopened(&self) -> bool {
        #[cfg(feature = "bdb")]
        {
            matches!(self.backend, HashBackend::Unopened(_))
        }
        #[cfg(not(feature = "bdb"))]
        {
            false
        }
    }

    /// `Bigram::get_all_items`'s key collector: every raw key in the
    /// container's own walk order.
    pub(crate) fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        match &self.backend {
            HashBackend::Open(store) => store.native_keys(),
            HashBackend::Missing => Ok(Vec::new()),
            #[cfg(feature = "bdb")]
            HashBackend::Unopened(db) => {
                db.cursor();
                Ok(Vec::new())
            }
        }
    }
}
