//! Berkeley DB 5.3 backend — libpinyin's original DBM, and the third
//! drop-in set (drop-in task 10).
//!
//! Enabled by the `bdb` cargo feature. One type, [`BdbStore`], serves
//! the raw keyspace of libpinyin's own files behind
//! [`crate::RawReadStore`] / [`crate::WriteStore`], each file one database
//! opened with no subdatabase name: `create_hash`/`open_hash_read_only`
//! are the `DB_HASH` form of `bigram.db`/`user_bigram.db`, and the
//! `put_raw`/`get_raw`/`range_raw` seam is what `stage_dbm`'s tree
//! containers (`pinyin_index.bin`, `phrase_index.bin`, `punct.bin`), the
//! session scratch store and the user-dir bigram writer are written
//! through. On this backend the system and user bigrams are the same
//! container — a `DB_HASH` file — so the trait's default
//! `open_user_bigram` (delegate to `open_hash_read_only`) is correct and
//! neither override that Kyoto Cabinet needs applies.
//!
//! # Key ordering
//!
//! `DB_BTREE` opened without `set_bt_compare` — which is how libpinyin
//! opens `phrase_large_table3_bdb.cpp` and
//! `chewing_large_table2_bdb.cpp` — uses Berkeley DB's default
//! comparator: byte-wise `memcmp`, shorter key first on a shared
//! prefix. That is the store's one rule exactly, so this backend
//! satisfies it without configuration, and setting a comparator would
//! silently reorder files libpinyin wrote.
//!
//! Confirmed experimentally rather than taken from the documentation
//! (`docs/findings/berkeleydb-backend.md`): a `DB_BTREE` loaded with
//! little-endian `u32` array keys that cross 256 in the first and in a
//! later element walks in raw-byte order, not integer order — the
//! 1-element key `00000001` immediately precedes the 2-element key
//! `0000000102010000` that extends it, and the decoded values run
//! `0x01000000, 0x00010000, 0x00ff0000, 0x00000100, …`, which is not
//! ascending. The shared key-ordering suite asserts this backend walks
//! identically to tkrzw and Kyoto Cabinet over exactly
//! those keys.
//!
//! # Atomicity
//!
//! Weak, and for the same reason as tkrzw's.
//! libpinyin uses no Berkeley DB environment and no transactions — every
//! `open` passes `NULL` for both — so a standalone `DB` handle has no
//! transaction to commit. [`crate::WriteStore::write`] therefore buffers
//! the closure's puts and removes, answers in-closure reads from that
//! buffer over the database, and applies the buffer in one pass on `Ok`;
//! on `Err` the buffer is dropped and nothing is written. A crash
//! *during* that apply can leave part of a batch on disk.
//!
//! Durability is the other way round: it is *stronger* here, not weaker.
//! Each commit ends with `DB->sync`, which is the only flush libdb
//! offers an environment-less handle, so once `write` returns the batch
//! is on the device rather than merely in the operating system's hands.
//! This backend cannot offer the soft commit tier Kyoto Cabinet and
//! tkrzw use; [`crate::WriteStore::compact`] calls the same `DB->sync`
//! and so owes nothing `write` has not already paid. The sync does not
//! repair the atomicity: the apply pass writes one record at a time and
//! the sync follows it, so a crash mid-apply leaves part of the batch
//! applied — and a crash that tears one of those page writes can
//! corrupt the database outright, which re-opening does not repair;
//! recovery is from a known-good copy.
//!
//! Matching libpinyin here is the point: a transactional environment
//! would write log and region files beside the user's profile, which
//! the user's own libpinyin does not expect and would not clean up.
//!
//! # Threading
//!
//! [`BdbStore`] is neither `Send` nor `Sync`. The C contract serializes
//! the calls that reach its standalone handle, opened with the pin's
//! flags: one call at a time on a context and all its handles. Cursor
//! reads land in caller-owned (`DB_DBT_USERMEM`) buffers, so a visitor can
//! read a row or call the store again; a point read is copied out of the
//! handle's own return buffer before the call returns.
//!
//! # Unsafe waiver
//!
//! This module carries none. Every `unsafe` in the backend — the `DB`
//! and `DBC` member calls,
//! the generated declarations — is confined to `ffi`, whose
//! `expect(unsafe_code)` is scoped to that module, so the safe wrapper
//! here stays under the workspace `deny`.

mod ffi;

use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::ops::Bound;
use std::path::Path;

use crate::common::{Row, in_bounds};
use crate::{AttachMode, Flow, HashAttach, RawReadStore, StoreError, WriteStore, WriteTxn};

use ffi::{Db, Seek};

/// A Berkeley DB store: a `DB_BTREE` container when opened by
/// [`RawReadStore::open_read_only`] / [`WriteStore::create`], a `DB_HASH`
/// raw keyspace when opened by the hash constructors.
pub struct BdbStore {
    db: Db,
}

/// A Berkeley DB attach whose `DB->open` failed. The pin keeps that
/// handle and calls it again (`chewing_large_table2_bdb.cpp:87-93`,
/// `ngram_bdb.cpp:40-42`, `flexible_ngram_bdb.h:110-146` at 074a2219), and
/// libdb answers each call with `BDB1565`. Berkeley DB requires
/// `DB->close` after a failed open and no other call on the handle
/// (`dbopen.html`, `dbclose.html`, 5.3.28), so `Db::open` has already
/// closed it: nothing here reaches libdb (compatibility register,
/// Berkeley DB failed handles).
pub struct BdbUnopenedHandle {
    _closed: (),
}

impl BdbUnopenedHandle {
    /// `Bigram::store`'s answer on the failed handle: the pin's `DB->put`
    /// gets `EINVAL` and `store` returns false (`ngram_bdb.cpp:148-163`).
    pub(crate) fn write(&self, _key: &[u8], _value: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::Backend(
            "DB->put: method not permitted before handle's open method".into(),
        ))
    }

    /// `Bigram::remove`'s answer on the failed handle: the pin's `DB->del`
    /// gets `EINVAL` and `remove` returns false (`ngram_bdb.cpp:165-176`).
    pub(crate) fn delete(&self, _key: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::Backend(
            "DB->del: method not permitted before handle's open method".into(),
        ))
    }

    /// `Bigram::get_all_items`' answer on the failed handle: the pin's
    /// `DB->cursor` leaves no cursor and the walk returns false
    /// (`ngram_bdb.cpp:178-191`).
    pub(crate) fn walk(&self) -> Result<(), StoreError> {
        Err(StoreError::Backend(
            "DB->cursor: method not permitted before handle's open method".into(),
        ))
    }
}

/// The pin's `attach` answer: an opened store, or a failed open.
pub enum BdbOpen {
    /// `DB->open` succeeded.
    Open(BdbStore),
    /// `DB->open` failed and its handle is closed.
    Unopened(BdbUnopenedHandle),
}

pub(crate) fn open_pin(
    path: &Path,
    hash: bool,
    read_only: bool,
) -> Result<BdbOpen, crate::StoreError> {
    let db_type = if hash { ffi::DB_HASH } else { ffi::DB_BTREE };
    Ok(
        match Db::open_attach(path, db_type, read_only, !read_only, ffi::SYSTEM_FILE_MODE)? {
            Some(db) => BdbOpen::Open(BdbStore { db }),
            None => BdbOpen::Unopened(BdbUnopenedHandle { _closed: () }),
        },
    )
}

impl BdbStore {
    /// The tools' plain native `DB->put`, without syncing. Matches
    /// `ngram_bdb.cpp:161` and `punct_table_bdb.cpp:176` at 074a2219, the
    /// peers' `KcStore::tool_set` and `TkrzwStore::tool_set` (each exists
    /// only under its own feature, so neither is linked, #1129). The shipping
    /// transaction and durability path is unchanged.
    ///
    /// # Errors
    /// Returns the native write error, or an error for a read-only handle.
    #[cfg(feature = "tools")]
    pub fn tool_set(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        self.db.put(key, value)
    }

    /// Applies a finished transaction's buffer and syncs. Not generic, so
    /// the many `write` closures share one copy of it.
    #[inline(never)]
    fn commit(&self, buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>) -> Result<(), StoreError> {
        self.commit_with(buffer, true).map(drop)
    }

    /// Applies `buffer`, one `DB->put` or `DB->del` per record, then syncs
    /// when `sync`. The answer is false when a `DB->del` found no record.
    fn commit_with(
        &self,
        buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
        sync: bool,
    ) -> Result<bool, StoreError> {
        let mut found = true;
        for (key, value) in buffer {
            match value {
                Some(value) => self.db.put(&key, &value)?,
                None => found &= self.db.del(&key)?,
            }
        }
        // Push the batch out, so a reader — including the user's own
        // libpinyin — sees a consistent file once `write` returns.
        // Without it the pages stay in the private mpool of a handle
        // opened with no environment, where no other process can reach
        // them. `DB->sync` is the only flush available here and it goes
        // all the way to the device, so this commit exceeds the seam's
        // floor; see `WriteStore::write`'s durability note.
        if sync {
            self.db.sync()?;
        }
        Ok(found)
    }

    /// The raw keyspace in ascending key order — the ordered (`DB_BTREE`)
    /// half of [`Self::range_raw`]: one cursor from the lower bound,
    /// streaming rows and stopping at the first key above `hi`.
    fn range_raw_ordered(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor()?;
        let mut seek = match lo {
            Bound::Unbounded => Seek::First,
            Bound::Included(key) | Bound::Excluded(key) => Seek::AtOrAfter(key),
        };
        loop {
            let row = match cursor.get(seek)? {
                None => return Ok(()),
                Some(row) => row,
            };
            seek = Seek::Next;
            if matches!(lo, Bound::Excluded(bound) if row.key == bound) {
                continue;
            }
            if !in_bounds(row.key, Bound::Unbounded, hi) {
                return Ok(());
            }
            if visit(row.key, row.value)?.is_break() {
                return Ok(());
            }
        }
    }

    /// Every row of the file, sorted by key. Not generic, so the many
    /// visitors share one copy of the cursor loop and the sort.
    #[inline(never)]
    fn sorted_rows(&self) -> Result<Vec<Row>, StoreError> {
        let mut cursor = self.db.cursor()?;
        let mut seek = Seek::First;
        let mut rows: Vec<Row> = Vec::new();
        loop {
            let (key, value) = match cursor.get(seek)? {
                None => break,
                Some(row) => (row.key.to_vec(), row.value.to_vec()),
            };
            seek = Seek::Next;
            rows.push((key, value));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(rows)
    }

    /// The unordered (`DB_HASH`) half of [`Self::range_raw`]: collect
    /// every row, sort by key, apply the bounds here. A hash cursor
    /// walks in bucket order, so this is the only correct shape — the
    /// same discipline the Kyoto Cabinet backend applies to its hash
    /// containers.
    fn range_raw_unordered(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        for (key, value) in self.sorted_rows()? {
            // Sorted, so rows below the lower bound come first and are
            // skipped (`in_bounds` covers the Excluded-equality case: a
            // key equal to an excluded bound is not above it), and the
            // first row above the upper bound ends the walk.
            if !in_bounds(&key, lo, Bound::Unbounded) {
                continue;
            }
            if !in_bounds(&key, Bound::Unbounded, hi) {
                return Ok(());
            }
            if visit(&key, &value)?.is_break() {
                return Ok(());
            }
        }
        Ok(())
    }
}

impl RawReadStore for BdbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_BTREE, true, false, ffi::SYSTEM_FILE_MODE)?,
        })
    }

    fn get_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.db.get(key, value)
    }

    fn get_raw_partial(
        &self,
        key: &[u8],
        offset: u32,
        length: u32,
        value: &mut Vec<u8>,
    ) -> Result<bool, StoreError> {
        self.db.get_partial(key, offset, length, value)
    }

    fn contains_raw(&self, key: &[u8]) -> Result<bool, StoreError> {
        self.db.exists(key)
    }

    fn scan_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        if self.db.is_hash() {
            self.range_raw_unordered(lo, hi, visit)
        } else {
            self.range_raw_ordered(lo, hi, visit)
        }
    }

    /// `Bigram::get_all_items`' `DB_NEXT` cursor walk (`ngram_bdb.cpp:143-172`):
    /// libdb's own record order, which for `DB_HASH` is bucket order.
    fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut keys = Vec::new();
        let mut cursor = self.db.cursor()?;
        let mut seek = ffi::Seek::First;
        while let Some(key) = cursor.key(seek)? {
            keys.push(key.to_vec());
            seek = ffi::Seek::Next;
        }
        Ok(keys)
    }

    fn open_hash_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_HASH, true, false, ffi::SYSTEM_FILE_MODE)?,
        })
    }

    /// `copy_bdb(tmp_db, m_db)` in `ChewingLargeTable2::load_db` and
    /// `PhraseLargeTable3::load_db` (`chewing_large_table2_bdb.cpp:99-133`,
    /// `phrase_large_table3_bdb.cpp:114-148` at 074a2219): the file's cursor
    /// walk into the in-memory table. A cursor that does not open is its
    /// own error; a failed copy is [`StoreError::FatalCopy`].
    fn copy_raw(&self, mut visit: impl FnMut(&[u8], &[u8])) -> Result<(), StoreError> {
        copy_bdb(&self.db, |key, value| {
            visit(key, value);
            Ok(())
        })
    }
}

impl WriteStore for BdbStore {
    type Txn<'a> = BdbTxn<'a>;

    /// A system table, created as `attach` creates one: mode 0644
    /// (`ffi::SYSTEM_FILE_MODE`).
    fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_BTREE, false, true, ffi::SYSTEM_FILE_MODE)?,
        })
    }

    /// The `DB_HASH` container: libpinyin's `bigram.db` form, the hash
    /// half of this backend's raw seam (see [`RawReadStore`]).
    fn create_hash(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_HASH, false, true, ffi::SYSTEM_FILE_MODE)?,
        })
    }

    /// `FlexibleBigram::attach` (`flexible_ngram_bdb.h:94-153` at
    /// 074a2219) with Berkeley DB's documented handle sequence. The pin
    /// retries `DB->open` on its failed handle (`:110-115`) and reads the
    /// signature through a handle whose open failed (`:134-146`). Berkeley
    /// DB requires `DB->close` after a failed `DB->open`, and no other call
    /// on that handle (`dbopen.html`, `dbclose.html`, 5.3.28). So each
    /// failed open closes its handle, the CREATE retry uses a new one, and
    /// a failed attach keeps nothing. The difference is libdb's missing
    /// `BDB1565` lines (compatibility register, Berkeley DB failed handles).
    fn attach_hash(
        path: &Path,
        mode: AttachMode,
        signature: (&[u8], &[u8]),
    ) -> Result<HashAttach<Self>, StoreError> {
        let read_only = mode == AttachMode::ReadOnly;
        // `Ok(None)`: the native `DB->open` failed and its handle is closed.
        let open = |create| {
            Db::open_attach(path, ffi::DB_HASH, read_only, create, ffi::SYSTEM_FILE_MODE)
                .map(|db| db.map(|db| Self { db }))
        };
        if let Some(store) = open(false)? {
            return Ok(HashAttach::Opened(store));
        }
        if mode != AttachMode::ReadWriteCreate {
            return Ok(HashAttach::Closed);
        }
        let Some(store) = open(true)? else {
            return Ok(HashAttach::Closed);
        };
        // The pin's `DB_DBT_PARTIAL` put at offset 0 of a new record stores
        // the same four bytes; its answer is the attach answer (`:118-131`).
        let (key, value) = signature;
        let written = store.db.put(key, value).is_ok();
        Ok(HashAttach::Created(store, written))
    }

    /// libdb's in-memory database (`DB->open` with a null file name, the
    /// form `ngram_bdb.cpp:51-56` uses for the user bigram), as a
    /// `DB_BTREE` so the ordered raw walks hold.
    fn set_raw(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db.put(key, value)
    }

    fn rewrite_raw(&self, rewrite: &mut crate::RecordRewrite<'_>) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor()?;
        let mut replacement = Vec::new();
        while let Some(row) = cursor.get(Seek::Next)? {
            replacement.clear();
            rewrite(row.key, row.value, &mut replacement);
            cursor.put_current(&replacement)?;
        }
        Ok(())
    }

    fn create_in_memory() -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_in_memory_tree()?,
        })
    }

    /// A user index table, created as `save_db` creates one: mode 0600
    /// (`ffi::USER_FILE_MODE`).
    fn create_user(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_BTREE, false, true, ffi::USER_FILE_MODE)?,
        })
    }

    /// The user bigram, created as `Bigram::save_db` creates it: a
    /// `DB_HASH` file, mode 0600 (`ffi::USER_FILE_MODE`).
    fn create_user_hash(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_HASH, false, true, ffi::USER_FILE_MODE)?,
        })
    }

    /// `ChewingLargeTable2::save_db` / `PhraseLargeTable3::save_db`
    /// (`chewing_large_table2_bdb.cpp:135-162`,
    /// `phrase_large_table3_bdb.cpp:150-177` at 074a2219): unlink, create a
    /// `DB_BTREE` file (`DB_CREATE, 0600`), `copy_bdb`'s one `put` per
    /// record in key order (`bdb_utils.h:43-74`), then `close`. The pin's
    /// `tmp_db->sync(m_db, 0)` syncs its in-memory source table, which
    /// oxpinyin does not hold, so the new file is not synced before close.
    fn write_user_index(
        path: &Path,
        records: impl FnOnce(&mut crate::RecordSink<'_>) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        crate::remove_if_present(path)?;
        let file = Db::open(path, ffi::DB_BTREE, false, true, ffi::USER_FILE_MODE)?;
        // `copy_bdb` asserts on a failed `put` or source walk.
        crate::copy::copy(records, |key, value| file.put(key, value))?;
        file.close_unsynced();
        Ok(())
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut Self::Txn<'_>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        // A read-only store refuses the whole transaction, not merely the
        // writes inside it: the tier contract is that `write` on a
        // read-only handle is `ReadOnly` even when the closure happens to
        // write nothing, which the shared write suite asserts.
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        let mut txn = BdbTxn {
            store: PhantomData,
            buffer: BTreeMap::new(),
        };
        let out = f(&mut txn)?;
        self.commit(txn.buffer)?;
        Ok(out)
    }

    /// `FlexibleBigram::store` and `remove` (`flexible_ngram_bdb.h:
    /// 198-214`, `:224-235` at 074a2219): the `DB->put` or `DB->del` alone,
    /// its answer the pin's. `reset()` makes the one `DB->sync`
    /// (`:51-57`), which [`WriteStore::flush`] is.
    fn write_deferred<R>(
        &self,
        f: impl FnOnce(&mut Self::Txn<'_>) -> Result<R, StoreError>,
    ) -> Result<(R, bool), StoreError> {
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        let mut txn = BdbTxn {
            store: PhantomData,
            buffer: BTreeMap::new(),
        };
        let out = f(&mut txn)?;
        let found = self.commit_with(txn.buffer, false)?;
        Ok((out, found))
    }

    fn flush(&self) -> Result<(), StoreError> {
        self.db.sync()
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        // Berkeley DB keeps a free-page list and reuses freed pages in
        // place; `DB->compact` exists in 5.3 but wants to move data under
        // a transaction, which this backend deliberately does not open
        // (see the module note). Stable storage is what `compact` owes —
        // `DB->sync` — and the file not shrinking mirrors Kyoto
        // Cabinet's compact, which also only syncs.
        self.db.sync()
    }
}

/// The buffered write transaction described in the module note.
pub struct BdbTxn<'store> {
    /// The transaction borrows its store for its lifetime.
    store: PhantomData<&'store BdbStore>,
    /// Raw key → new value, or `None` for a removal. Ordered so the apply
    /// pass touches keys in B-tree order.
    buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl WriteTxn for BdbTxn<'_> {
    // The point operations stay out of line: the user store calls them from
    // hundreds of sites, and inlining each one's buffer insert into every
    // caller costs far more code than a direct call. Dispatch is static
    // either way.

    /// The bare-keyspace half of the raw seam: what `stage_dbm`'s tree
    /// containers and the user-bigram writer write through, and what
    /// `get_raw`/`range_raw` read back on this backend.
    #[inline(never)]
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.buffer.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    /// The removal half of [`WriteTxn::put_raw`].
    #[inline(never)]
    fn remove_raw(&mut self, key: &[u8]) -> Result<(), StoreError> {
        self.buffer.insert(key.to_vec(), None);
        Ok(())
    }
}

// ── the user bigram's in-memory container ─────────────────────────

/// libpinyin's Berkeley DB user bigram as `Bigram::load_db` leaves it:
/// an in-memory `DB_HASH` filled from the file by `copy_bdb`'s cursor
/// walk (`ngram_bdb.cpp:47-79`, `bdb_utils.h:43-74`), so its walk is the
/// pin's.
///
/// When the in-memory `DB->open` fails, `load_db` returns before the file
/// is read and the pin carries on with that handle (`ngram_bdb.cpp:54-57`;
/// `pinyin.cpp:401` and `zhuyin.cpp:343` ignore the answer). This is the
/// failed container: its handle is closed ([`BdbUnopenedHandle`], register
/// row 113), and each call answers as the pin's call on the failed handle
/// does.
pub struct BdbUserBigramDb {
    db: Result<Db, BdbUnopenedHandle>,
}

/// `copy_bdb(srcdb, destdb)` (`bdb_utils.h:43-74` at 074a2219): the
/// records of `src` in its cursor order, each handed to `set`. A cursor
/// that does not open is the pin's soft `return false` (`:49-52`): its own
/// error. A failed `put` or a cursor walk that ends in an error is
/// [`StoreError::FatalCopy`], where the copy asserts (`:61`, `:67`).
fn copy_bdb(
    src: &Db,
    set: impl FnMut(&[u8], &[u8]) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    #[cfg(feature = "test-hooks")]
    crate::copy::faults::make_iterator()?;
    let mut cursor = src.cursor()?;
    crate::copy::copy(
        |visit| {
            let mut seek = ffi::Seek::First;
            while let Some(row) = cursor.get(seek)? {
                visit(row.key, row.value)?;
                seek = ffi::Seek::Next;
            }
            Ok(())
        },
        set,
    )
}

impl BdbUserBigramDb {
    /// The failed container `load_db` leaves when its in-memory `DB->open`
    /// fails (`ngram_bdb.cpp:54-57`).
    const fn failed() -> Self {
        Self {
            db: Err(BdbUnopenedHandle { _closed: () }),
        }
    }

    /// [`Self::failed`] for the user store's tests of a session over it: no
    /// input makes the in-memory open fail short of memory. Test-only: it
    /// exists only with the `test-hooks` feature, which only a
    /// `[dev-dependencies]` entry enables.
    #[cfg(feature = "test-hooks")]
    #[must_use]
    pub const fn failed_container() -> Self {
        Self::failed()
    }

    /// [`UserBigramDb::load_db`] over `create`, the in-memory container's
    /// open: [`Db::open_in_memory_hash_attach`], or an injected failure.
    fn load_db_with(
        path: &Path,
        create: impl FnOnce() -> Result<Option<Db>, StoreError>,
    ) -> Result<Self, StoreError> {
        let Some(db) = create()? else {
            // `return false` before `tmp_db` is created: the file is not read.
            return Ok(Self::failed());
        };
        // `tmp_db->open(…, dbfile, NULL, DB_HASH, DB_RDONLY, 0600)` then
        // `copy_bdb(tmp_db, m_db)`; a file that does not open leaves the
        // container empty, as the pin's does.
        if let Ok(file) = Db::open(path, ffi::DB_HASH, true, false, ffi::USER_FILE_MODE) {
            // A cursor that does not open leaves the container as it is:
            // `load_db` returns false and its callers ignore the answer. A
            // failed copy is where the pin asserts.
            if let Err(StoreError::FatalCopy) = copy_bdb(&file, |key, value| db.put(key, value)) {
                return Err(StoreError::FatalCopy);
            }
        }
        Ok(Self { db: Ok(db) })
    }
}

impl crate::UserBigramDb for BdbUserBigramDb {
    fn empty() -> Result<Self, StoreError> {
        Ok(match Db::open_in_memory_hash_attach()? {
            Some(db) => Self { db: Ok(db) },
            None => Self::failed(),
        })
    }

    fn load_db(path: &Path) -> Result<Self, StoreError> {
        Self::load_db_with(path, Db::open_in_memory_hash_attach)
    }

    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        match &self.db {
            Ok(db) => db.get(key, value),
            // `Bigram::load`'s `DB->get` fails, so `load` answers no gram
            // (`ngram_bdb.cpp:140-142`).
            Err(_) => {
                value.clear();
                Ok(false)
            }
        }
    }

    fn store(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        match &self.db {
            Ok(db) => db.put(key, value),
            Err(handle) => handle.write(key, value),
        }
    }

    fn remove(&self, key: &[u8]) -> Result<(), StoreError> {
        match &self.db {
            Ok(db) => db.del(key).map(drop),
            Err(handle) => handle.delete(key),
        }
    }

    fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError> {
        // `copy_bdb`'s cursor walk without the values: the same cursor order, into
        // one vector of four-byte tokens, as `get_all_items` appends them
        // to one `GArray` (`ngram_bdb.cpp:178-215` at 074a2219). Berkeley
        // DB keeps no record count to reserve by, so it grows as it goes.
        let Ok(db) = &self.db else {
            // `get_all_items` empties the array, then its `DB->cursor` leaves
            // no cursor and it answers false (`ngram_bdb.cpp:178-191`). Every
            // caller ignores that answer and reads the empty array
            // (`pinyin.cpp:779`; `mask_out`'s false at `:1230`, `:3766`,
            // `zhuyin.cpp:765`).
            return Ok(Vec::new());
        };
        let mut keys = Vec::new();
        let mut cursor = db.cursor()?;
        let mut seek = ffi::Seek::First;
        while let Some(key) = cursor.key(seek)? {
            keys.push(<[u8; 4]>::try_from(key).ok());
            seek = ffi::Seek::Next;
        }
        Ok(keys)
    }

    fn is_empty(&self) -> Result<bool, StoreError> {
        let Ok(db) = &self.db else {
            return Ok(true);
        };
        Ok(db.cursor()?.key(ffi::Seek::First)?.is_none())
    }

    const CAN_FAIL: bool = true;

    fn is_failed(&self) -> bool {
        self.db.is_err()
    }

    fn save_db(&self, path: &Path) -> Result<(), StoreError> {
        match &self.db {
            Ok(db) => save_bigram_db(db, path),
            Err(handle) => save_failed_bigram_db(handle, path),
        }
    }
}

/// `Bigram::save_db` over the failed container (`ngram_bdb.cpp:81-108`):
/// unlink, create the `DB_HASH` file (`DB_CREATE, 0600`), whose empty
/// pages libdb writes and syncs as it creates it (`db_open.c:360-361`,
/// Berkeley DB 5.3.28), then `copy_bdb` finds no cursor on the failed
/// handle (`bdb_utils.h:49-52`) and the save answers false. The pin never
/// closes that file's handle; this one is closed, unsynced, as a
/// successful save closes it. The caller's rename still runs, as the
/// pin's does (`pinyin.cpp:1121`, `zhuyin.cpp:687`), so the empty file
/// replaces the profile's `user_bigram.db`.
fn save_failed_bigram_db(handle: &BdbUnopenedHandle, path: &Path) -> Result<(), StoreError> {
    crate::remove_if_present(path)?;
    let file = Db::open(path, ffi::DB_HASH, false, true, ffi::USER_FILE_MODE)?;
    file.close_unsynced();
    handle.walk()
}

fn save_bigram_db(db: &Db, path: &Path) -> Result<(), StoreError> {
    // `Bigram::save_db` (`ngram_bdb.cpp:81-108`): unlink, create a
    // `DB_HASH` file (`DB_CREATE, 0600`), copy this container into it
    // in cursor order, `tmp_db->sync(m_db, 0)` — a sync of this
    // in-memory container, not of the file — then close the file.
    crate::remove_if_present(path)?;
    let file = Db::open(path, ffi::DB_HASH, false, true, ffi::USER_FILE_MODE)?;
    copy_bdb(db, |key, value| file.put(key, value))?;
    // The pin discards this call's return code, and so does this.
    let _sync = db.sync();
    file.close_unsynced();
    Ok(())
}

impl BdbStore {
    /// Save an attached bigram using the same implementation as the
    /// in-memory user bigram (`ngram_{tkrzwdb,kyotodb,bdb}.cpp` at 074a2219).
    ///
    /// # Errors
    /// Returns the existing save operation's file or backend error.
    pub fn save_bigram_db(&self, path: &Path) -> Result<(), StoreError> {
        save_bigram_db(&self.db, path)
    }
}

#[cfg(test)]
mod tests {
    use super::BdbStore;
    use crate::{RawReadStore as _, WriteStore as _, WriteTxn as _};
    use std::ops::{Bound, ControlFlow};

    // A visitor reads the store it is walking, on both an ordered tree and
    // an unordered hash; the raw tier is what the data crate's chewing,
    // phrase, bigram, addon and punctuation readers go through.
    #[test]
    fn raw_readers_keep_rows_valid_during_reentrant_reads() {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-bdb-shared-raw-readers-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        for hash in [false, true] {
            let path = dir.join(if hash { "hash.db" } else { "tree.db" });
            let store = if hash {
                BdbStore::create_hash(&path).unwrap()
            } else {
                BdbStore::create(&path).unwrap()
            };
            store
                .write(|txn| {
                    for key in 0_u8..8 {
                        txn.put_raw(&[key], &vec![key; if key == 3 { 9000 } else { 20 }])?;
                    }
                    Ok(())
                })
                .unwrap();
            drop(store);
            let store = if hash {
                BdbStore::open_hash_read_only(&path).unwrap()
            } else {
                BdbStore::open_read_only(&path).unwrap()
            };
            let mut read = Vec::new();
            for _ in 0..2 {
                assert!(store.get_raw(&[3], &mut read).unwrap());
                assert_eq!(read, [3; 9000]);
                assert!(store.contains_raw(&[3]).unwrap());
                let mut keys = Vec::new();
                store
                    .scan_raw(
                        Bound::Included(&[2]),
                        Bound::Excluded(&[6]),
                        |key, value| {
                            assert!(store.get_raw(key, &mut read).unwrap());
                            assert_eq!(read, value);
                            assert!(value.iter().all(|byte| *byte == key[0]));
                            keys.push(key[0]);
                            Ok(ControlFlow::Continue(()))
                        },
                    )
                    .unwrap();
                assert_eq!(keys, [2, 3, 4, 5]);
            }
            drop(store);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-bdb-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `user_bigram.db` with one gram, saved by a working container.
    fn saved_bigram(dir: &std::path::Path) -> std::path::PathBuf {
        use crate::UserBigramDb as _;
        let path = dir.join("user_bigram.db");
        let db = super::BdbUserBigramDb::load_db(&path).unwrap();
        db.store(&7_u32.to_le_bytes(), &[1, 2, 3, 4]).unwrap();
        db.save_db(&path).unwrap();
        path
    }

    /// `ngram_bdb.cpp:51-52`: `assert(ret == 0)` after `db_create`. The
    /// process dies of SIGABRT, with no Rust panic text or other line.
    #[test]
    fn a_failed_user_bigram_db_create_aborts_where_load_db_asserts() {
        let stderr = crate::death_test::assert_aborts(
            "bdb::tests::a_failed_user_bigram_db_create_aborts_where_load_db_asserts",
            || {
                // The open dies before any file is read, so no directory.
                let _ = super::BdbUserBigramDb::load_db_with(
                    std::path::Path::new("never-read/user_bigram.db"),
                    || super::ffi::Db::failed_in_memory_hash_attach(super::ffi::FailedStep::Create),
                );
            },
        );
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }

    #[test]
    fn a_failed_in_memory_open_leaves_the_pins_failed_container() {
        use crate::UserBigramDb as _;
        let dir = temp_dir("bigram-db-open");
        let path = saved_bigram(&dir);
        // `ngram_bdb.cpp:54-57`: the in-memory `DB->open` fails and
        // `load_db` returns before the file is read; the init carries on.
        let db = super::BdbUserBigramDb::load_db_with(&path, || {
            super::ffi::Db::failed_in_memory_hash_attach(super::ffi::FailedStep::Open)
        })
        .unwrap();
        assert!(db.is_failed());
        let key = 7_u32.to_le_bytes();
        let mut value = vec![9];
        // `load`: a miss, though the file holds the gram (`:140-142`).
        assert!(!db.get(&key, &mut value).unwrap());
        assert!(value.is_empty());
        // `store` and `remove` answer false (`:161-162`, `:174-175`).
        assert!(db.store(&key, &[1, 2, 3, 4]).is_err());
        assert!(db.remove(&key).is_err());
        // `get_all_items`: the emptied array (`:178-191`).
        assert!(db.keys().unwrap().is_empty());
        assert!(db.is_empty().unwrap());

        // `save_db` answers false after it has created the empty hash
        // file (`:84-100`): the caller renames that file into place.
        let tmp = dir.join("user_bigram.db.tmp");
        std::fs::write(&tmp, b"stale").unwrap();
        assert!(db.save_db(&tmp).is_err());
        let saved = super::BdbUserBigramDb::load_db(&tmp).unwrap();
        assert!(!saved.is_failed());
        assert!(saved.keys().unwrap().is_empty());
        // The profile's own file is not touched by the save.
        let kept = super::BdbUserBigramDb::load_db(&path).unwrap();
        assert_eq!(kept.keys().unwrap(), vec![Some(key)]);
        drop((db, saved, kept));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
