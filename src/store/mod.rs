//! Backend-agnostic ordered key–value store for oxpinyin tables.
//!
//! This module defines an ordered byte-KV interface over each DBM file's raw
//! keyspace, split into two capability tiers — [`RawReadStore`] (open, point
//! get, ranged scan, native walk, count) and [`WriteStore`] (creation, raw
//! writes, compaction) — and provides the peer implementations behind it:
// Only the selected peer type is present in this build.
#![cfg_attr(feature = "kyotocabinet", doc = " [`KcStore`] on Kyoto Cabinet,")]
#![cfg_attr(not(feature = "kyotocabinet"), doc = " `KcStore` on Kyoto Cabinet,")]
#![cfg_attr(feature = "tkrzw", doc = " [`TkrzwStore`] on tkrzw, and")]
#![cfg_attr(not(feature = "tkrzw"), doc = " `TkrzwStore` on tkrzw, and")]
#![cfg_attr(feature = "bdb", doc = " [`BdbStore`] on Berkeley DB.")]
#![cfg_attr(not(feature = "bdb"), doc = " `BdbStore` on Berkeley DB.")]
//! See [backend selection](https://github.com/shenghaoc/oxpinyin/blob/main/docs/architecture.md#backend-selection). The backend determines the
//! database container and its native iteration order.
//!
//! # Key ordering
//!
//! [`RawReadStore::scan_raw`] and [`RawReadStore::range_raw`] traverse the requested
//! bounds in ascending byte order, comparing raw keys lexicographically.
//! [`RawReadStore::native_keys`] returns keys in the container's own order. A hash
//! database need not visit keys in sorted order.

// TreeDBM uses LexicalKeyComparator, TreeDB uses LEXICALCOMP, and DB_BTREE uses its
// default bytewise comparator. See docs/findings/store-key-ordering.md,
// phrase_large_table3_tkrzwdb.cpp:155–190 and ngram_tkrzwdb.cpp:143–172.

// Constitution §4: keep these non-test denials. A permitted site needs the
// narrowest justified #[expect], citing the pin's abort site or an invariant
// making failure unreachable; see docs/rust-conventions.md.
// An expectation proves lint coverage, not correspondence with the pin.
// #[cfg(test)] modules are exempt; tests/, benches/ and examples/
// are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]
#![cfg_attr(not(test), deny(clippy::unreachable))]

use std::ops::{Bound, ControlFlow};
use std::path::Path;

/// Errors that can occur when opening or scanning a store.
///
/// The variants preserve the I/O-vs-other distinction without naming any
/// backend type, so callers can branch on I/O failures independently of
/// which backend produced them.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// An I/O error from the underlying storage layer.
    #[error("store I/O error: {0}")]
    Io(#[source] std::io::Error),
    /// A non-I/O error from the storage backend.
    #[error("store error: {0}")]
    Backend(#[source] BackendError),
    /// A mutation was requested through a read-only store.
    #[error("store is read-only")]
    ReadOnly,
    /// A public input cannot be represented by the selected backend.
    #[error("invalid store input: {0}")]
    InvalidInput(&'static str),
    /// A checked record read found the record, then the value read failed
    /// or returned a different size. Kyoto Cabinet's `Bigram::load` checks
    /// the size and then reads the body, and asserts when the two disagree
    /// (`ngram_kyotodb.cpp:131-139` at the pin).
    #[error("store read failed after its size check")]
    ReadAfterCheck,
}

/// A non-I/O failure reported by the storage backend.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct BackendError {
    message: String,
}

impl From<&str> for BackendError {
    fn from(message: &str) -> Self {
        Self::from(message.to_owned())
    }
}

impl From<String> for BackendError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

/// What a scan visitor answers after each row: [`ControlFlow::Continue`]
/// to go on, [`ControlFlow::Break`] to end the scan right there. A break
/// is a normal end of the scan — the backend abandons the underlying
/// cursor walk and the scan returns `Ok(())` — and is distinct from a
/// visitor `Err`, which aborts the scan and propagates.
pub type Flow = ControlFlow<()>;

/// A visitor that fills the replacement bytes for one native record.
pub type RecordRewrite<'a> = dyn FnMut(&[u8], &[u8], &mut Vec<u8>) + 'a;

#[cfg(not(feature = "bdb"))]
pub(crate) struct Rewriter<'a, 'f>(pub(crate) &'a mut RecordRewrite<'f>);

#[cfg(not(feature = "bdb"))]
impl Rewriter<'_, '_> {
    fn replacement(&mut self, key: &[u8], value: &[u8]) -> Vec<u8> {
        let mut replacement = Vec::new();
        (self.0)(key, value, &mut replacement);
        replacement
    }
}

/// Where [`WriteStore::write_user_index`] sends each record in key order.
pub type RecordSink<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), StoreError> + 'a;

// ── traits ─────────────────────────────────────────────────────────

/// The write tier: creation and raw writes.
///
/// Adds mutation on top of [`RawReadStore`], whose reads a writable backend
/// necessarily also provides.
pub trait WriteStore: RawReadStore {
    /// Set one native record without a transaction or a flush.
    ///
    /// # Errors
    /// Returns the backend write error.
    fn set_raw(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;

    /// Rewrite every record in the container's native walk order.
    ///
    /// # Errors
    /// Returns the backend walk or write error.
    fn rewrite_raw(&self, rewrite: &mut RecordRewrite<'_>) -> Result<(), StoreError>;

    /// Save the in-memory user index to its native file format.
    ///
    /// # Errors
    /// Returns the backend read or file-write error.
    fn save_user_index(&self, path: &Path) -> Result<(), StoreError>
    where
        Self: Sized,
    {
        Self::write_user_index(path, |sink| {
            self.range_raw(Bound::Unbounded, Bound::Unbounded, sink)
        })
    }

    /// Open or create a store file in read-write mode.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be created.
    fn create(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Open or create a **hash** store file in read-write mode — the
    /// write half of [`RawReadStore::open_hash_read_only`].
    ///
    /// libpinyin's `bigram.db` is a KC **`HashDB`** / Tkrzw **`HashDBM`**
    /// while its other DBMs are tree containers; `import_interpolation`
    /// writes the bigram through this constructor so the reader's hash
    /// open finds a hash file.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be created.
    fn create_hash(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;
    /// A fresh, empty, **in-memory** tree store: the backend's own
    /// on-memory ordered container, which touches no file, reads no
    /// environment, and dies with the handle — what a libpinyin session
    /// runs on between `pinyin_init` and `pinyin_fini`, where the pin
    /// keeps its working state in process memory too (`pinyin.cpp:
    /// 326-444` at 074a2219 loads the profile into the context; nothing
    /// under `src` opens temp space). Being memory, it is also private to
    /// the process: a `fork`ed child's writes and saves cannot reach the
    /// parent's session through a shared file description, which a
    /// file-backed session cannot promise.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the container cannot be created.
    fn create_in_memory() -> Result<Self, StoreError>
    where
        Self: Sized;
    /// Writes one user phrase/pinyin index in the backend's pin format,
    /// with the calls the pin's `save_db` makes. `records` hands each
    /// record, in ascending key order, to the sink it is given; nothing
    /// is buffered between it and the backend. Each backend follows its
    /// `save_db` sequence: tkrzw's `RemoveFile`,
    /// `TreeDBM` open, one `Set` per record, `Synchronize(false)` and close;
    /// Berkeley DB's unlink, `DB_BTREE` open, one `put` per record and
    /// close; Kyoto Cabinet's unlink and `ProtoTreeDB` `dump_snapshot`.
    /// The caller owns staging and the rename.
    ///
    /// # Errors
    /// Returns an error when creation or a record write fails.
    fn write_user_index(
        path: &Path,
        records: impl FnOnce(&mut RecordSink<'_>) -> Result<(), StoreError>,
    ) -> Result<(), StoreError>
    where
        Self: Sized;

    /// Removes one raw record without a transaction or a sync.
    ///
    /// # Errors
    /// Returns a backend error on failure, including a read-only store.
    #[doc(hidden)]
    fn remove_raw(&self, key: &[u8]) -> Result<bool, StoreError>;

    /// Whether writes are disabled for the native handle.
    #[doc(hidden)]
    fn is_read_only(&self) -> bool;

    /// Synchronizes the native container.
    /// # Errors
    ///
    /// Returns [`StoreError`] when the flush fails.
    fn flush(&self) -> Result<(), StoreError>;
}

// ── Read tier ─────────────────────────────────────────────────────
//
// libpinyin's DBM files (pinyin_index.bin, phrase_index.bin, bigram.db,
// punct.bin) are each one database opened with no subdatabase name, with
// raw byte keys. This trait reads that keyspace as it is, so oxpinyin can
// consume a libpinyin-generated data directory without importing or
// converting its files.

/// Read-only access to a store file's raw keyspace.
///
/// The KC, Tkrzw and BDB backends implement this by calling the underlying
/// library with the caller's key verbatim, matching libpinyin's
/// single-keyspace DBM layout. A backend that can only read implements
/// this and nothing else; the system-table loader binds to exactly this
/// tier.
pub trait RawReadStore {
    /// Open an existing store file in read-only mode.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be opened read-only.
    fn open_read_only(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Read a single raw key into `value`, a buffer the caller owns:
    /// `value` is cleared, then holds the stored bytes on a hit; the
    /// answer is whether the key was found.
    ///
    /// The buffer keeps its capacity, so a caller that passes one buffer
    /// to successive reads allocates only when a value outgrows it — the
    /// chunk libpinyin's tables reuse across reads (`m_chunk.set_size`
    /// then `get` into it, `ngram_kyotodb.cpp:136-139` at 074a2219).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend read fails.
    fn get_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError>;

    /// Read a raw value into a bounded buffer and return its full length.
    ///
    /// # Errors
    /// Returns [`StoreError`] when the backend read fails.
    #[cfg(feature = "kyotocabinet")]
    fn get_raw_bounded(&self, key: &[u8], value: &mut [u8]) -> Result<Option<usize>, StoreError> {
        let mut whole = Vec::new();
        if !self.get_raw(key, &mut whole)? {
            return Ok(None);
        }
        let count = value.len().min(whole.len());
        value[..count].copy_from_slice(&whole[..count]);
        Ok(Some(whole.len()))
    }

    /// Reads at most `length` value bytes at `offset`. The default copies
    /// the whole value and slices it; Berkeley DB overrides it with the
    /// pin's `DB_DBT_PARTIAL` read, which never copies the rest of a
    /// record.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend read fails.
    fn get_raw_partial(
        &self,
        key: &[u8],
        offset: u32,
        length: u32,
        value: &mut Vec<u8>,
    ) -> Result<bool, StoreError> {
        let mut whole = Vec::new();
        if !self.get_raw(key, &mut whole)? {
            return Ok(false);
        }
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(whole.len());
        let end = start
            .saturating_add(usize::try_from(length).unwrap_or(usize::MAX))
            .min(whole.len());
        value.clear();
        value.extend_from_slice(&whole[start..end]);
        Ok(true)
    }

    /// Read a raw bigram row the way libpinyin's `Bigram::load` does: a
    /// size check followed by the body read. The default is one
    /// [`Self::get_raw`]; KC overrides it with its two visits and reports
    /// a body failure after a successful check as
    /// [`StoreError::ReadAfterCheck`], the point where the pin asserts
    /// (`ngram_kyotodb.cpp:131-139` at 074a2219). A failed check stays a
    /// miss, as the pin's `Bigram::load` returns false for it.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the checked body read disagrees with
    /// the check; other backend failures follow [`Self::get_raw`].
    fn get_bigram_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.get_raw(key, value)
    }

    /// Read a raw pinyin- or phrase-table row the way libpinyin's
    /// `ChewingLargeTable2::search_internal` and `PhraseLargeTable3::search` do. The default is one
    /// [`Self::get_raw`]. KC overrides it with the pin's size check, which
    /// answers a miss when it fails and a hit with no body read for an
    /// empty row, then the body read; a body read that fails or returns
    /// another size is [`StoreError::ReadAfterCheck`], the point where the
    /// pin asserts (`chewing_large_table2_kyotodb.cpp:159-176`,
    /// `phrase_large_table3_kyotodb.cpp:174-187` at 074a2219).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the checked body read disagrees with
    /// the check; other backend failures follow [`Self::get_raw`].
    fn get_pinyin_row_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.get_raw(key, value)
    }

    /// Whether a raw key exists, an empty value included, with no value
    /// bytes copied — libpinyin's existence check before its suggestion
    /// walk (`m_db->check`, `phrase_large_table3_kyotodb.cpp:206`;
    /// `m_db->Get(key, nullptr)`, `phrase_large_table3_tkrzwdb.cpp:166`, at
    /// 074a2219). Each backend answers with its own existence call.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend read fails.
    fn contains_raw(&self, key: &[u8]) -> Result<bool, StoreError>;

    // The continuation walk for search_suggestion relies on ordered bounds,
    // phrase_large_table3_tkrzwdb.cpp:155–190.
    /// Visits rows between the supplied bounds in ascending key-byte order. Returning
    /// [`ControlFlow::Break`] from the visitor ends the walk successfully.
    ///
    /// This walk is not a snapshot. Writes during visitor calls can change
    /// subsequent records or provoke native backend errors. The effects depend
    /// on the backend; this trait gives no uniform guarantee for such writes.
    ///
    /// # Errors
    ///
    /// Returns a backend or visitor [`StoreError`]. Earlier visitor calls have already run.
    fn scan_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError>;

    /// [`Self::scan_raw`] for a visitor that never stops the walk early.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend walk fails; a visitor
    /// error aborts the walk and propagates.
    fn range_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        self.scan_raw(lo, hi, |key, value| {
            visit(key, value).map(|()| ControlFlow::Continue(()))
        })
    }

    /// Copies every record of a saved user index into `visit`, in the walk
    /// order, as the pin's `load_db` copies the file into its in-memory
    /// table. The default is the whole [`Self::range_raw`] walk, whose
    /// error the caller handles: Kyoto Cabinet's `load_snapshot` asserts
    /// nothing (074a2219 chewing_large_table2_kyotodb.cpp:94-105). tkrzw and
    /// Berkeley DB copy with `copy_tkrzwdb` and `copy_bdb`, which assert on a
    /// failed source walk at the copy operation.
    ///
    /// # Errors
    ///
    /// A [`StoreError`] when the walk fails where the pin does not assert.
    fn copy_raw(&self, mut visit: impl FnMut(&[u8], &[u8])) -> Result<(), StoreError> {
        self.range_raw(Bound::Unbounded, Bound::Unbounded, |key, value| {
            visit(key, value);
            Ok(())
        })
    }

    /// Appends raw keys in the container's **own walk order** — the order
    /// libpinyin's `Bigram::get_all_items` collects
    /// (`ngram_tkrzwdb.cpp:143-172`, `ngram_bdb.cpp`,
    /// `ngram_kyotodb.cpp`): Berkeley DB and Kyoto Cabinet advance a
    /// cursor, tkrzw calls `ProcessEach`, and a hash container's order is
    /// the library's bucket order, not ascending byte order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend walk fails. Keys visited before
    /// the failure stay in `keys`.
    fn native_keys(&self, keys: &mut Vec<Vec<u8>>) -> Result<(), StoreError>;

    /// The number of raw (unframed) rows — the count half of the raw
    /// keyspace [`RawReadStore::range_raw`] walks. The hash containers
    /// (`bigram.db`) are unordered on some backends, so a completeness
    /// check that cannot walk them (a KC `HashDB` cursor has no ordered
    /// first position) compares per-key values through [`Self::get_raw`]
    /// and closes the reverse direction through this count.
    ///
    /// The default implementation counts by walking the raw keyspace;
    /// KC and Tkrzw override it with the library's own record count.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend count fails.
    #[cfg(test)]
    fn count_raw(&self) -> Result<u64, StoreError> {
        let mut count: u64 = 0;
        self.range_raw(Bound::Unbounded, Bound::Unbounded, |_key, _value| {
            count += 1;
            Ok(())
        })?;
        Ok(count)
    }

    /// Opens a hash-DB file in read-only mode (for `bigram.db`).
    ///
    /// libpinyin's `bigram.db` uses KC **`HashDB`** / Tkrzw **`HashDBM`**,
    /// while the other DBM files use TreeDB/TreeDBM.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend open fails.
    fn open_hash_read_only(path: &std::path::Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Opens a saved user phrase/pinyin index. KC uses snapshot streams
    /// (074a2219 chewing_large_table2_kyotodb.cpp:94-105); the default is
    /// the backend's ordinary index container.
    ///
    /// # Errors
    /// Returns an error when the index cannot be read.
    fn open_user_index(path: &std::path::Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::open_read_only(path)
    }
}

mod user_bigram {
    use super::StoreError;
    use std::path::Path;

    /// libpinyin's user bigram container as the pin holds it between
    /// `pinyin_init` and `pinyin_fini`: `Bigram::m_db` after `load_db` — an
    /// **in-memory** container of the backend's own type, filled from
    /// `user_bigram.db` by the backend's own copy loop, mutated by `store` /
    /// `remove`, walked by `get_all_items`, and written back by `save_db`.
    ///
    /// The container is the pin's by construction — the same library, the
    /// same database type, the same default tuning, the same sequence of
    /// inserts — so its walk order is the pin's walk order: tkrzw `TinyDBM`
    /// copied from the `HashDBM` file by iterator (`ngram_tkrzwdb.cpp:48-63`,
    /// `tkrzwdb_utils.h`), Kyoto Cabinet `StashDB` filled by
    /// `load_snapshot` (`ngram_kyotodb.cpp:54-64`), Berkeley DB an
    /// in-memory `DB_HASH` filled by cursor copy (`ngram_bdb.cpp:47-79`,
    /// `bdb_utils.h:43-74`). Keys are the 4-byte native-endian predecessor
    /// token; values the `SingleGram` chunk.
    #[doc(hidden)]
    pub trait UserBigramDb: Sized {
        /// A fresh, empty in-memory container — what `load_db` works on
        /// before the copy.
        ///
        /// # Errors
        ///
        /// Returns [`StoreError`] for a linked library this backend does not
        /// support. A container the pin cannot create ends the process, as the
        /// pin's assert or uncaught `std::bad_alloc` does.
        fn empty() -> Result<Self, StoreError>;

        /// `Bigram::load_db`: a fresh in-memory container, then the file's
        /// records in the file's own walk order. An absent or unreadable
        /// file leaves the container empty, as the pin's does — its
        /// `load_db` fails after the in-memory database already exists.
        ///
        /// # Errors
        ///
        /// As [`Self::empty`].
        fn load_db(path: &Path) -> Result<Self, StoreError>;

        /// `Bigram::load`'s read: the stored value for `key`, into `value`
        /// with [`super::RawReadStore::get_raw`]'s buffer contract — cleared, filled on a
        /// hit, and the answer is whether `key` was found. The pin reads into
        /// the chunk the `Bigram` owns and reuses (`ngram_kyotodb.cpp:136-139`
        /// at 074a2219).
        ///
        /// # Errors
        ///
        /// Returns [`StoreError`] when the read fails.
        fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError>;

        /// `Bigram::load` as `train_result3` calls it
        /// (`phonetic_lookup.h:876-881` at 074a2219): [`Self::get`]'s buffer
        /// contract, with a failed read answering no value, as the pin's
        /// `load` answers `false` and training makes a fresh gram
        /// (`ngram_bdb.cpp:127-145`, `ngram_tkrzwdb.cpp:102-121`,
        /// `ngram_kyotodb.cpp:124-144`). Kyoto Cabinet's `load` checks the
        /// size before its read only to size a C++ buffer; in its
        /// single-threaded in-memory database the two cannot disagree, so
        /// one read gives the same answer.
        ///
        /// # Errors
        ///
        /// None: the result is `Ok` whether or not the read succeeds.
        fn load(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
            self.get(key, value).or_else(|_| {
                value.clear();
                Ok(false)
            })
        }

        /// `Bigram::store`: set `key` to `value`.
        ///
        /// # Errors
        ///
        /// Returns [`StoreError`] when the write fails.
        fn store(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;

        /// `Bigram::remove`: delete `key`; an absent key is not an error.
        ///
        /// # Errors
        ///
        /// Returns [`StoreError`] when the delete fails.
        fn remove(&self, key: &[u8]) -> Result<(), StoreError>;

        /// `Bigram::get_all_items`: every key in the container's walk order,
        /// collected into one vector as the pin collects `phrase_token_t`s
        /// into one `GArray` (`ngram_kyotodb.cpp:163-196` at 074a2219). A key
        /// that is not four bytes, which the pin's visitor asserts against,
        /// comes back as `None` in its place, for the caller to refuse.
        ///
        /// # Errors
        ///
        /// Returns [`StoreError`] when the walk fails.
        fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError>;

        /// Whether the container holds no record. Not a pin call: oxpinyin's
        /// own check of whether a session holds grams, answered without the
        /// full [`Self::keys`] walk where the backend keeps a record count.
        ///
        /// # Errors
        ///
        /// Returns [`StoreError`] when the container cannot be read.
        fn is_empty(&self) -> Result<bool, StoreError>;

        /// Whether this backend has a failed container at all: only Berkeley
        /// DB's `load_db` carries on after its in-memory `DB->open` fails
        /// (`ngram_bdb.cpp:54-57` at 074a2219). The other backends cannot fail
        /// to create the container and carry on, so a caller's check of
        /// [`Self::is_failed`] behind this constant is a constant `false` there.
        const CAN_FAIL: bool = false;

        /// Whether this is the failed container a Berkeley DB `load_db` leaves
        /// when its in-memory `DB->open` fails: reads miss, walks are empty,
        /// and `store` fails. It never changes after creation.
        fn is_failed(&self) -> bool {
            false
        }

        /// `Bigram::save_db`: remove `path`, then write the container to it
        /// the pin's way (a fresh file of the backend's user-bigram form,
        /// filled in this container's walk order).
        ///
        /// # Errors
        ///
        /// Returns [`StoreError`] when the file cannot be written.
        fn save_db(&self, path: &Path) -> Result<(), StoreError>;
    }

    /// The compiled-in backend's [`UserBigramDb`].
    #[cfg(feature = "kyotocabinet")]
    #[doc(hidden)]
    pub type DefaultUserBigramDb = super::KcUserBigramDb;
    /// The compiled-in backend's [`UserBigramDb`].
    #[cfg(feature = "tkrzw")]
    #[doc(hidden)]
    pub type DefaultUserBigramDb = super::TkrzwUserBigramDb;
    /// The compiled-in backend's [`UserBigramDb`].
    #[cfg(feature = "bdb")]
    #[doc(hidden)]
    pub type DefaultUserBigramDb = super::BdbUserBigramDb;
}

// Downstream C-ABI fixtures use the native container only with test hooks.
#[cfg(feature = "test-hooks")]
#[doc(hidden)]
pub use user_bigram::{DefaultUserBigramDb, UserBigramDb};
#[cfg(not(feature = "test-hooks"))]
pub(crate) use user_bigram::{DefaultUserBigramDb, UserBigramDb};

/// Removes `path`; absence is not an error — libpinyin's
/// `unlink`-before-create, which ignores `ENOENT`.
pub(crate) fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Io(error)),
    }
}

// ── The default backend: compile-time selection ───────────────────────
// See https://github.com/shenghaoc/oxpinyin/blob/main/docs/architecture.md#backend-selection.

/// The selected store backend on
/// `--no-default-features --features kyotocabinet`.
#[cfg(feature = "kyotocabinet")]
pub type DefaultStore = KcStore;

/// The tkrzw store selected by the default feature set.
#[cfg(feature = "tkrzw")]
pub type DefaultStore = TkrzwStore;

/// The selected store backend on `--no-default-features --features bdb`:
/// Berkeley DB, libpinyin's original DBM and its `configure.ac` default
/// (`DBM` is `BerkeleyDB`), not oxpinyin's default.
#[cfg(feature = "bdb")]
pub type DefaultStore = BdbStore;

/// The backend's `database format:` token — the string `user.conf`'s
/// conformance check compares (`table_info.cpp`'s
/// `to/from_table_database_format_type`: exactly `BerkeleyDB`,
/// `KyotoCabinet`, `Tkrzw` upstream), so a same-backend pair stays
/// conform and every cross-backend pair answers non-conform. The build
/// script selects it once from the backend feature, the pin's
/// `DATABASE_FORMAT`. Both `.pc` files carry the same token, from
/// `tools/dev/cell.py`.
pub const DEFAULT_STORE_DB_FORMAT: &str = env!("OXPINYIN_STORE_DATABASE_FORMAT");

/// Helpers shared by the backends' raw walks and path checks; every item is
/// gated to the backends that use it (see the module docs).
mod common;
/// The record copy of the pin's tkrzw and Berkeley DB `load_db` and
/// `save_db`, which aborts where the pin's copy asserts.
#[cfg(any(feature = "tkrzw", feature = "bdb"))]
mod copy;
/// The copy's fault points for other crates' death tests.
#[cfg(all(
    any(test, feature = "test-hooks"),
    any(feature = "tkrzw", feature = "bdb")
))]
pub use copy::faults::fail_destination_write_after;
#[cfg(all(test, any(feature = "tkrzw", feature = "bdb")))]
pub(crate) use copy::faults::{fail_iterator_creation_after, fail_source_read_after};
#[cfg(test)]
pub(crate) mod death_test;
/// The read-only stores' point-read fault point for other crates' tests.
#[cfg(any(test, feature = "test-hooks"))]
mod read_faults;
#[cfg(any(test, feature = "test-hooks"))]
pub use read_faults::fail_point_read_of;

#[cfg(feature = "tkrzw")]
mod tkrzw;
#[cfg(feature = "tkrzw")]
pub use tkrzw::{TkrzwStore, TkrzwUserBigramDb};

#[cfg(feature = "bdb")]
mod bdb;
#[cfg(feature = "bdb")]
pub use bdb::{BdbStore, BdbUserBigramDb};

#[cfg(feature = "kyotocabinet")]
pub mod kyotocabinet;
#[cfg(feature = "kyotocabinet")]
pub use kyotocabinet::{KcStore, KcUserBigramDb};

mod system_attach;
pub use system_attach::{AccessMode, DbmKind, SystemAttachment};

// ── tests ──────────────────────────────────────────────────────────

/// Owned-result forms of the buffer-filling point reads, for the unit
/// tests' assertions.
#[cfg(test)]
pub(crate) trait RawReadOwned: RawReadStore {
    fn read_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let mut value = Vec::new();
        Ok(self.get_raw(key, &mut value)?.then_some(value))
    }
}

#[cfg(test)]
impl<S: RawReadStore + ?Sized> RawReadOwned for S {}

#[cfg(test)]
pub(crate) trait GramReadOwned: UserBigramDb {
    fn read_gram(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let mut value = Vec::new();
        Ok(self.get(key, &mut value)?.then_some(value))
    }
}

#[cfg(test)]
impl<D: UserBigramDb> GramReadOwned for D {}

#[cfg(test)]
mod tests;
