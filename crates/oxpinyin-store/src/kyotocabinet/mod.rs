//! Kyoto Cabinet backend for the store's peer set.
//!
//! Enabled by the `kyotocabinet` cargo feature (`--no-default-features
//! --features kyotocabinet`); the workspace's default selected backend is
//! tkrzw.
//! [`KcStore`] is a `TreeDB` implementation of the store's two capability
//! tiers, addressed by `(table, key)` like every other backend.
//!
//! # The class is chosen by the caller, never by the file name
//!
//! The backend calls Kyoto Cabinet through a cxx bridge (`bridge`, behind
//! `ffi`), which constructs the class the caller names, as libpinyin
//! does, and opens the plain path. oxpinyin names its
//! native tables `.kct` (the Kyoto Cabinet TreeDB convention), but nothing
//! reads that suffix. `kclangc.h`'s `PolyDB` would pick the class from it,
//! and fail outright on libpinyin's `.db` names (`kclangc.h:312-320`).
//!
//! # Key ordering
//!
//! `TreeDB` with no `rcomp` tuning parameter uses Kyoto Cabinet's default
//! record comparator, `LEXICALCOMP` — byte-wise, shorter key first on a
//! shared prefix. The cross-backend conformance tests in `super` assert
//! this backend walks identically to the other peers over keys
//! that cross 256 in the first and in a later element — where byte order
//! and integer order genuinely differ.
//!
//! # Atomicity
//!
//! Kyoto Cabinet gives a standalone handle real transactions
//! (`begin_transaction`/`end_transaction`), so [`WriteStore::write`] is the
//! library's own transaction rather than a buffered imitation: the
//! closure's writes go straight to the database inside the transaction,
//! reads see them, and an `Err` rolls the whole thing back.
//!
//! Commits end with `synchronize(false)`: the commit is pushed to the
//! operating system, so another process — or another handle in this one
//! — observes it and it survives a process crash, but it is not yet on
//! the device. [`WriteStore::compact`] is the hard `synchronize(true)`,
//! the one point at which a file-backed store reaches stable storage.
//! The tkrzw backend documents the same split and the measurement
//! behind it (`docs/findings/perf-train-commit-fsync-2026-09-09.md`).
//!
//! `compact` is no longer where `pinyin_save` reaches storage, though.
//! `UserStore::save` still calls it, but since the libpinyin session
//! moved into memory (#621) the store it compacts is the `ProtoTreeDB`
//! of [`WriteStore::create_in_memory`], which has no file. The profile
//! `pinyin_save` writes goes out through `write_user_index` and the
//! user bigram's `save_db`, which dump a snapshot (`dump_snapshot`)
//! and issue no sync at all — as the pin's own `save_db` does
//! (`chewing_large_table2_kyotodb.cpp:124-130`,
//! `phrase_large_table3_kyotodb.cpp:139-145`,
//! `ngram_kyotodb.cpp:82-101` at `074a2219`).
//!
//! A power cut *during* the commit itself can still tear the
//! transaction — TreeDB writes through no write-ahead log — the same
//! residual the tkrzw backend documents.
//!
//! # Threading
//!
//! [`KcStore`] is `Send + Sync`: Kyoto Cabinet's database classes carry
//! their own locking (every access method takes the database's rwlock), and the
//! `unsafe impl`s on the native owner record exactly that contract — see
//! the SAFETY comment in `ffi.rs`. This is required for a peer backend that
//! serves as the compiled-in `DefaultStore`: the user-store registry holds
//! `DefaultStore` behind a `static Mutex`, and the runtime compile-asserts
//! its handles `Send + Sync`.
//!
//! # Unsafe waiver
//!
//! This module carries none. Every `unsafe` in the backend — the calls
//! to the bridge's region functions and the `unsafe impl Send`/`Sync` on
//! the native owner — is confined to `ffi`, and the bridge's declarations
//! to `bridge`; each has an `expect(unsafe_code)` scoped to that module,
//! so the safe wrapper here stays under the workspace `deny`.

mod bridge;
mod ffi;

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::Path;

use crate::common::{frame, in_bounds, prefix, unframe};
use crate::{
    AttachMode, Flow, HashAttach, ReadStore, StoreError, WriteStore, WriteTxn, validate_table_name,
};

use ffi::{Db, DbType};

/// A Kyoto Cabinet `TreeDB` store implementing both capability tiers.
pub struct KcStore {
    db: Db,
}

/// A system file's object whose open failed, kept until facade
/// finalization as the pin keeps it.
pub(crate) struct KcUnopenedHandle {
    db: Db,
}

/// The pin's system attach (`chewing_large_table2_kyotodb.cpp:77-90`,
/// `phrase_large_table3_kyotodb.cpp:92-105`, `punct_table_kyotodb.cpp:
/// 52-65`, `ngram_kyotodb.cpp:110-120` at 074a2219), `ATTACH_READONLY`, or
/// with `read_only` false the utilities' `ATTACH_READWRITE | ATTACH_CREATE`
/// (`kyotodb_utils.h:32-45`). Whether the open
/// succeeds or not, the object's teardown is the owner's `reset()`:
/// `synchronize()`, `close()`, then `delete` (`chewing_large_table2_kyotodb.
/// cpp:65-74`, `phrase_large_table3_kyotodb.cpp:76-88`,
/// `punct_table_kyotodb.cpp:37-49`, `ngram_kyotodb.cpp:43-50`).
pub(crate) fn open_system(
    path: &Path,
    hash: bool,
    read_only: bool,
) -> Result<Result<KcStore, KcUnopenedHandle>, StoreError> {
    let kind = if hash { DbType::Hash } else { DbType::Tree };
    let (db, opened) = Db::open_retained(path, kind, read_only, !read_only)?;
    let db = db.with_reset_sync();
    Ok(if opened {
        Ok(KcStore { db })
    } else {
        Err(KcUnopenedHandle { db })
    })
}

impl KcUnopenedHandle {
    pub(crate) fn read(&self, key: &[u8]) {
        // Every failed system read ends at check: no body get or cursor
        // follows (ngram_kyotodb.cpp:131-134 at 074a2219).
        let _ = self.db.check(key);
    }

    /// `Bigram::store`'s `m_db->set(...)` (`ngram_kyotodb.cpp:145-153`),
    /// which the unopened object refuses with `INVALID`; that refusal is
    /// the answer.
    pub(crate) fn write(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db.set(key, value)
    }

    /// `Bigram::get_all_items`' `iterate` (`ngram_kyotodb.cpp:186-196`):
    /// the unopened object refuses it, the pin ignores the answer and
    /// returns true, and no key is collected.
    pub(crate) fn walk(&self) -> Result<(), StoreError> {
        let _ = self.db.raw_keys();
        Ok(())
    }
}

impl KcStore {
    /// A test tool's plain native set without a transaction or synchronization.
    ///
    /// # Errors
    /// Returns the native write error.
    #[cfg(feature = "tools")]
    pub fn tool_set(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db.set(key, value)
    }

    /// Ends the open transaction by committing it. Not generic, so the many
    /// `write` closures share one copy of it.
    #[inline(never)]
    fn commit(&self) -> Result<(), StoreError> {
        self.db.end_transaction(true)?;
        // Push the commit to the operating system (`hard = 0`), so another
        // process — or another handle in this one — reading the database
        // observes it, and a process crash cannot lose it. `compact` is the
        // hard sync; see the module docs for why the device-level sync sits
        // there.
        self.db.sync(false)
    }

    /// Ends the open transaction by rolling it back. The rollback's own
    /// failure must not mask the reason the closure failed, which is what
    /// the caller needs to see.
    #[inline(never)]
    fn roll_back(&self) {
        let _ = self.db.end_transaction(false);
    }

    /// Walks `table`'s rows in ascending key order within `[lo, hi]`.
    ///
    /// One cursor, positioned once with `kccurjumpkey` and advanced by
    /// `kccuraccept`'s own step, stopping at the first key outside the
    /// table's framed prefix — so a scan costs the rows it returns and one
    /// more.
    fn walk(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        // The cursor logic is `TableRows::next`, one copy shared by every
        // call site; each visitor's instantiation is only this loop.
        let mut rows = self.table_rows(table, lo, hi)?;
        while let Some(row) = rows.next()? {
            if visit(row.key, row.value)?.is_break() {
                break;
            }
        }
        Ok(())
    }

    /// The row stream behind [`Self::walk`].
    fn table_rows<'b>(
        &self,
        table: &str,
        lo: Bound<&'b [u8]>,
        hi: Bound<&'b [u8]>,
    ) -> Result<TableRows<'_, 'b>, StoreError> {
        validate_table_name(table)?;
        let table_prefix = prefix(table);
        let start = match lo {
            Bound::Unbounded => table_prefix.clone(),
            Bound::Included(key) | Bound::Excluded(key) => frame(table, key),
        };
        Ok(TableRows {
            cursor: self.db.cursor(),
            positioned: false,
            start,
            table_prefix,
            lo,
            hi,
        })
    }

    /// Every row of the file, sorted by key. Not generic, so the many
    /// visitors share one copy of the iteration and the sort.
    #[inline(never)]
    fn sorted_rows(&self) -> Result<Vec<ffi::Record>, StoreError> {
        let mut rows = self.db.records()?;
        rows.sort_unstable_by(|a, b| a.key().cmp(b.key()));
        Ok(rows)
    }

    /// [`crate::RawReadStore::scan_raw`] on an ordered class: one cursor,
    /// positioned at `lo` and stepped until the first key past `hi`, so the
    /// walk costs the rows it returns and one more — libpinyin's own
    /// `search_suggestion` walk, `cursor->jump(key)` and `step()`
    /// (`phrase_large_table3_kyotodb.cpp:211-244` at 074a2219).
    ///
    /// `TreeDB` and `ProtoTreeDB` order keys with `LEXICALCOMP`, byte-wise
    /// with the shorter key first on a shared prefix, which is the order of
    /// `[u8]`; libpinyin never tunes the comparator. So this visits the
    /// same rows, in the same order, as sorting the whole file would.
    ///
    /// The visitor runs over the cursor's copy of each row after
    /// `kccuraccept` has returned, outside the record lock, so it may call
    /// back into the store.
    fn walk_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor();
        let positioned = match lo {
            // `kccurjumpkey("")` is the first record on an ordered class,
            // but the no-key jump says so directly.
            Bound::Unbounded => cursor.jump_first()?,
            Bound::Included(key) | Bound::Excluded(key) => cursor.jump_to(key)?,
        };
        if !positioned {
            return Ok(());
        }
        while cursor.next()? {
            let key = cursor.key();
            if matches!(lo, Bound::Excluded(bound) if key == bound) {
                continue;
            }
            if !in_bounds(key, Bound::Unbounded, hi) {
                return Ok(());
            }
            if visit(key, cursor.value())?.is_break() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// Every row of `table` in `[lo, hi]`, copied out in key order.
    fn collect_rows(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
    ) -> Result<ffi::Rows, StoreError> {
        let mut out = ffi::Rows::new();
        let mut rows = self.table_rows(table, lo, hi)?;
        while let Some(row) = rows.next()? {
            out.push(row.key, row.value);
        }
        Ok(out)
    }
}

/// A table's rows in ascending key order within `[lo, hi]`: one cursor,
/// positioned once with `kccurjumpkey` and advanced by `kccuraccept`'s own
/// step, ending at the first key outside the table's framed prefix — so a
/// walk costs the rows it returns and one more.
///
/// The stepping is a method of this concrete type rather than a loop in
/// each generic visitor, so the many `scan` call sites share one copy of
/// it and each instantiation is only a thin `while let`.
struct TableRows<'db, 'b> {
    cursor: ffi::Cursor<'db>,
    positioned: bool,
    start: Vec<u8>,
    table_prefix: Vec<u8>,
    lo: Bound<&'b [u8]>,
    hi: Bound<&'b [u8]>,
}

/// One row of a [`TableRows`] walk, borrowed from the cursor's buffer
/// until the next step: the key without the table's framing prefix.
struct TableRow<'r> {
    key: &'r [u8],
    value: &'r [u8],
}

impl TableRows<'_, '_> {
    /// The next row inside the table and bounds, or `None` at the end of
    /// the walk.
    #[inline(never)]
    fn next(&mut self) -> Result<Option<TableRow<'_>>, StoreError> {
        if !self.positioned {
            self.positioned = true;
            if !self.cursor.jump_to(&self.start)? {
                return Ok(None);
            }
        }
        loop {
            if !self.cursor.next()? {
                return Ok(None);
            }
            let Some(key) = unframe(&self.table_prefix, self.cursor.key()) else {
                // Past the last row of this table.
                return Ok(None);
            };
            if matches!(self.lo, Bound::Excluded(bound) if key == bound) {
                continue;
            }
            if !in_bounds(key, Bound::Unbounded, self.hi) {
                return Ok(None);
            }
            break;
        }
        Ok(Some(TableRow {
            key: self
                .cursor
                .key()
                .get(self.table_prefix.len()..)
                .unwrap_or(&[]),
            value: self.cursor.value(),
        }))
    }
}

impl ReadStore for KcStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Tree, true, false)?,
        })
    }

    fn get(&self, table: &str, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        self.db.get(&frame(table, key), value)
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        self.walk(table, lo, hi, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let table_prefix = prefix(table);
        let mut cursor = self.db.cursor();
        if !cursor.jump_to(&table_prefix)? {
            return Ok(true);
        }
        // Reads the first key at or after the prefix, and nothing else.
        Ok(!cursor.peek_key()? || unframe(&table_prefix, cursor.key()).is_none())
    }
}

impl crate::RawReadStore for KcStore {
    fn get_raw_bounded(&self, key: &[u8], value: &mut [u8]) -> Result<Option<usize>, StoreError> {
        self.db.get_bounded(key, value)
    }

    fn get_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, crate::StoreError> {
        self.db.get(key, value)
    }

    fn get_bigram_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, crate::StoreError> {
        self.db.get_bigram(key, value)
    }

    fn contains_raw(&self, key: &[u8]) -> Result<bool, crate::StoreError> {
        self.db.check(key)
    }

    fn scan_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, crate::StoreError>,
    ) -> Result<(), crate::StoreError> {
        if self.db.is_ordered() {
            return self.walk_raw(lo, hi, visit);
        }
        // The contract is ascending key order, but a cursor over the
        // unordered containers (HashDB, and the StashDB the user bigram
        // loads into) walks in bucket order — jump positions at a hash
        // slot, not at the first key at or above a lower bound. Collect
        // every row, sort by key, and apply the bounds here. Each row is
        // dropped once visited, so a visitor that copies rows out never
        // holds two copies of the file.
        for record in self.sorted_rows()? {
            let (key, value) = (record.key(), record.value());
            if !in_bounds(key, lo, Bound::Unbounded) {
                // Below the lower bound, which an `Excluded` bound's
                // equality test alone did not catch.
                continue;
            }
            if !in_bounds(key, Bound::Unbounded, hi) {
                return Ok(());
            }
            if visit(key, value)?.is_break() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// The library's own O(1) record count — over the whole file, which
    /// on this backend is exactly the raw keyspace.
    fn count_raw(&self) -> Result<u64, crate::StoreError> {
        self.db.count()
    }

    /// `Bigram::get_all_items`' `iterate` walk (`ngram_kyotodb.cpp:186-196`),
    /// its one native call: Kyoto Cabinet's own order, which for `HashDB`
    /// is bucket order.
    fn native_keys(&self) -> Result<Vec<Vec<u8>>, crate::StoreError> {
        self.db.raw_keys()
    }

    fn open_hash_read_only(path: &Path) -> Result<Self, crate::StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, true, false)?,
        })
    }

    fn open_user_index(path: &Path) -> Result<Self, crate::StoreError> {
        // 074a2219 chewing_large_table2_kyotodb.cpp:94-105 and
        // phrase_large_table3_kyotodb.cpp:109-121: ProtoTreeDB snapshots.
        // A snapshot that fails part way keeps the records it set before
        // the failure (`BasicDB::load_snapshot` sets record by record,
        // `kcdb.h:2339-2418`), and the pin keeps that container; init
        // ignores the `load_db` answer.
        let db = Db::open_proto_tree()?.with_reset_sync();
        let _ = db.load_snapshot(path);
        Ok(Self {
            db: db.into_read_only(),
        })
    }

    fn open_user_bigram(path: &Path) -> Result<Self, crate::StoreError> {
        // Kyoto Cabinet's user bigram is a snapshot stream, not a hash
        // file (`ngram_kyotodb.cpp:54-64`): open an in-memory stash and
        // load the snapshot into it. The system bigram stays a HashDB file
        // through `open_hash_read_only` above — the two are different
        // containers and must not share one open.
        let db = Db::open_stash()?.with_reset_sync();
        db.load_snapshot(path)?;
        // The seam is read-only even though the stash class opens
        // writer: seal it so a stray `write` cannot mutate memory that
        // no dump would ever persist.
        Ok(Self {
            db: db.into_read_only(),
        })
    }
}

impl WriteStore for KcStore {
    type Txn<'a> = KcTxn<'a>;

    fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Tree, false, true)?,
        })
    }

    fn create_hash(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, false, true)?,
        })
    }

    /// `FlexibleBigram::attach` (`flexible_ngram_kyotodb.h:129-180` at
    /// 074a2219): a `HashDB` opened without `OCREATE`, and with
    /// `ATTACH_CREATE` opened again with it. A failed open without CREATE
    /// deletes the object with no `close` (`:145-150`); a failed retry
    /// keeps it (`:154-155`), and every later call answers `INVALID`. The retry's
    /// signature `set` is ignored (`:157-162`).
    ///
    /// The retry is a second `open` of the same object with the pin's
    /// mode, `OREADER | OWRITER | OCREATE`, so it reads the state the
    /// first open left (`FFATAL` from `set_error`).
    fn attach_hash(
        path: &Path,
        mode: AttachMode,
        signature: (&[u8], &[u8]),
    ) -> Result<HashAttach<Self>, StoreError> {
        let read_only = mode == AttachMode::ReadOnly;
        let flags = if read_only {
            ffi::OREADER
        } else {
            ffi::OREADER | ffi::OWRITER
        };
        let mut db = Db::new_unopened(DbType::Hash, read_only)?;
        if db.open_mode(path, flags)? {
            return Ok(HashAttach::Opened(Self { db }));
        }
        if mode != AttachMode::ReadWriteCreate {
            db.delete_unopened();
            return Ok(HashAttach::Closed);
        }
        if !db.open_mode(path, flags | ffi::OCREATE)? {
            return Ok(HashAttach::Unopened(Self { db }));
        }
        let (key, value) = signature;
        let _ = db.set(key, value);
        Ok(HashAttach::Created(Self { db }, true))
    }

    /// The pin's own in-memory ordered container, `ProtoTreeDB` (what its
    /// user indexes live in between loads and saves,
    /// `chewing_large_table2_kyotodb.cpp:124-130`): no file, no WAL, no
    /// lock file. `ProtoDB` carries its own transaction log, so
    /// [`WriteStore::write`]'s begin/commit/rollback keep their meaning.
    fn create_in_memory() -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_proto_tree()?.with_reset_sync(),
        })
    }

    /// Creates a raw writable tree with user-file permissions. Production
    /// user-index files use `write_user_index`'s snapshot path below.
    fn create_user(path: &Path) -> Result<Self, StoreError> {
        let mut file = std::fs::OpenOptions::new();
        file.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut file, 0o666);
        drop(file.open(path).map_err(StoreError::Io)?);
        Self::create(path)
    }

    fn write_user_index(
        path: &Path,
        records: impl FnOnce(&mut crate::RecordSink<'_>) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        // The pin unlinks then dump_snapshot's the in-memory tree
        // (chewing_large_table2_kyotodb.cpp:124-130,
        // phrase_large_table3_kyotodb.cpp:139-145). Persistence passes
        // the staged .tmp path, so the previous profile remains intact.
        // The records are set into a `ProtoTreeDB`, the pin's container,
        // as they stream in; the file is the snapshot, never a `TreeDB`.
        crate::remove_if_present(path)?;
        let db = Db::open_proto_tree()?;
        records(&mut |key, value| db.set(key, value))?;
        db.dump_snapshot(path)
    }

    fn write_user_bigram(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) -> Result<(), StoreError> {
        // The inverse of `open_user_bigram`, mirroring the pin's
        // `Bigram::save_db` (`ngram_kyotodb.cpp:82-101`) — unlink then
        // dump — with the unlink deferred into an atomic rename: the
        // snapshot is dumped to a sibling temporary first, so a failure
        // anywhere before the rename leaves the previous profile intact
        // instead of destroyed. The temporary itself is unlinked first,
        // as `pinyin.cpp:1017` unlinks it: a stale one's mode would
        // otherwise survive the dump's truncation.
        let tmp = crate::sibling_temp(path);
        crate::remove_if_present(&tmp)?;
        let db = Db::open_stash()?;
        for (key, value) in rows {
            db.set(key, value)?;
        }
        db.dump_snapshot(&tmp)?;
        std::fs::rename(&tmp, path).map_err(StoreError::Io)?;
        Ok(())
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut Self::Txn<'_>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        // A read-only store refuses the whole transaction, not merely the
        // writes inside it: the tier contract is that `write` on a
        // read-only handle is `ReadOnly` even when the closure writes
        // nothing.
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        // Atomicity here is committer-side only: Kyoto Cabinet does not
        // isolate readers from this transaction. `begin_transaction`
        // releases its writer lock once the transaction is under way, and
        // the writes land in the live database before `end_transaction`
        // commits or rolls them back, so a concurrent reader can observe
        // intermediate values — including ones a rollback then removes.
        self.db.begin_transaction()?;
        let mut txn = KcTxn {
            store: self,
            buffer: None,
        };
        match f(&mut txn) {
            Ok(out) => {
                self.commit()?;
                Ok(out)
            }
            Err(error) => {
                self.roll_back();
                Err(error)
            }
        }
    }

    /// `FlexibleBigram::store` and `remove` (`flexible_ngram_kyotodb.h:
    /// 227-252` at 074a2219): the `set` or `remove` alone, with no
    /// transaction and no sync, its answer the pin's. The closure's writes
    /// are buffered, so an `Err` from it applies nothing. `reset()` makes
    /// the one `synchronize()` (`:86-92`), which [`WriteStore::flush`] is.
    fn write_deferred<R>(
        &self,
        f: impl FnOnce(&mut Self::Txn<'_>) -> Result<R, StoreError>,
    ) -> Result<(R, bool), StoreError> {
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        let mut txn = KcTxn {
            store: self,
            buffer: Some(BTreeMap::new()),
        };
        let out = f(&mut txn)?;
        let mut found = true;
        for (key, slot) in txn.buffer.unwrap_or_default() {
            match slot {
                Some(value) => self.db.set(&key, &value)?,
                None => found &= self.db.remove(&key)?,
            }
        }
        Ok((out, found))
    }

    /// `m_db->synchronize()` (`flexible_ngram_kyotodb.h:86-92`), made on a
    /// read-only container too, as the pin's `reset()` makes it.
    fn flush(&self) -> Result<(), StoreError> {
        self.db.sync(false)
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.db.is_read_only() {
            return Err(StoreError::ReadOnly);
        }
        // Kyoto Cabinet reuses freed regions in place through its own free
        // block pool; there is no in-place rewrite that does not go
        // through a copy of the whole file, which is not what the other
        // backends' `compact` does either (Berkeley DB's successful
        // `compact` also does not shrink the file). Making the current state
        // durable on the device is the honest implementation — and it is
        // a file-backed store's one stable-storage point. It is no longer
        // where `pinyin_save` reaches storage: `UserStore::save` still
        // calls this, but on the in-memory session store (#621), which
        // has no file.
        self.db.sync(true)
    }
}

/// A write transaction: the library's own, or the deferred buffer.
///
/// Under [`WriteStore::write`] every method writes straight through to the
/// database inside Kyoto Cabinet's transaction, so read-your-writes is the
/// database's semantics and rollback is the library's. Under
/// [`WriteStore::write_deferred`] the writes wait in `buffer`, keyed as
/// they will be stored (a `None` slot is a pending removal), and reads
/// look there first.
pub struct KcTxn<'store> {
    store: &'store KcStore,
    buffer: Option<BTreeMap<Vec<u8>, Option<Vec<u8>>>>,
}

impl KcTxn<'_> {
    /// Applies one write now, or buffers it under `write_deferred`.
    fn apply(&mut self, key: Vec<u8>, value: Option<&[u8]>) -> Result<(), StoreError> {
        if let Some(buffer) = self.buffer.as_mut() {
            buffer.insert(key, value.map(<[u8]>::to_vec));
            return Ok(());
        }
        match value {
            Some(value) => self.store.db.set(&key, value),
            None => self.store.db.remove(&key).map(drop),
        }
    }
}

impl WriteTxn for KcTxn<'_> {
    // The point operations stay out of line: the user store calls them from
    // hundreds of sites, and inlining each one's validation, framing and
    // buffer insert (or FFI call) into every caller costs far more code
    // than a direct call. Dispatch is static either way.
    #[inline(never)]
    fn get(&self, table: &str, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let framed = frame(table, key);
        if let Some(slot) = self.buffer.as_ref().and_then(|buffer| buffer.get(&framed)) {
            value.clear();
            let Some(bytes) = slot else {
                return Ok(false);
            };
            value.extend_from_slice(bytes);
            return Ok(true);
        }
        self.store.db.get(&framed, value)
    }

    #[inline(never)]
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.apply(frame(table, key), Some(value))
    }

    #[inline(never)]
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        // The file's bare keyspace, no table-name framing — what
        // `get_raw`/`range_raw` read back on this backend.
        self.apply(key.to_vec(), Some(value))
    }

    #[inline(never)]
    fn remove_raw(&mut self, key: &[u8]) -> Result<(), StoreError> {
        // The removal half of `put_raw`, on the same bare keyspace.
        self.apply(key.to_vec(), None)
    }

    #[inline(never)]
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.apply(frame(table, key), None)
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        // A cursor inside the transaction sees the transaction's writes.
        // The rows are collected first: the visitor may itself call back
        // into the store, and holding a cursor across that is what the
        // borrow of `self.store.db` would otherwise allow.
        let rows = self.store.collect_rows(table, lo, hi)?;
        let Some(buffer) = self.buffer.as_ref() else {
            for index in 0..rows.len() {
                let (key, value) = rows.get(index);
                if visit(key, value)?.is_break() {
                    break;
                }
            }
            return Ok(());
        };
        // Under `write_deferred` the buffered writes are laid over the
        // stored rows, in key order.
        let table_prefix = prefix(table);
        let mut merged: BTreeMap<&[u8], &[u8]> =
            (0..rows.len()).map(|index| rows.get(index)).collect();
        for (framed, slot) in buffer {
            let Some(key) = unframe(&table_prefix, framed) else {
                continue;
            };
            if !in_bounds(key, lo, hi) {
                continue;
            }
            match slot {
                Some(value) => merged.insert(key, value),
                None => merged.remove(key),
            };
        }
        for (key, value) in merged {
            if visit(key, value)?.is_break() {
                break;
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        if self.buffer.is_none() {
            return self.store.is_empty(table);
        }
        let mut empty = true;
        self.scan(table, Bound::Unbounded, Bound::Unbounded, |_, _| {
            empty = false;
            Ok(Flow::Break(()))
        })?;
        Ok(empty)
    }
}

// ── the user bigram's in-memory container ─────────────────────────

/// libpinyin's Kyoto Cabinet user bigram as `Bigram::load_db` leaves it:
/// an in-memory `StashDB` filled by `load_snapshot`
/// (`ngram_kyotodb.cpp:54-64`), so its bucket walk is the pin's.
pub struct KcUserBigramDb {
    db: Db,
}

impl crate::UserBigramDb for KcUserBigramDb {
    fn empty() -> Result<Self, StoreError> {
        // `m_db = new StashDB; m_db->open("-", …)`; `reset()` synchronizes
        // before it closes (`ngram_kyotodb.cpp:43-50`).
        Ok(Self {
            db: Db::open_stash()?.with_reset_sync(),
        })
    }

    fn load_db(path: &Path) -> Result<Self, StoreError> {
        // Then `load_snapshot(dbfile)`; a snapshot that does not load
        // leaves the stash as far as it got — empty for an absent file.
        let this = Self::empty()?;
        let _ = this.db.load_snapshot(path);
        Ok(this)
    }

    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.db.get(key, value)
    }

    fn store(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db.set(key, value)
    }

    fn remove(&self, key: &[u8]) -> Result<(), StoreError> {
        self.db.remove(key).map(drop)
    }

    fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError> {
        // `get_all_items`' own call: `iterate` with a key-collecting
        // visitor (`ngram_kyotodb.cpp:163-196`), the stash's bucket order.
        self.db.keys()
    }

    fn is_empty(&self) -> Result<bool, StoreError> {
        Ok(self.db.count()? == 0)
    }

    fn save_db(&self, path: &Path) -> Result<(), StoreError> {
        save_bigram_db(&self.db, path)
    }
}

fn save_bigram_db(db: &Db, path: &Path) -> Result<(), StoreError> {
    // `Bigram::save_db` (`ngram_kyotodb.cpp:82-101`): unlink, then
    // `dump_snapshot`.
    crate::remove_if_present(path)?;
    db.dump_snapshot(path)
}

impl KcStore {
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
    use std::collections::BTreeMap;
    use std::ops::{Bound, ControlFlow};
    use std::path::PathBuf;

    use super::{KcStore, KcUserBigramDb};
    use crate::{
        GramReadOwned, RawReadOwned, RawReadStore, ReadStore, UserBigramDb, WriteStore, WriteTxn,
    };

    /// A file under the temp directory, removed on drop.
    struct Temp(PathBuf);

    impl Temp {
        fn new(tag: &str, ext: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "oxpinyin-store-kc-{tag}-{}.{ext}",
                std::process::id()
            ));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// The pin's `attach_options` modes (`kyotodb_utils.h:32-45`).
    const OREADER: u32 = 1;
    const OWRITER: u32 = 2;
    const OCREATE: u32 = 4;

    #[test]
    fn retained_failed_system_attachments_synchronize_before_close_once() {
        use super::ffi::{NativeCall, take_native_calls};
        use crate::SystemAttachment;

        // Audit each pin class, rather than inferring one from another:
        // chewing_large_table2_kyotodb.cpp:65-73 (main/addon pinyin),
        // phrase_large_table3_kyotodb.cpp:76-86 (main/addon phrase),
        // ngram_kyotodb.cpp:43-48 (HashDB), punct_table_kyotodb.cpp:37-47.
        for (table, hash) in [
            ("system-pinyin", false),
            ("system-phrase", false),
            ("system-bigram", true),
            ("addon-pinyin", false),
            ("addon-phrase", false),
            ("punctuation", false),
        ] {
            let missing = Temp::new(table, "missing");
            let SystemAttachment::Unopened(db) = SystemAttachment::open(&missing.0, hash).unwrap()
            else {
                panic!("missing {table} unexpectedly opened");
            };
            let opened = take_native_calls();
            let &[(handle, NativeCall::Open(_, OREADER))] = opened.as_slice() else {
                panic!("{table}: one read-only open, got {opened:?}");
            };
            db.finish();
            assert_eq!(
                take_native_calls(),
                [
                    (handle, NativeCall::Synchronize(false)),
                    (handle, NativeCall::Close),
                    (handle, NativeCall::Delete),
                ],
                "{table}: native calls on the same retained object"
            );
            db.finish();
            drop(db);
            assert!(take_native_calls().is_empty(), "{table}");
        }
    }

    #[test]
    fn opened_system_attachments_synchronize_before_close() {
        use super::ffi::{NativeCall, take_native_calls};
        use crate::SystemAttachment;

        // The successful attach has the same owner `reset()` as the failed
        // one: synchronize, close, delete (ngram_kyotodb.cpp:43-50,
        // chewing_large_table2_kyotodb.cpp:65-74).
        for hash in [false, true] {
            let file = Temp::new(&format!("opened-system-{hash}"), "db");
            if hash {
                drop(KcStore::create_hash(&file.0).unwrap());
            } else {
                drop(KcStore::create(&file.0).unwrap());
            }
            let SystemAttachment::Open(store) = SystemAttachment::open(&file.0, hash).unwrap()
            else {
                panic!("{hash}: an existing file did not open");
            };
            take_native_calls();
            let handle = store.db.address();
            drop(store);
            assert_eq!(
                take_native_calls(),
                [
                    (handle, NativeCall::Synchronize(false)),
                    (handle, NativeCall::Close),
                    (handle, NativeCall::Delete),
                ],
                "hash: {hash}"
            );
        }
    }

    #[test]
    fn an_attached_bigram_synchronizes_before_close() {
        use super::ffi::{NativeCall, take_native_calls};

        // `Bigram::reset()` synchronizes, closes and deletes whatever it
        // attached (ngram_kyotodb.cpp:43-50, 110-120).
        let file = Temp::new("attached-bigram", "db");
        for read_only in [false, true] {
            let crate::SystemAttachment::Open(store) =
                crate::SystemAttachment::attach_bigram(&file.0, read_only).unwrap()
            else {
                panic!("read only: {read_only}: the bigram did not attach");
            };
            take_native_calls();
            let handle = store.db.address();
            drop(store);
            assert_eq!(
                take_native_calls(),
                [
                    (handle, NativeCall::Synchronize(false)),
                    (handle, NativeCall::Close),
                    (handle, NativeCall::Delete),
                ],
                "read only: {read_only}"
            );
        }
    }

    #[test]
    fn a_failed_bigram_attach_keeps_its_hashdb_until_reset() {
        use super::ffi::{NativeCall, OCREATE, OREADER, OWRITER, take_native_calls};
        use crate::SystemAttachment;
        use std::os::unix::ffi::OsStrExt as _;

        // `Bigram::attach` makes one `open` on a new `HashDB`, with no
        // CREATE retry and no signature check, and keeps the object when
        // the open fails (ngram_kyotodb.cpp:110-120). `load`, `store` and
        // `get_all_items` then reach it: `check`, `set`, and an `iterate`
        // with no `count` before it (:124-153, :186-196). `reset()`
        // synchronizes, closes and deletes it (:43-50).
        let dir = Temp::new("failed-bigram", "missing");
        let path = dir.0.join("bigram.db");
        for read_only in [true, false] {
            take_native_calls();
            let SystemAttachment::Unopened(db) =
                SystemAttachment::attach_bigram(&path, read_only).unwrap()
            else {
                panic!("read only: {read_only}: a missing directory opened");
            };
            let opened = take_native_calls();
            let handle = opened.first().expect("native open").0;
            let mode = if read_only {
                OREADER
            } else {
                OREADER | OWRITER | OCREATE
            };
            assert_eq!(
                opened,
                [(
                    handle,
                    NativeCall::Open(path.as_os_str().as_bytes().to_vec(), mode)
                )],
                "read only: {read_only}: the object outlives the attach"
            );
            db.read(b"tkn\0");
            // The utilities write only through the read-write attach. The
            // refused `set` is `store`'s false; the refused `iterate` is
            // ignored and `get_all_items` answers true.
            if !read_only {
                assert!(db.write(b"tkn\0", b"gram").is_err());
            }
            db.walk().expect("get_all_items answers true");
            db.finish();
            let mut expected = vec![(handle, NativeCall::Check)];
            if !read_only {
                expected.push((handle, NativeCall::Set));
            }
            expected.extend([
                (handle, NativeCall::Iterate),
                (handle, NativeCall::Synchronize(false)),
                (handle, NativeCall::Close),
                (handle, NativeCall::Delete),
            ]);
            assert_eq!(take_native_calls(), expected, "read only: {read_only}");
            drop(db);
            assert!(take_native_calls().is_empty(), "read only: {read_only}");
            assert!(!path.exists(), "no file was created");
        }
    }

    #[test]
    fn in_memory_owners_open_dash_and_synchronize_before_close() {
        use super::ffi::{NativeCall, take_native_calls};

        // The pin opens both in-memory containers as "-" with
        // OREADER|OWRITER|OCREATE (chewing_large_table2_kyotodb.cpp:100-102,
        // ngram_kyotodb.cpp:58-60), and its `reset()` synchronizes before it
        // closes (chewing_large_table2_kyotodb.cpp:65-74,
        // ngram_kyotodb.cpp:43-50).
        let snapshot = Temp::new("in-memory-owner", "bin");
        KcStore::write_user_index(&snapshot.0, |sink| sink(b"k", b"v")).unwrap();
        take_native_calls();
        // Each owner is made, dropped, and its object's address returned.
        type Owner = fn(&std::path::Path) -> usize;
        let owners: [(&str, Owner); 4] = [
            ("session store", |_| {
                KcStore::create_in_memory().unwrap().db.address()
            }),
            ("user index", |path| {
                KcStore::open_user_index(path).unwrap().db.address()
            }),
            ("user bigram", |_| {
                KcUserBigramDb::empty().unwrap().db.address()
            }),
            ("user bigram load", |path| {
                KcUserBigramDb::load_db(path).unwrap().db.address()
            }),
        ];
        for (owner, make) in owners {
            let handle = make(&snapshot.0);
            assert_eq!(
                take_native_calls(),
                [
                    (
                        handle,
                        NativeCall::Open(b"-".to_vec(), OREADER | OWRITER | OCREATE)
                    ),
                    (handle, NativeCall::Synchronize(false)),
                    (handle, NativeCall::Close),
                    (handle, NativeCall::Delete),
                ],
                "{owner}"
            );
        }
    }

    #[test]
    fn a_malformed_user_index_snapshot_keeps_its_valid_prefix() {
        // `BasicDB::load_snapshot` sets each record as it reads it and fails
        // at the bad tag after them (`kcdb.h:2339-2418`); the pin keeps that
        // ProtoTreeDB (`chewing_large_table2_kyotodb.cpp:94-106`).
        let file = Temp::new("partial-snapshot", "bin");
        KcStore::write_user_index(&file.0, |sink| {
            sink(b"first", b"1")?;
            sink(b"second", b"")
        })
        .unwrap();
        let mut bytes = std::fs::read(&file.0).unwrap();
        assert_eq!(bytes.pop(), Some(0xff), "the snapshot's end mark");
        bytes.push(0x01);
        std::fs::write(&file.0, bytes).unwrap();

        let store = KcStore::open_user_index(&file.0).expect("the partial container stays");
        assert_eq!(store.read_raw(b"first").unwrap(), Some(b"1".to_vec()));
        assert_eq!(store.read_raw(b"second").unwrap(), Some(Vec::new()));
    }

    #[test]
    fn a_create_retry_reopens_the_same_hashdb_and_a_failed_open_deletes_without_close() {
        use super::ffi::{NativeCall as Call, take_native_calls};
        use crate::{AttachMode, HashAttach};

        // `flexible_ngram_kyotodb.h:143-155`: one `new HashDB`, then
        // `open(mode)` and, with ATTACH_CREATE, `open(mode | OCREATE)` on
        // the same object; without CREATE, `delete` and no `close`.
        let dir = Temp::new("retry-dir", "missing");
        let path = dir.0.join("model.db");
        let bytes = path.as_os_str().as_encoded_bytes().to_vec();
        take_native_calls();
        let HashAttach::Unopened(store) =
            KcStore::attach_hash(&path, AttachMode::ReadWriteCreate, (b"k", b"v")).unwrap()
        else {
            panic!("a missing directory opened");
        };
        let calls = take_native_calls();
        let handle = calls.first().expect("native open").0;
        assert_eq!(
            calls,
            [
                (handle, Call::Open(bytes.clone(), OREADER | OWRITER)),
                (
                    handle,
                    Call::Open(bytes.clone(), OREADER | OWRITER | OCREATE)
                ),
            ]
        );
        drop(store);

        take_native_calls();
        let HashAttach::Closed =
            KcStore::attach_hash(&path, AttachMode::ReadWrite, (b"k", b"v")).unwrap()
        else {
            panic!("a failed open without CREATE keeps no object");
        };
        let calls = take_native_calls();
        let handle = calls.first().expect("native open").0;
        assert_eq!(
            calls,
            [
                (handle, Call::Open(bytes, OREADER | OWRITER)),
                (handle, Call::Delete)
            ]
        );
    }

    #[test]
    fn flush_synchronizes_a_read_only_container_natively() {
        use super::ffi::{NativeCall, take_native_calls};

        // `FlexibleBigram::reset()` synchronizes whatever the attach mode
        // (`flexible_ngram_kyotodb.h:86-92`); an OREADER HashDB answers true
        // (`kchashdb.h:969-983`).
        let file = Temp::new("readonly-flush", "kch");
        drop(KcStore::create_hash(&file.0).unwrap());
        let store = KcStore::open_hash_read_only(&file.0).unwrap();
        take_native_calls();
        store.flush().expect("an OREADER HashDB synchronizes");
        let calls = take_native_calls();
        assert_eq!(calls.len(), 1, "{calls:?}");
        assert_eq!(calls[0].1, NativeCall::Synchronize(false));
    }

    #[test]
    fn an_empty_value_is_stored_on_every_container() {
        // An empty slice crosses the bridge as address 1, Kyoto Cabinet's
        // `Visitor::REMOVE` (kcdb.h:46); stored as such, `set` would delete
        // the record instead of emptying it (kcdb.h:1424-1445). `Vec::new()`
        // is the slice with no allocation.
        let empty: Vec<u8> = Vec::new();
        let check = |store: &KcStore, container: &str| {
            store
                .write(|txn| {
                    txn.put_raw(b"present", b"value")?;
                    txn.put_raw(b"present", &empty)?;
                    txn.put_raw(b"fresh", &empty)
                })
                .unwrap();
            for key in [&b"present"[..], b"fresh"] {
                assert_eq!(
                    store.read_raw(key).unwrap(),
                    Some(Vec::new()),
                    "{container}"
                );
                assert!(store.contains_raw(key).unwrap(), "{container}");
            }
        };
        let tree = Temp::new("empty-tree", "kct");
        check(&KcStore::create(&tree.0).unwrap(), "TreeDB");
        let hash = Temp::new("empty-hash", "kch");
        check(&KcStore::create_hash(&hash.0).unwrap(), "HashDB");
        check(&KcStore::create_in_memory().unwrap(), "ProtoTreeDB");
        let stash = KcUserBigramDb::empty().unwrap();
        stash.store(b"present", b"value").unwrap();
        stash.store(b"present", &empty).unwrap();
        assert_eq!(
            stash.read_gram(b"present").unwrap(),
            Some(Vec::new()),
            "StashDB"
        );
    }

    #[test]
    fn keys_walk_the_stash_in_its_cursor_order() {
        // `keys()` is one `iterate`, the pin's `get_all_items` call; the
        // stash's cursor visits its buckets in the same order.
        let db = KcUserBigramDb::empty().unwrap();
        for token in 0_u32..3000 {
            db.store(&token.wrapping_mul(2_654_435_761).to_le_bytes(), &[1, 2, 3])
                .unwrap();
        }
        let mut cursor = db.db.cursor();
        let mut walked = Vec::new();
        if cursor.jump_first().unwrap() {
            while cursor.next_key().unwrap() {
                walked.push(<[u8; 4]>::try_from(cursor.key()).ok());
            }
        }
        assert_eq!(walked.len(), 3000);
        assert_eq!(db.keys().unwrap(), walked);
    }

    #[test]
    fn point_reads_tell_empty_values_from_misses() {
        let db = KcUserBigramDb::empty().unwrap();
        db.store(b"empty", b"").unwrap();
        db.store(b"full", &[7; 300]).unwrap();
        assert_eq!(db.read_gram(b"empty").unwrap(), Some(Vec::new()));
        assert_eq!(db.read_gram(b"full").unwrap(), Some(vec![7; 300]));
        assert_eq!(db.read_gram(b"absent").unwrap(), None);
        let store = KcStore::create_in_memory().unwrap();
        store
            .write(|txn| {
                txn.put_raw(b"empty", b"")?;
                txn.put_raw(b"full", &[9; 5])
            })
            .unwrap();
        assert_eq!(store.read_raw(b"empty").unwrap(), Some(Vec::new()));
        assert_eq!(store.read_raw(b"full").unwrap(), Some(vec![9; 5]));
        assert_eq!(store.read_raw(b"absent").unwrap(), None);
        assert!(store.contains_raw(b"empty").unwrap());
        assert!(store.contains_raw(b"full").unwrap());
        assert!(!store.contains_raw(b"absent").unwrap());
    }

    /// Keys that share prefixes, cross 0x00/0xFF and include the empty
    /// key's neighbours, half with empty values.
    fn rows() -> BTreeMap<Vec<u8>, Vec<u8>> {
        let mut rows = BTreeMap::new();
        for a in [0x00_u8, 0x01, 0x7F, 0x80, 0xFE, 0xFF] {
            rows.insert(vec![a], vec![]);
            for b in [0x00_u8, 0x41, 0xFF] {
                rows.insert(vec![a, b], vec![a, b, 1]);
                rows.insert(vec![a, b, b], if b == 0x41 { vec![] } else { vec![b; 40] });
            }
        }
        rows
    }

    /// An owned `[lo, hi]` pair.
    type Range = (Bound<Vec<u8>>, Bound<Vec<u8>>);

    fn bounds() -> Vec<Range> {
        let points: [&[u8]; 6] = [
            &[],
            &[0x00],
            &[0x01, 0x41],
            &[0x7F, 0xFF, 0xFF],
            &[0x80],
            &[0xFF, 0xFF],
        ];
        let mut out = vec![(Bound::Unbounded, Bound::Unbounded)];
        for lo in points {
            for hi in points {
                for (l, h) in [
                    (Bound::Included(lo.to_vec()), Bound::Excluded(hi.to_vec())),
                    (Bound::Excluded(lo.to_vec()), Bound::Included(hi.to_vec())),
                    (Bound::Included(lo.to_vec()), Bound::Unbounded),
                    (Bound::Unbounded, Bound::Excluded(hi.to_vec())),
                ] {
                    out.push((l, h));
                }
            }
        }
        out
    }

    fn as_ref(bound: &Bound<Vec<u8>>) -> Bound<&[u8]> {
        match bound {
            Bound::Included(key) => Bound::Included(key),
            Bound::Excluded(key) => Bound::Excluded(key),
            Bound::Unbounded => Bound::Unbounded,
        }
    }

    /// `scan_raw` over `store` agrees with the model for every bound pair,
    /// and a visitor `Break` stops after exactly that row.
    fn assert_scans_match(store: &KcStore, model: &BTreeMap<Vec<u8>, Vec<u8>>) {
        for (lo, hi) in bounds() {
            let lower = as_ref(&lo);
            let upper = as_ref(&hi);
            let expected: Vec<(Vec<u8>, Vec<u8>)> = model
                .iter()
                .filter(|(key, _)| crate::common::in_bounds(key, lower, upper))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect();
            let mut got = Vec::new();
            store
                .scan_raw(lower, upper, |key, value| {
                    got.push((key.to_vec(), value.to_vec()));
                    Ok(ControlFlow::Continue(()))
                })
                .unwrap();
            assert_eq!(got, expected, "bounds {lo:?} .. {hi:?}");
            let mut first = Vec::new();
            store
                .scan_raw(lower, upper, |key, _| {
                    first.push(key.to_vec());
                    Ok(ControlFlow::Break(()))
                })
                .unwrap();
            assert_eq!(
                first,
                expected
                    .iter()
                    .take(1)
                    .map(|row| row.0.clone())
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn a_failed_suggestion_makes_only_the_check_call() {
        use super::ffi::{NativeCall, take_native_calls};
        use crate::SystemAttachment;

        // `ChewingLargeTable2::search_suggestion` on a tree whose open
        // failed: its first call is `m_db->check(akbuf, aksiz)` and the
        // `-1` answer returns before any cursor is made
        // (chewing_large_table2_kyotodb.cpp:539-544 at 074a2219), so no
        // `jump`, `step` or `get` follows. After the owner's `reset()` the
        // object is gone and the call reaches nothing.
        let dir = Temp::new("failed-suggestion", "missing");
        let path = dir.0.join("pinyin_index.bin");
        take_native_calls();
        let SystemAttachment::Unopened(db) = SystemAttachment::open(&path, false).unwrap() else {
            panic!("a path in a missing directory opened");
        };
        let handle = take_native_calls().first().expect("native open").0;
        db.suggest(&[1, 0, 2, 0]);
        assert_eq!(take_native_calls(), [(handle, NativeCall::Check)]);
        db.finish();
        take_native_calls();
        db.suggest(&[1, 0, 2, 0]);
        db.read(&[1, 0, 2, 0]);
        assert!(take_native_calls().is_empty(), "a finished object is inert");
    }

    #[test]
    fn a_read_only_container_refuses_set_and_remove_before_any_native_call() {
        use super::ffi::take_native_calls;
        use crate::StoreError;

        // A `Bigram`/`FlexibleBigram` attached `ATTACH_READONLY` is a KC
        // `OREADER` database, so its `set` and `remove` answer false
        // (ngram_kyotodb.cpp:152, :160; flexible_ngram_kyotodb.h:235, :251
        // at 074a2219). The wrapper's answer is `ReadOnly`, and the
        // refusal makes no native call.
        let file = Temp::new("readonly-set", "kch");
        let store = KcStore::create_hash(&file.0).unwrap();
        store.write(|txn| txn.put_raw(b"k", b"v")).unwrap();
        drop(store);
        let store = KcStore::open_hash_read_only(&file.0).unwrap();
        take_native_calls();
        assert!(matches!(
            store.db.set(b"k", b"w"),
            Err(StoreError::ReadOnly)
        ));
        assert!(matches!(store.db.remove(b"k"), Err(StoreError::ReadOnly)));
        assert!(take_native_calls().is_empty());
        assert_eq!(store.read_raw(b"k").unwrap(), Some(b"v".to_vec()));
    }

    #[test]
    fn scan_raw_is_ascending_and_bounded_on_every_container() {
        let model = rows();
        let fill = |store: &KcStore| {
            store
                .write(|txn| {
                    for (key, value) in &model {
                        txn.put_raw(key, value)?;
                    }
                    Ok(())
                })
                .unwrap();
        };
        let tree = Temp::new("scan-tree", "kct");
        let store = KcStore::create(&tree.0).unwrap();
        fill(&store);
        drop(store);
        assert_scans_match(&KcStore::open_read_only(&tree.0).unwrap(), &model);

        let hash = Temp::new("scan-hash", "kch");
        let store = KcStore::create_hash(&hash.0).unwrap();
        fill(&store);
        drop(store);
        assert_scans_match(&KcStore::open_hash_read_only(&hash.0).unwrap(), &model);

        let store = KcStore::create_in_memory().unwrap();
        fill(&store);
        assert_scans_match(&store, &model);

        let rows: Vec<(Vec<u8>, Vec<u8>)> = model.clone().into_iter().collect();
        let index = Temp::new("scan-index", "bin");
        KcStore::write_user_index(&index.0, |sink| {
            rows.iter().try_for_each(|(key, value)| sink(key, value))
        })
        .unwrap();
        assert_scans_match(&KcStore::open_user_index(&index.0).unwrap(), &model);

        let bigram = Temp::new("scan-bigram", "db");
        KcStore::write_user_bigram(&bigram.0, &rows).unwrap();
        assert_scans_match(&KcStore::open_user_bigram(&bigram.0).unwrap(), &model);
    }
}
