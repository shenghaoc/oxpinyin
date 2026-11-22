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
//! Each build selects one backend through [`DefaultStore`]. The backend determines the
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

// ── Exactly-one-backend invariant, enforced at compile time ────────────
//
// The store backends (kyotocabinet, tkrzw, bdb) are peer
// implementations behind the store's trait surface, and every oxpinyin
// build has exactly one of them. Cargo features are additive under
// unification, so a plausible-looking `cargo build --features kyotocabinet`
// silently combines kyotocabinet with the default tkrzw feature — precisely the
// slide these guards refuse. Every consumer crate forwards its own
// `{kyotocabinet, tkrzw, bdb}` features onto this crate, so
// this one guard suffices for the whole workspace.

// The zero-backend case is refused too — a build with no backend has no
// `DefaultStore` type to assemble the runtime around, and the resulting
// "unresolved type" error a downstream consumer would hit is a worse
// diagnostic than saying "select a backend" here.

#[cfg(not(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb",)))]
compile_error!(
    "oxpinyin: no store backend selected. Enable exactly one of \
     `tkrzw` (the default), `kyotocabinet`, or `bdb`. On the \
     command line: `cargo build` for the default (tkrzw), or \
     `cargo build --no-default-features --features {kyotocabinet|bdb}` \
     for a peer."
);

#[cfg(any(
    all(feature = "kyotocabinet", feature = "tkrzw"),
    all(feature = "kyotocabinet", feature = "bdb"),
    all(feature = "tkrzw", feature = "bdb"),
))]
compile_error!(
    "oxpinyin: more than one store backend selected. Exactly one \
     of `bdb`, `kyotocabinet`, `tkrzw` may be enabled per \
     build. A build that names an alternate peer must also disable the \
     workspace's default feature set: \
     `cargo build --no-default-features --features {kyotocabinet|tkrzw}`."
);

use std::fmt;
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

/// A non-I/O failure the storage backend reported: its message, and the
/// backend's own status code where the library gives one (tkrzw).
///
/// Every backend builds this the same way — from the library's message
/// text — so the store's error type carries no erased payload.
#[derive(Debug)]
pub struct BackendError {
    code: Option<i32>,
    message: String,
}

impl BackendError {
    /// A failure with a message and the backend's status `code`.
    #[must_use]
    pub const fn with_code(code: i32, message: String) -> Self {
        Self {
            code: Some(code),
            message,
        }
    }

    /// The backend's status code, where the library reports one.
    #[must_use]
    pub const fn code(&self) -> Option<i32> {
        self.code
    }
}

impl From<&str> for BackendError {
    fn from(message: &str) -> Self {
        Self {
            code: None,
            message: message.to_owned(),
        }
    }
}

impl From<String> for BackendError {
    fn from(message: String) -> Self {
        Self {
            code: None,
            message,
        }
    }
}

impl fmt::Display for BackendError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for BackendError {}

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

/// A write transaction over a store file's raw keyspace.
///
/// An `Err` from the closure writes nothing provided the rollback succeeds.
/// Kyoto Cabinet's `KcStore::roll_back` (`kyotocabinet/mod.rs`) discards the
/// rollback's own error, so a failed rollback can leave the closure's
/// writes in place. A failure or crash during commit can leave part of
/// the batch applied, as in libpinyin. See the "Atomicity" sections in the
/// backend module docs: [Berkeley DB], [tkrzw], and [Kyoto Cabinet].
/// Reached through [`WriteStore::write`] as the backend's own
/// [`WriteStore::Txn`] type; the trait is not object safe.
///
/// [Berkeley DB]: https://github.com/shenghaoc/oxpinyin/blob/main/src/store/bdb/mod.rs
/// [tkrzw]: https://github.com/shenghaoc/oxpinyin/blob/main/src/store/tkrzw/mod.rs
/// [Kyoto Cabinet]: https://github.com/shenghaoc/oxpinyin/blob/main/src/store/kyotocabinet/mod.rs
pub trait WriteTxn {
    /// Insert or overwrite a raw `key` → `value` in the file's bare
    /// keyspace — the write half of [`RawReadStore::get_raw`]. Rows
    /// written here are what `get_raw` / [`RawReadStore::range_raw`] read
    /// back on the same backend: what libpinyin's own DBMs store and what
    /// `oxpinyin-utils`' storage utilities write.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend write fails.
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError>;

    /// Remove a raw `key` — the removal counterpart of
    /// [`WriteTxn::put_raw`]. No-op when the key is absent.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend write fails.
    fn remove_raw(&mut self, key: &[u8]) -> Result<(), StoreError>;
}

/// The write tier: creation, raw writes, and compaction.
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

    /// The backend's write transaction, handed to the closure of
    /// [`WriteStore::write`] by mutable reference. Each backend names
    /// its own concrete type, so the transaction's methods are called
    /// without dynamic dispatch.
    type Txn<'a>: WriteTxn
    where
        Self: 'a;

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
    /// open finds a hash file. The default implementation delegates to
    /// [`WriteStore::create`] (correct for a backend with no
    /// hash/tree distinction). KC and Tkrzw override this to select the
    /// hash container class.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be created.
    fn create_hash(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::create(path)
    }
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
    /// Create a **user** tree file: the two index tables `pinyin_save`
    /// writes into a user dir. It is [`WriteStore::create`] with the
    /// file mode libpinyin's own `save_db` creates a user table with,
    /// where that differs from the one `attach` uses for the system
    /// tables. Berkeley DB passes 0600 there and 0644 in `attach`, and
    /// Kyoto Cabinet writes a user table through an `std::ofstream`
    /// (0666). The process umask applies on top, as it does upstream.
    /// The default is [`WriteStore::create`] (tkrzw's `save_db` opens
    /// its tables exactly as `attach` does).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be created.
    fn create_user(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::create(path)
    }
    /// [`WriteStore::create_user`]'s hash form: the user bigram, where a
    /// backend writes it as a hash container. The default is
    /// [`WriteStore::create_hash`].
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be created.
    #[cfg(any(test, feature = "test-hooks"))]
    fn create_user_hash(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::create_hash(path)
    }

    /// Writes one user phrase/pinyin index in the backend's pin format,
    /// with the calls the pin's `save_db` makes. `records` hands each
    /// record, in ascending key order, to the sink it is given; nothing
    /// is buffered between it and the backend. Each shipping backend
    /// overrides this with its `save_db` sequence: tkrzw's `RemoveFile`,
    /// `TreeDBM` open, one `Set` per record, `Synchronize(false)` and close;
    /// Berkeley DB's unlink, `DB_BTREE` open, one `put` per record and
    /// close; Kyoto Cabinet's unlink and `ProtoTreeDB` `dump_snapshot`.
    /// The default writes through [`WriteStore::write`]. The caller owns
    /// staging and the rename.
    ///
    /// # Errors
    /// Returns an error when creation or a record write fails.
    fn write_user_index(
        path: &Path,
        records: impl FnOnce(&mut RecordSink<'_>) -> Result<(), StoreError>,
    ) -> Result<(), StoreError>
    where
        Self: Sized,
    {
        let store = Self::create_user(path)?;
        store.write(|txn| records(&mut |key, value| txn.put_raw(key, value)))
    }

    /// Writes `rows` to `path` as the **user** bigram container.
    ///
    /// The write half of [`RawReadStore::open_user_bigram`], and for the
    /// same reason it is not [`Self::create_hash`]: libpinyin's Kyoto
    /// Cabinet user bigram is a snapshot stream, produced by filling an
    /// in-memory `StashDB` and calling `dump_snapshot`
    /// (`ngram_kyotodb.cpp:82-101`), not by creating a hash file at the
    /// path. tkrzw writes a genuine container there, so
    /// the default is [`Self::create_hash`] plus a raw write — what that
    /// backend already did correctly.
    ///
    /// Rows arrive already sorted by key; the container's own order is
    /// what matters on read.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the container cannot be written.
    #[cfg(any(test, feature = "test-hooks"))]
    fn write_user_bigram(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) -> Result<(), StoreError>
    where
        Self: Sized,
    {
        // Atomic replacement: build the whole container at a sibling
        // temporary, compact it, close it, then rename it over `path`.
        // Removing `path` up front and writing in place would destroy
        // the previous file if creation, insertion, or compaction
        // failed — and a rename(2) over an existing file is itself
        // atomic, so this file is always the old one or the new one,
        // never a mix.
        let tmp = sibling_temp(path);
        // libpinyin unlinks the temporary before `save_db` creates it
        // (`pinyin.cpp:1017`, and `save_db` unlinks again): a stale one
        // left by a crashed save is not reopened, so its rows and its
        // mode do not carry into the new file.
        remove_if_present(&tmp)?;
        let mut store = Self::create_user_hash(&tmp)?;
        store.write(|txn| {
            for (key, value) in rows {
                txn.put_raw(key, value)?;
            }
            Ok(())
        })?;
        // The hard sync sits with this `compact`, and only one of
        // this default's users still owes it there: tkrzw's `write`
        // reaches the operating system and its `compact` the device,
        // so the device sync must land before the rename.
        // Berkeley DB already reaches stable storage in `write`;
        // its `compact` here is compaction work, not durability.
        store.compact()?;
        drop(store);
        std::fs::rename(&tmp, path).map_err(StoreError::Io)?;
        Ok(())
    }

    /// Run `f` inside a write transaction. An `Err` from `f` writes
    /// nothing provided the rollback succeeds. On Kyoto Cabinet the
    /// rollback's own error is discarded, so a failed rollback can leave
    /// the closure's writes in place (see [`WriteTxn`]). `Ok` starts the
    /// commit. A failure or crash during commit can leave part of the
    /// batch applied, as in libpinyin. See
    /// [`WriteTxn`] for links to each backend's "Atomicity" section.
    /// The closure sees its own writes.
    ///
    /// # Durability
    ///
    /// A commit that returns `Ok` has left the process on every backend: it
    /// is visible to any other reader and survives a **process** crash.
    /// That is the floor the seam guarantees, and it is what `write`
    /// promises — no more.
    ///
    /// **Stable storage is `WriteStore::compact`'s guarantee, not
    /// this one.** The Kyoto Cabinet and tkrzw backends commit with a
    /// soft `kcdbsync` / `Synchronize` and hard-sync in `compact`.
    /// Berkeley DB exceeds the floor: it has no softer primitive to
    /// offer — `DB->sync` is the only flush libdb gives an
    /// environment-less handle, while skipping it would strand the
    /// batch in a private mpool no other process can read. Callers must
    /// not read any of that as the contract.
    ///
    /// Profile saves compact the in-memory session store. They write through
    /// [`WriteStore::write_user_index`] and the user bigram's save path. The
    /// synchronization of those files depends on the backend. A successful save does not
    /// guarantee that the whole profile has reached stable storage.
    ///
    /// The closure must not call [`WriteStore::write`] again. Backends may
    /// serialize write transactions, so a nested call can block forever.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the transaction or its callback fails.
    fn write<R>(
        &self,
        f: impl FnOnce(&mut Self::Txn<'_>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError>;

    /// Removes one raw record without a transaction or a sync.
    ///
    /// # Errors
    /// Returns a backend error on failure, including a read-only store.
    #[doc(hidden)]
    fn remove_raw(&self, key: &[u8]) -> Result<bool, StoreError>;

    /// Whether writes are disabled for the native handle.
    #[doc(hidden)]
    fn is_read_only(&self) -> bool;

    /// The pin's `FlexibleBigram` writes: the closure's puts and removes,
    /// applied as one native `set` or `remove` each when the closure
    /// returns `Ok`, with no transaction and no sync. An `Err` from the
    /// closure applies nothing. [`WriteStore::flush`] is the one sync, at
    /// the owner's reset (`flexible_ngram_bdb.h:51-57`,
    /// `flexible_ngram_kyotodb.h:86-92`, `flexible_ngram_tkrzwdb.h:84-90`).
    ///
    /// The flag aggregates the applied removals: true unless one of them
    /// found no record, and true when the closure removes nothing. It is
    /// not one native result. `FlexibleBigram::remove` is its only reader,
    /// and its closure holds exactly one removal, so there the flag is that
    /// removal's native answer, which the pin's `remove` returns.
    /// A store that holds no such container refuses with
    /// [`StoreError::InvalidInput`].
    ///
    /// # Errors
    ///
    /// As [`WriteStore::write`].
    fn write_deferred<R>(
        &self,
        f: impl FnOnce(&mut Self::Txn<'_>) -> Result<R, StoreError>,
    ) -> Result<(R, bool), StoreError>;

    /// Synchronizes the native container.
    /// # Errors
    ///
    /// Returns [`StoreError`] when the flush fails.
    fn flush(&self) -> Result<(), StoreError>;

    /// Perform backend-dependent compaction work, and put the store on
    /// **stable storage**.
    ///
    /// Berkeley DB syncs
    /// instead — `DB->compact` wants a transaction this backend never
    /// opens — and the file does not shrink. Kyoto Cabinet and tkrzw hard-sync here —
    /// see [`WriteStore::write`]'s durability note, which explains why
    /// the device-level sync sits on this call and not on every commit
    /// for those two. Berkeley DB has already paid it at every commit,
    /// so for that peer this call adds no durability.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when compaction fails.
    #[cfg(any(test, feature = "test-hooks"))]
    fn compact(&mut self) -> Result<(), StoreError>;
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
    /// the library's bucket order, not ascending byte order. Tree
    /// containers fall back to [`RawReadStore::range_raw`]'s ascending
    /// walk.
    ///
    /// The default is the ascending walk, which is a tree container's own
    /// order. The hash peers override it so a caller that must reproduce
    /// the pin's collection order does not substitute a sorted one.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend walk fails. Keys visited before
    /// the failure stay in `keys`.
    fn native_keys(&self, keys: &mut Vec<Vec<u8>>) -> Result<(), StoreError> {
        self.range_raw(Bound::Unbounded, Bound::Unbounded, |key, _value| {
            keys.push(key.to_vec());
            Ok(())
        })
    }

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
    /// while the other DBM files use TreeDB/TreeDBM. The default
    /// implementation delegates to [`RawReadStore::open_read_only`] (correct
    /// for a backend with no hash/tree distinction). KC and
    /// Tkrzw override this to select the hash container class.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend open fails.
    fn open_hash_read_only(path: &std::path::Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::open_read_only(path)
    }

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

    /// Opens the **user** bigram container at `path` for reading.
    ///
    /// This is *not* [`Self::open_hash_read_only`], and the two must not
    /// share one implementation: libpinyin keeps its system and user
    /// bigrams in different containers on Kyoto Cabinet. The system
    /// `bigram.db` is a `HashDB` file (`attach`, `ngram_kyotodb.cpp:110`),
    /// while the user `user_bigram.db` is a snapshot stream of an
    /// in-memory `StashDB` (`load_db`'s `load_snapshot`,
    /// `ngram_kyotodb.cpp:54-64`) — magic `KCSS`, which a hash open
    /// rejects with "missing magic data of the file". On tkrzw both are
    /// genuine hash files (`ngram_tkrzwdb.cpp:48-63`), so the default —
    /// the hash container — is correct for both.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend open fails.
    #[cfg(any(test, feature = "test-hooks"))]
    fn open_user_bigram(path: &std::path::Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::open_hash_read_only(path)
    }
}

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
pub(crate) trait UserBigramDb: Sized {
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
    /// with [`RawReadStore::get_raw`]'s buffer contract — cleared, filled on a
    /// hit, and the answer is whether `key` was found. The pin reads into
    /// the chunk the `Bigram` owns and reuses (`ngram_kyotodb.cpp:136-139`
    /// at 074a2219).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the read fails.
    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError>;

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
    fn is_empty(&self) -> Result<bool, StoreError> {
        Ok(self.keys()?.is_empty())
    }

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

/// Removes `path`; absence is not an error — libpinyin's
/// `unlink`-before-create, which ignores `ENOENT`.
pub(crate) fn remove_if_present(path: &Path) -> Result<(), StoreError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(StoreError::Io(error)),
    }
}

/// `<name>.<ext>.tmp` beside `path` — the staged-replacement sibling
/// [`WriteStore::write_user_bigram`] builds before the atomic rename.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn sibling_temp(path: &Path) -> std::path::PathBuf {
    let mut name = path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(".tmp");
    path.with_file_name(name)
}

// ── The default backend: compile-time selection ───────────────────────
//
// One backend per oxpinyin binary. The backend implementations
// (Kyoto Cabinet, tkrzw, Berkeley DB) are peers behind the
// store's trait interface, so `DefaultStore` resolves to a single
// concrete type at compile time and everything above it is already
// generic over `RawReadStore` / `WriteStore`. The cfg chain below is
// exactly that selection: it picks the enabled backend feature; a
// multi-feature build resolves deterministically along the chain order
// (kyotocabinet > tkrzw > bdb). The chain order is a
// tie-break for the additive unification, not a hierarchy — tkrzw
// is only the enabled feature that the workspace's default set carries,
// and any single `--features <backend>` on `--no-default-features`
// selects that backend's peer implementation instead.

// Each `cfg(feature = ...)` block below is exclusive — the exactly-one-
// backend guards at the top of this file refuse builds where more than
// one feature is enabled — so a build compiles at most one `DefaultStore`
// alias and one `DEFAULT_STORE_EXT` constant.

/// The default store backend — Kyoto Cabinet, on
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

/// The compiled-in backend's [`UserBigramDb`].
#[cfg(feature = "kyotocabinet")]
pub(crate) type DefaultUserBigramDb = KcUserBigramDb;
/// The compiled-in backend's [`UserBigramDb`].
#[cfg(feature = "tkrzw")]
pub(crate) type DefaultUserBigramDb = TkrzwUserBigramDb;
/// The compiled-in backend's [`UserBigramDb`].
#[cfg(feature = "bdb")]
pub(crate) type DefaultUserBigramDb = BdbUserBigramDb;

/// File extension for [`DefaultStore`]'s native tables — one per peer;
/// the store forces its database type through open parameters, so the
/// extension is naming, not detection.
#[cfg(any(test, feature = "test-hooks"))]
#[cfg(feature = "kyotocabinet")]
pub const DEFAULT_STORE_EXT: &str = "kct";
/// File extension for [`DefaultStore`]'s native tables (tkrzw `TreeDBM`).
#[cfg(any(test, feature = "test-hooks"))]
#[cfg(feature = "tkrzw")]
pub const DEFAULT_STORE_EXT: &str = "tkt";
/// File extension for [`DefaultStore`]'s native tables (Berkeley DB).
/// The DBM files libpinyin itself uses carry its own names
/// (`bigram.db`, `pinyin_index.bin`, …); this extension names this
/// backend's session-scratch containers and the `fixtures/w3`
/// directory of this backend.
#[cfg(any(test, feature = "test-hooks"))]
#[cfg(feature = "bdb")]
pub const DEFAULT_STORE_EXT: &str = "db";

/// The backend's `database format:` token — the string `user.conf`'s
/// conformance check compares (`table_info.cpp`'s
/// `to/from_table_database_format_type`: exactly `BerkeleyDB`,
/// `KyotoCabinet`, `Tkrzw` upstream), so a same-backend pair stays
/// conform and every cross-backend pair answers non-conform. The build
/// script selects it once from the backend feature, the pin's
/// `DATABASE_FORMAT`. Both `.pc` files carry the same token, from
/// `tools/dev/cell.py`.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb"))]
pub const DEFAULT_STORE_DB_FORMAT: &str = env!("OXPINYIN_STORE_DATABASE_FORMAT");

/// Helpers shared by the backends' raw walks and path checks; every item is
/// gated to the backends that use it (see the module docs).
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw", feature = "bdb"))]
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
pub use tkrzw::{TkrzwStore, TkrzwTxn, TkrzwUserBigramDb};

#[cfg(feature = "bdb")]
mod bdb;
#[cfg(feature = "bdb")]
pub use bdb::{BdbStore, BdbTxn, BdbUserBigramDb};

#[cfg(feature = "kyotocabinet")]
pub mod kyotocabinet;
#[cfg(feature = "kyotocabinet")]
pub use kyotocabinet::{KcStore, KcTxn, KcUserBigramDb};

mod system_attach;
pub use system_attach::SystemAttachment;

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
