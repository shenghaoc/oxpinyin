//! The pin's two DBM container kinds through `oxpinyin`'s raw
//! keyspace: `TreeDB`/`TreeDBM`/`DB_BTREE` for the index and punctuation
//! tables, `HashDB`/`HashDBM`/`DB_HASH` for `bigram.db`.
//!
//! Writable attachments create a missing file and preserve existing records.
//! Each record write uses one native put or set. On close, file-backed trees
//! synchronize first. Bigram hashes do the same on Berkeley DB and Kyoto
//! Cabinet. On tkrzw the bigram owner closes the file directly.

use std::path::Path;

use oxpinyin::data::row_format::encode_token_key;
use oxpinyin::single_gram::encode_single_gram;
use oxpinyin_tool_support::data::decode_single_gram;
use oxpinyin_tool_support::input as tool;

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

/// One `SingleGram` row: the stored `total_freq` and its
/// `{next_token, count}` records in stored (token-ascending) order
/// (`src/storage/ngram.cpp:31-57`).
pub(crate) struct Gram {
    /// `SingleGram::m_chunk`'s first word.
    pub total: u32,
    /// The item array after it.
    pub records: Vec<(u32, u32)>,
}

impl Gram {
    /// `SingleGram::get_freq`: the count stored for `token`, `None` where
    /// the pin's binary search finds no record.
    pub(crate) fn get_freq(&self, token: u32) -> Option<u32> {
        let index = self.lower_bound(token);
        self.records
            .get(index)
            .filter(|(next, _)| *next == token)
            .map(|(_, freq)| *freq)
    }

    /// `SingleGram::set_freq`: `false` where the record is absent
    /// (`check_result` is `assert` in the reference build).
    pub(crate) fn set_freq(&mut self, token: u32, freq: u32) -> bool {
        let index = self.lower_bound(token);
        match self.records.get_mut(index) {
            Some((next, stored)) if *next == token => {
                *stored = freq;
                true
            }
            _ => false,
        }
    }

    /// `SingleGram::insert_freq`: `false` where the token is already
    /// present; otherwise the record goes at its lower-bound position.
    pub(crate) fn insert_freq(&mut self, token: u32, freq: u32) -> bool {
        let index = self.lower_bound(token);
        if self
            .records
            .get(index)
            .is_some_and(|(next, _)| *next == token)
        {
            return false;
        }
        self.records.insert(index, (token, freq));
        true
    }

    /// The pin's `SingleGram` lower bound: the first record whose token is
    /// not less than `token` (`ngram.cpp:231-250`; the walk
    /// `src/single_gram.rs`'s `get_freq` mirrors), not the unspecified
    /// index a binary search gives among equal keys.
    fn lower_bound(&self, token: u32) -> usize {
        self.records.partition_point(|(next, _)| *next < token)
    }

    /// `SingleGram::m_chunk`'s bytes (`encode_single_gram`), the value
    /// `Bigram::store` writes under the previous token's key.
    pub(crate) fn encode(&self) -> Vec<u8> {
        encode_single_gram(self.total, &self.records)
    }
}

/// `Bigram::load` of `token` from `bigram`: `None` where the container is
/// unopened (`m_db` NULL) or has no such key.
///
/// A stored value that is not a whole `SingleGram` would make the pin's
/// `get_total_freq`/`get_freq` read past the value; oxpinyin
/// refuses it by the abort it already uses for a corrupt chunk.
pub(crate) fn load_gram(bigram: Option<&Hash>, token: u32) -> Option<Gram> {
    let value = bigram?.get(&encode_token_key(token))?;
    match decode_single_gram(&value) {
        Some((total, records)) => Some(Gram { total, records }),
        None => tool::abort(),
    }
}

/// `Bigram::store` of `token`'s row. The pin's `m_db->put` answer is
/// ignored by `gen_ngram`/`gen_deleted_ngram`.
pub(crate) fn store_gram(bigram: Option<&Hash>, token: u32, gram: &Gram) {
    if let Some(bigram) = bigram {
        let _ = bigram.put(&encode_token_key(token), &gram.encode());
    }
}
