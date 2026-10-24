//! Kyoto Cabinet backend for the store's peer set.
//!
//! Enabled by the `kyotocabinet` cargo feature (`--no-default-features
//! --features kyotocabinet`); the workspace's default selected backend is
//! Berkeley DB.
//! [`KcStore`] is a `TreeDB` implementation of the store's two capability
//! tiers, addressed by `(table, key)` like every other backend.
//!
//! # The class is chosen by the caller, never by the file name
//!
//! The backend calls Kyoto Cabinet through its own C++ shim
//! (`shim.cc`, behind `ffi`), which constructs the class the caller
//! names, as libpinyin does, and opens the plain path. oxpinyin names its
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
//! (`begin_transaction`/`end_transaction`, through the shim's
//! `oxkc_db_begin_transaction`/`oxkc_db_end_transaction`; the C API's
//! `kcdbbegintran`/`kcdbendtran`), so [`WriteStore::write`] is the
//! library's own transaction rather than a buffered imitation: the
//! closure's writes go straight to the database inside the transaction,
//! reads see them, and an `Err` rolls the whole thing back.
//!
//! Commits end with `oxkc_db_sync(hard = 0)` (`synchronize(false)`, the C
//! API's `kcdbsync`): the commit is pushed to the
//! operating system, so another process — or another handle in this one
//! — observes it and it survives a process crash, but it is not yet on
//! the device. [`WriteStore::compact`] is the hard `oxkc_db_sync(hard = 1)`,
//! the one point at which a file-backed store reaches stable storage.
//! The tkrzw backend documents the same split and the measurement
//! behind it (`docs/findings/perf-train-commit-fsync-2026-09-09.md`).
//!
//! `compact` is no longer where `pinyin_save` reaches storage, though.
//! `UserStore::save` still calls it, but since the libpinyin session
//! moved into memory (#621) the store it compacts is the `ProtoTreeDB`
//! of [`WriteStore::create_in_memory`], which has no file. The profile
//! `pinyin_save` writes goes out through `write_user_index` and the
//! user bigram's `save_db`, which dump a snapshot (`oxkc_db_dump_snapshot`)
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
//! `unsafe impl`s on the FFI handle record exactly that contract — see the
//! SAFETY comment in `ffi.rs`. This is required for a peer backend that
//! serves as the compiled-in `DefaultStore`: the user-store registry holds
//! `DefaultStore` behind a `static Mutex`, and the runtime compile-asserts
//! its handles `Send + Sync`.
//!
//! # Unsafe waiver
//!
//! This module carries none. Every `unsafe` in the backend — the
//! shim calls, the `unsafe impl Send`/`Sync` on the handle, the
//! generated declarations — is confined to `ffi`, whose
//! `expect(unsafe_code)` is scoped to that module, so the safe wrapper
//! here stays under the workspace `deny`.

mod ffi;

use std::ops::Bound;
use std::path::Path;

use crate::common::{frame, in_bounds, prefix, unframe};
use crate::{Flow, ReadStore, StoreError, WriteStore, WriteTxn, validate_table_name};

use ffi::{Db, DbType};

/// A Kyoto Cabinet `TreeDB` store implementing both capability tiers.
pub struct KcStore {
    db: Db,
}

/// Hidden bench entry points.
///
/// Reserved for measurement work in this crate's `benches/` directory;
/// the `bench-internal` feature is off by default and no shipping
/// profile enables it. The tuning knob these open up is what
/// shenghaoc/oxpinyin#402's time-side measurement bench uses to
/// characterise a Kyoto Cabinet default for an upstream report; the
/// shipping open path itself matches libpinyin's untuned open
/// byte-for-byte and stays that way (see
/// `docs/findings/compatibility-policy.md`).
///
/// Not stable, not documented for external use, not part of any
/// contract this backend keeps. Names live only under this feature so
/// nothing else in the crate — tests, doc builds, the shipping default
/// selection — can even see them.
#[cfg(feature = "bench-internal")]
impl KcStore {
    /// Opens `path` read-only, appending `extra_tuning` after `#type=kct`.
    ///
    /// `extra_tuning` must start with `#` (`#bnum=4096`, `#bnum=4096#pccap=1m`,
    /// …) or be empty. Empty reproduces [`KcStore::open_read_only`]
    /// byte-for-byte.
    #[doc(hidden)]
    pub fn open_read_only_tuned(path: &Path, extra_tuning: &str) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_with_tuning(path, DbType::Tree, true, false, extra_tuning)?,
        })
    }

    /// Same as [`Self::open_read_only_tuned`] but for `HashDB`.
    #[doc(hidden)]
    pub fn open_hash_read_only_tuned(path: &Path, extra_tuning: &str) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_with_tuning(path, DbType::Hash, true, false, extra_tuning)?,
        })
    }

    /// Walks the raw keyspace (no table framing), calling `visit` with each
    /// key. Stops on the first `Err` from `visit`. Used only to sample a
    /// stable set of keys for the `kyotocabinet_bnum` bench; production
    /// walks go through [`crate::RawReadStore::range_raw`].
    #[doc(hidden)]
    pub fn walk_raw_keys(
        &self,
        mut visit: impl FnMut(&[u8]) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor()?;
        if !cursor.jump_to(&[])? {
            return Ok(());
        }
        while cursor.next_key()? {
            visit(cursor.key())?;
        }
        Ok(())
    }
}

impl KcStore {
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
            cursor: self.db.cursor()?,
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
        let mut cursor = self.db.cursor()?;
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
        let mut cursor = self.db.cursor()?;
        if !cursor.jump_to(&table_prefix)? {
            return Ok(true);
        }
        // Reads the first key at or after the prefix, and nothing else.
        Ok(!cursor.peek_key()? || unframe(&table_prefix, cursor.key()).is_none())
    }
}

impl crate::RawReadStore for KcStore {
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

    fn open_hash_read_only(path: &Path) -> Result<Self, crate::StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, true, false)?,
        })
    }

    fn open_user_index(path: &Path) -> Result<Self, crate::StoreError> {
        // 074a2219 chewing_large_table2_kyotodb.cpp:94-105 and
        // phrase_large_table3_kyotodb.cpp:109-121: ProtoTreeDB snapshots.
        let db = Db::open_proto_tree()?;
        db.load_snapshot(path)?;
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
        let db = Db::open_stash()?;
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

    /// The pin's own in-memory ordered container, `ProtoTreeDB` (what its
    /// user indexes live in between loads and saves,
    /// `chewing_large_table2_kyotodb.cpp:124-130`): no file, no WAL, no
    /// lock file. `ProtoDB` carries its own transaction log, so
    /// [`WriteStore::write`]'s begin/commit/rollback keep their meaning.
    fn create_in_memory() -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_proto_tree()?,
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

    fn write_user_index(path: &Path, rows: &[(Vec<u8>, Vec<u8>)]) -> Result<(), StoreError> {
        // The pin unlinks then dump_snapshot's the in-memory tree
        // (chewing_large_table2_kyotodb.cpp:124-130,
        // phrase_large_table3_kyotodb.cpp:139-145). Persistence passes
        // the staged .tmp path, so the previous profile remains intact.
        crate::remove_if_present(path)?;
        let db = Db::open_proto_tree()?;
        for (key, value) in rows {
            db.set(key, value)?;
        }
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
        let mut txn = KcTxn { store: self };
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

/// A write transaction — the library's own, not a buffer.
///
/// Every method writes straight through to the database inside Kyoto
/// Cabinet's transaction, so read-your-writes is the database's semantics
/// rather than something this type has to emulate, and rollback is the
/// library's.
pub struct KcTxn<'store> {
    store: &'store KcStore,
}

impl WriteTxn for KcTxn<'_> {
    // The point operations stay out of line: the user store calls them from
    // hundreds of sites, and inlining each one's validation, framing and
    // buffer insert (or FFI call) into every caller costs far more code
    // than a direct call. Dispatch is static either way.
    #[inline(never)]
    fn get(&self, table: &str, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.store.get(table, key, value)
    }

    #[inline(never)]
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.store.db.set(&frame(table, key), value)
    }

    #[inline(never)]
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        // The file's bare keyspace, no table-name framing — what
        // `get_raw`/`range_raw` read back on this backend.
        self.store.db.set(key, value)
    }

    #[inline(never)]
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.store.db.remove(&frame(table, key))
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        // A cursor inside the transaction sees the transaction's writes,
        // so the walk needs no merge pass. It does need the rows
        // collected first: the visitor may itself call back into the
        // store, and holding a cursor across that is what the borrow of
        // `self.store.db` would otherwise allow.
        let rows = self.store.collect_rows(table, lo, hi)?;
        for index in 0..rows.len() {
            let (key, value) = rows.get(index);
            if visit(key, value)?.is_break() {
                break;
            }
        }
        Ok(())
    }

    #[inline(never)]
    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        self.store.is_empty(table)
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
        // `m_db = new StashDB; m_db->open("-", …)`.
        Ok(Self {
            db: Db::open_stash()?,
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
        self.db.remove(key)
    }

    fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError> {
        // `get_all_items`' own call: `iterate` with a key-collecting
        // visitor (`ngram_kyotodb.cpp:163-196`), the stash's bucket order.
        self.db.keys()
    }

    fn save_db(&self, path: &Path) -> Result<(), StoreError> {
        // `Bigram::save_db` (`ngram_kyotodb.cpp:82-101`): unlink, then
        // `dump_snapshot`.
        crate::remove_if_present(path)?;
        self.db.dump_snapshot(path)
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

    #[test]
    fn keys_walk_the_stash_in_its_cursor_order() {
        // `keys()` is one `iterate`, the pin's `get_all_items` call; the
        // stash's cursor visits its buckets in the same order.
        let db = KcUserBigramDb::empty().unwrap();
        for token in 0_u32..3000 {
            db.store(&token.wrapping_mul(2_654_435_761).to_le_bytes(), &[1, 2, 3])
                .unwrap();
        }
        let mut cursor = db.db.cursor().unwrap();
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
        KcStore::write_user_index(&index.0, &rows).unwrap();
        assert_scans_match(&KcStore::open_user_index(&index.0).unwrap(), &model);

        let bigram = Temp::new("scan-bigram", "db");
        KcStore::write_user_bigram(&bigram.0, &rows).unwrap();
        assert_scans_match(&KcStore::open_user_bigram(&bigram.0).unwrap(), &model);
    }
}
