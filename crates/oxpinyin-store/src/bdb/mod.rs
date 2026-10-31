//! Berkeley DB 5.3 backend — libpinyin's original DBM, and the third
//! drop-in set (drop-in task 10).
//!
//! Enabled by the `bdb` cargo feature. One type, [`BdbStore`], serves
//! both roles the architecture asks of a libpinyin-DBM backend:
//!
//! * the framed **container** (`DB_BTREE`, one file, `table || 0x00 ||
//!   key` like the Kyoto Cabinet and tkrzw backends) behind
//!   [`crate::ReadStore`] / [`crate::WriteStore`] — the session scratch
//!   store, the datagen output, the benches;
//! * the **raw** keyspace of libpinyin's own files behind
//!   [`crate::RawReadStore`]: `create_hash`/`open_hash_read_only` are
//!   the `DB_HASH` form of `bigram.db`/`user_bigram.db`, and the
//!   `put_raw`/`get_raw`/`range_raw` seam is what `stage_dbm`'s tree
//!   containers (`pinyin_index.bin`, `phrase_index.bin`, `punct.bin`)
//!   and the user-dir bigram writer are written through. On this
//!   backend the system and user bigrams are the same container — a
//!   `DB_HASH` file — so the trait's default `open_user_bigram`
//!   (delegate to `open_hash_read_only`) is correct and neither
//!   override that Kyoto Cabinet needs applies.
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
//! [`BdbStore`] is `Send` and `Sync` through a Rust `Mutex` around the
//! standalone handle, opened with the pin's flags. Every database and
//! cursor call has exclusive access. Cursor reads land in caller-owned
//! (`DB_DBT_USERMEM`) buffers, so the lock can be released before a
//! visitor reads a row or calls the store again; a point read is copied
//! out of the handle's own return buffer while the lock is held. The user
//! store also retains its own `Mutex` for whole write batches.
//!
//! # Unsafe waiver
//!
//! This module carries none. Every `unsafe` in the backend — the `DB`
//! and `DBC` member calls, the `unsafe impl Send` on the handle,
//! the generated declarations — is confined to `ffi`, whose
//! `expect(unsafe_code)` is scoped to that module, so the safe wrapper
//! here stays under the workspace `deny`.

mod ffi;

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::Path;

use crate::common::{Row, fill_buffered, frame, in_bounds, prefix, unframe};
use crate::{Flow, RawReadStore, ReadStore, StoreError, WriteStore, WriteTxn, validate_table_name};

use ffi::{Seek, SharedDb as Db};

/// One borrowed `(key, value)` row.
type BorrowedRow<'a> = (&'a [u8], &'a [u8]);

/// A Berkeley DB store: a `DB_BTREE` container when opened by
/// [`ReadStore::open_read_only`] / [`WriteStore::create`], a `DB_HASH`
/// raw keyspace when opened by the hash constructors.
pub struct BdbStore {
    db: Db,
}

impl BdbStore {
    /// Applies a finished transaction's buffer and syncs. Not generic, so
    /// the many `write` closures share one copy of it.
    #[inline(never)]
    fn commit(&self, buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>) -> Result<(), StoreError> {
        for (key, value) in buffer {
            match value {
                Some(value) => self.db.put(&key, &value)?,
                None => {
                    self.db.del(&key)?;
                }
            }
        }
        // Push the batch out, so a reader — including the user's own
        // libpinyin — sees a consistent file once `write` returns.
        // Without it the pages stay in the private mpool of a handle
        // opened with no environment, where no other process can reach
        // them. `DB->sync` is the only flush available here and it goes
        // all the way to the device, so this commit exceeds the seam's
        // floor; see `WriteStore::write`'s durability note.
        self.db.sync()
    }

    /// Walks `table`'s rows in ascending key order, within `[lo, hi]`,
    /// handing each to `visit`.
    ///
    /// One cursor, positioned once with `DB_SET_RANGE` and advanced with
    /// `DB_NEXT`; the walk stops at the first key outside the table's
    /// framed prefix, so a scan costs the rows it returns and one more.
    ///
    /// **Assumes a `DB_BTREE` handle.** `DB_SET_RANGE` is Btree-only —
    /// libdb answers `EINVAL` on a `DB_HASH` — so unlike
    /// [`Self::range_raw`] this does not branch on [`ffi::Db::is_hash`].
    /// That holds because the framed tier is reached only through a
    /// tree-opened store: every caller of the hash constructors
    /// (`create_hash` / `open_hash_read_only` / `open_user_bigram`) uses
    /// the raw tier alone. Nothing in the type system enforces it — the
    /// hash constructors return the same `Self` the tree ones do. See the
    /// Stage 2 section of `docs/findings/store-key-ordering.md`.
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
        while let Some((key, value)) = rows.next()? {
            if visit(&key, &value)?.is_break() {
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
            started: false,
            start,
            table_prefix,
            lo,
            hi,
        })
    }

    /// Whether `table` has any row at all — one cursor positioning, never
    /// a scan.
    ///
    /// Assumes a `DB_BTREE` handle for the same reason [`Self::walk`]
    /// does, and on the same grounds.
    fn first_key_of(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let table_prefix = prefix(table);
        let mut cursor = self.db.cursor()?;
        Ok(
            match cursor.get(Seek::AtOrAfter(table_prefix.as_slice()))? {
                None => false,
                Some(row) => unframe(&table_prefix, row.key).is_some(),
            },
        )
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

/// A table's rows in ascending key order, within `[lo, hi]`: one cursor,
/// positioned once with `DB_SET_RANGE` and advanced with `DB_NEXT`,
/// ending at the first key outside the table's framed prefix or bounds,
/// so a walk costs the rows it returns and one more.
///
/// The stepping is a method of this concrete type rather than a loop in
/// each generic visitor, so the many `scan` call sites share one copy of
/// it and each instantiation is only a thin `while let`.
struct TableRows<'db, 'b> {
    cursor: ffi::Cursor<'db>,
    started: bool,
    start: Vec<u8>,
    table_prefix: Vec<u8>,
    lo: Bound<&'b [u8]>,
    hi: Bound<&'b [u8]>,
}

impl TableRows<'_, '_> {
    /// The next `(key, value)` inside the table and bounds, copied out of
    /// the cursor's buffers, or `None` at the end of the walk.
    #[inline(never)]
    fn next(&mut self) -> Result<Option<Row>, StoreError> {
        loop {
            let seek = if self.started {
                Seek::Next
            } else {
                self.started = true;
                Seek::AtOrAfter(self.start.as_slice())
            };
            // The row borrows the cursor, so everything this iteration
            // keeps is copied out before the next `get` — which is what
            // makes the borrow check the enforcement of libdb's rule.
            let (key, value) = match self.cursor.get(seek)? {
                None => return Ok(None),
                Some(row) => {
                    let Some(key) = unframe(&self.table_prefix, row.key) else {
                        // Past the last row of this table.
                        return Ok(None);
                    };
                    (key.to_vec(), row.value.to_vec())
                }
            };
            if matches!(self.lo, Bound::Excluded(bound) if key.as_slice() == bound) {
                continue;
            }
            match self.hi {
                Bound::Unbounded => {}
                Bound::Included(bound) if key.as_slice() <= bound => {}
                Bound::Excluded(bound) if key.as_slice() < bound => {}
                _ => return Ok(None),
            }
            return Ok(Some((key, value)));
        }
    }
}

impl ReadStore for BdbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_BTREE, true, false, ffi::SYSTEM_FILE_MODE)?,
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
        Ok(!self.first_key_of(table)?)
    }
}

impl RawReadStore for BdbStore {
    fn get_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.db.get(key, value)
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

    /// libdb's in-memory database (`DB->open` with a null file name, the
    /// form `ngram_bdb.cpp:51-56` uses for the user bigram), as a
    /// `DB_BTREE` so the table framing's ordered walks hold.
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
            store: self,
            buffer: BTreeMap::new(),
        };
        let out = f(&mut txn)?;
        self.commit(txn.buffer)?;
        Ok(out)
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
    store: &'store BdbStore,
    /// Key (framed for table rows, bare for `put_raw` rows) → new value,
    /// or `None` for a removal. Ordered so the apply pass touches keys
    /// in B-tree order.
    buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl WriteTxn for BdbTxn<'_> {
    // The point operations stay out of line: the user store calls them from
    // hundreds of sites, and inlining each one's validation, framing and
    // buffer insert (or FFI call) into every caller costs far more code
    // than a direct call. Dispatch is static either way.
    #[inline(never)]
    fn get(&self, table: &str, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let framed = frame(table, key);
        // Read-your-writes: the buffer wins over the database.
        match self.buffer.get(&framed) {
            Some(slot) => Ok(fill_buffered(slot.as_deref(), value)),
            None => self.store.db.get(&framed, value),
        }
    }

    #[inline(never)]
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.buffer.insert(frame(table, key), Some(value.to_vec()));
        Ok(())
    }

    /// The bare-keyspace half of the raw seam: what `stage_dbm`'s tree
    /// containers and the user-bigram writer write through, and what
    /// `get_raw`/`range_raw` read back on this backend.
    #[inline(never)]
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.buffer.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    /// The removal half of [`WriteTxn::put_raw`]: a bare key, no framing.
    #[inline(never)]
    fn remove_raw(&mut self, key: &[u8]) -> Result<(), StoreError> {
        self.buffer.insert(key.to_vec(), None);
        Ok(())
    }

    #[inline(never)]
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.buffer.insert(frame(table, key), None);
        Ok(())
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        self.merged_walk(table, lo, hi, visit)
    }

    #[inline(never)]
    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let mut empty = true;
        self.merged_walk(table, Bound::Unbounded, Bound::Unbounded, |_, _| {
            empty = false;
            // Stop at the first row rather than walking the table.
            Ok(Flow::Break(()))
        })?;
        Ok(empty)
    }
}

impl BdbTxn<'_> {
    /// A walk that reads the database and the buffer as one table.
    ///
    /// The buffer is small (one closure's writes) while the table may be
    /// large, so the merge collects the buffer's rows for this table and
    /// walks the database once, splicing them in by key order and
    /// skipping database rows the buffer has overwritten or removed.
    fn merged_walk(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        let mut rows = self.merged_rows(table, lo, hi)?;
        while let Some((key, value)) = rows.next()? {
            if visit(key, value)?.is_break() {
                break;
            }
        }
        Ok(())
    }

    /// The row stream behind [`Self::merged_walk`].
    fn merged_rows<'t, 'b>(
        &'t self,
        table: &'t str,
        lo: Bound<&'b [u8]>,
        hi: Bound<&'b [u8]>,
    ) -> Result<MergedRows<'t, 'b>, StoreError> {
        validate_table_name(table)?;
        let table_prefix = prefix(table);

        let mut pending: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (framed, value) in &self.buffer {
            let Some(key) = unframe(&table_prefix, framed) else {
                continue;
            };
            if let Some(value) = value
                && in_bounds(key, lo, hi)
            {
                pending.push((key.to_vec(), value.clone()));
            }
        }
        // `buffer` is a BTreeMap over framed keys, and every key here
        // shares one prefix, so `pending` is already in key order.
        Ok(MergedRows {
            stored: self.store.table_rows(table, lo, hi)?,
            table,
            buffer: &self.buffer,
            pending,
            next_pending: 0,
            stored_done: false,
            held: None,
            spent: false,
        })
    }
}

/// Which buffer of a [`MergedRows`] holds the row [`MergedRows::advance`]
/// settled on.
enum Settled {
    End,
    /// The held stored row.
    Stored,
    /// `pending[index]`.
    Pending(usize),
}

/// A transaction's view of one table: the database's rows and the
/// transaction's buffered writes read as one table, in key order.
///
/// The buffer is small (one closure's writes) while the table may be
/// large, so the merge holds the buffer's live rows for this table in
/// `pending` and streams the database once, splicing them in by key
/// order and skipping database rows the buffer has overwritten or
/// removed. Like [`TableRows`] it is a concrete stepper, so the merge
/// exists once and each visitor's instantiation is a thin loop.
struct MergedRows<'t, 'b> {
    stored: TableRows<'t, 'b>,
    table: &'t str,
    buffer: &'t BTreeMap<Vec<u8>, Option<Vec<u8>>>,
    pending: Vec<(Vec<u8>, Vec<u8>)>,
    next_pending: usize,
    stored_done: bool,
    /// A stored row read ahead and not yet settled against `pending`.
    held: Option<(Vec<u8>, Vec<u8>)>,
    /// The held row was handed out by the last call; drop it before
    /// settling the next one.
    spent: bool,
}

impl MergedRows<'_, '_> {
    /// The next merged `(key, value)`, borrowed from this stream until the
    /// following call, or `None` at the end.
    #[inline(never)]
    fn next(&mut self) -> Result<Option<BorrowedRow<'_>>, StoreError> {
        Ok(match self.advance()? {
            Settled::End => None,
            Settled::Stored => self
                .held
                .as_ref()
                .map(|(key, value)| (key.as_slice(), value.as_slice())),
            Settled::Pending(index) => self
                .pending
                .get(index)
                .map(|(key, value)| (key.as_slice(), value.as_slice())),
        })
    }

    /// Settles which row comes next: buffered inserts below the next
    /// stored key first; then the stored row itself unless the buffer
    /// overrides or removes it; past the last stored row, the remaining
    /// buffered inserts.
    fn advance(&mut self) -> Result<Settled, StoreError> {
        if self.spent {
            self.held = None;
            self.spent = false;
        }
        loop {
            if self.held.is_none() && !self.stored_done {
                match self.stored.next()? {
                    Some(row) => self.held = Some(row),
                    None => self.stored_done = true,
                }
            }
            let Some((stored_key, _)) = &self.held else {
                // Past the last stored row, whatever buffering remains is
                // all inserts.
                return Ok(self.take_pending());
            };
            if let Some((pending_key, _)) = self.pending.get(self.next_pending)
                && pending_key < stored_key
            {
                return Ok(self.take_pending());
            }
            let overwritten = self.buffer.contains_key(&frame(self.table, stored_key));
            if !overwritten {
                self.spent = true;
                return Ok(Settled::Stored);
            }
            // Either removed, or already emitted from `pending`.
            let emitted = self.next_pending > 0
                && self
                    .pending
                    .get(self.next_pending - 1)
                    .is_some_and(|(key, _)| key == stored_key);
            let overridden = self
                .pending
                .get(self.next_pending)
                .is_some_and(|(key, _)| key == stored_key);
            self.held = None;
            if !emitted && overridden {
                return Ok(self.take_pending());
            }
        }
    }

    /// The next buffered insert, or the end when none remains.
    fn take_pending(&mut self) -> Settled {
        if self.next_pending < self.pending.len() {
            self.next_pending += 1;
            Settled::Pending(self.next_pending - 1)
        } else {
            Settled::End
        }
    }
}

// ── the user bigram's in-memory container ─────────────────────────

/// libpinyin's Berkeley DB user bigram as `Bigram::load_db` leaves it:
/// an in-memory `DB_HASH` filled from the file by `copy_bdb`'s cursor
/// walk (`ngram_bdb.cpp:47-79`, `bdb_utils.h:43-74`), so its walk is the
/// pin's.
pub struct BdbUserBigramDb {
    db: Db,
}

/// Visits one borrowed record at a time in the database's native order.
fn visit_db(
    db: &Db,
    mut visit: impl FnMut(&[u8], &[u8]) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    let mut cursor = db.cursor()?;
    let mut seek = ffi::Seek::First;
    while let Some(row) = cursor.get(seek)? {
        visit(row.key, row.value)?;
        seek = ffi::Seek::Next;
    }
    Ok(())
}

impl crate::UserBigramDb for BdbUserBigramDb {
    fn empty() -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_in_memory_hash()?,
        })
    }

    fn load_db(path: &Path) -> Result<Self, StoreError> {
        let this = Self::empty()?;
        // `tmp_db->open(…, dbfile, NULL, DB_HASH, DB_RDONLY, 0600)` then
        // `copy_bdb(tmp_db, m_db)`; a file that does not open leaves the
        // container empty, as the pin's does.
        if let Ok(file) = Db::open(path, ffi::DB_HASH, true, false, ffi::USER_FILE_MODE) {
            // Keep the usable in-memory container if source traversal fails;
            // callers of the pin retain it even when load_db returns false.
            if let Err(error) = visit_db(&file, |key, value| this.db.put(key, value)) {
                eprintln!("user-bigram load {}: {error}", path.display());
            }
        }
        Ok(this)
    }

    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.db.get(key, value)
    }

    fn store(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db.put(key, value)
    }

    fn remove(&self, key: &[u8]) -> Result<(), StoreError> {
        self.db.del(key)
    }

    fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError> {
        // `visit_db`'s walk without the values: the same cursor order, into
        // one vector of four-byte tokens, as `get_all_items` appends them
        // to one `GArray` (`ngram_bdb.cpp:178-215` at 074a2219). Berkeley
        // DB keeps no record count to reserve by, so it grows as it goes.
        let mut keys = Vec::new();
        let mut cursor = self.db.cursor()?;
        let mut seek = ffi::Seek::First;
        while let Some(key) = cursor.key(seek)? {
            keys.push(<[u8; 4]>::try_from(key).ok());
            seek = ffi::Seek::Next;
        }
        Ok(keys)
    }

    fn save_db(&self, path: &Path) -> Result<(), StoreError> {
        // `Bigram::save_db` (`ngram_bdb.cpp:81-107`): unlink, create a
        // `DB_HASH` file (`DB_CREATE, 0600`), copy this container into it
        // in cursor order, sync, close.
        crate::remove_if_present(path)?;
        let file = Db::open(path, ffi::DB_HASH, false, true, ffi::USER_FILE_MODE)?;
        visit_db(&self.db, |key, value| file.put(key, value))?;
        file.sync()
    }
}
