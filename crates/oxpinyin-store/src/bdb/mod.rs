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
//! identically to redb, LMDB, tkrzw and Kyoto Cabinet over exactly
//! those keys.
//!
//! # Atomicity
//!
//! Weaker than redb's and LMDB's, and for the same reason as tkrzw's.
//! libpinyin uses no Berkeley DB environment and no transactions — every
//! `open` passes `NULL` for both — so a standalone `DB` handle has no
//! transaction to commit. [`crate::WriteStore::write`] therefore buffers
//! the closure's puts and removes, answers in-closure reads from that
//! buffer over the database, and applies the buffer in one pass on `Ok`;
//! on `Err` the buffer is dropped and nothing is written. A crash
//! *during* that apply can leave part of a batch on disk. Each commit
//! ends with `DB->sync`, so once it returns the bytes are in the
//! operating system's hands and visible to any reader, including after a
//! process crash — but that is not stable storage against power loss;
//! [`crate::WriteStore::compact`] is the stable-storage point, as on
//! every backend.
//!
//! Matching libpinyin here is the point: a transactional environment
//! would write log and region files beside the user's profile, which
//! the user's own libpinyin does not expect and would not clean up.
//!
//! # Threading
//!
//! [`BdbStore`] is `Send` and `Sync`. The handle is opened with
//! `DB_THREAD` and every read lands in caller-owned
//! (`DB_DBT_USERMEM`) memory — libdb's documented contract for a
//! handle used from multiple threads, and what makes both claims sound
//! (`src/bdb/ffi.rs` carries the audit). Concurrent reads share the
//! handle the contract's way; the user store's writes go through its
//! own `Mutex` on top.

mod ffi;

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::Path;

use crate::common::{frame, in_bounds, prefix, unframe};
use crate::{
    RawReadStore, ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name,
};

use ffi::{Db, Seek};

/// A Berkeley DB store: a `DB_BTREE` container when opened by
/// [`ReadStore::open_read_only`] / [`WriteStore::create`], a `DB_HASH`
/// raw keyspace when opened by the hash constructors.
pub struct BdbStore {
    db: Db,
}

impl BdbStore {
    /// Walks `table`'s rows in ascending key order, within `[lo, hi]`,
    /// handing each to `visit`.
    ///
    /// One cursor, positioned once with `DB_SET_RANGE` and advanced with
    /// `DB_NEXT`; the walk stops at the first key outside the table's
    /// framed prefix, so a scan costs the rows it returns and one more.
    fn walk(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let table_prefix = prefix(table);
        let start = match lo {
            Bound::Unbounded => table_prefix.clone(),
            Bound::Included(key) | Bound::Excluded(key) => frame(table, key),
        };
        let mut cursor = self.db.cursor()?;
        let mut seek = Seek::AtOrAfter(start.as_slice());
        loop {
            // The row borrows the cursor, so everything this iteration
            // keeps is copied out before the next `get` — which is what
            // makes the borrow check the enforcement of libdb's rule.
            let (key, value) = match cursor.get(seek)? {
                None => return Ok(()),
                Some(row) => {
                    let Some(key) = unframe(&table_prefix, row.key) else {
                        // Past the last row of this table.
                        return Ok(());
                    };
                    (key.to_vec(), row.value.to_vec())
                }
            };
            seek = Seek::Next;
            if matches!(lo, Bound::Excluded(bound) if key.as_slice() == bound) {
                continue;
            }
            match hi {
                Bound::Unbounded => {}
                Bound::Included(bound) if key.as_slice() <= bound => {}
                Bound::Excluded(bound) if key.as_slice() < bound => {}
                _ => return Ok(()),
            }
            visit(&key, &value)?;
        }
    }

    /// Whether `table` has any row at all — one cursor positioning, never
    /// a scan.
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
        visit: &mut Visitor<'_>,
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
            visit(row.key, row.value)?;
        }
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
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor()?;
        let mut seek = Seek::First;
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        loop {
            let (key, value) = match cursor.get(seek)? {
                None => break,
                Some(row) => (row.key.to_vec(), row.value.to_vec()),
            };
            seek = Seek::Next;
            rows.push((key, value));
        }
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        for (key, value) in rows {
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
            visit(&key, &value)?;
        }
        Ok(())
    }
}

impl ReadStore for BdbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_BTREE, true, false)?,
        })
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        self.db.get(&frame(table, key))
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        self.walk(table, lo, hi, visit)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        self.walk(table, Bound::Unbounded, Bound::Unbounded, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        Ok(!self.first_key_of(table)?)
    }
}

impl RawReadStore for BdbStore {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        self.db.get(key)
    }

    fn range_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        if self.db.is_hash() {
            self.range_raw_unordered(lo, hi, visit)
        } else {
            self.range_raw_ordered(lo, hi, visit)
        }
    }

    fn open_hash_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_HASH, true, false)?,
        })
    }
}

impl WriteStore for BdbStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_BTREE, false, true)?,
        })
    }

    /// The `DB_HASH` container: libpinyin's `bigram.db` form, the hash
    /// half of this backend's raw seam (see [`RawReadStore`]).
    fn create_hash(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_HASH, false, true)?,
        })
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
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
        let buffer = txn.buffer;
        for (key, value) in buffer {
            match value {
                Some(value) => self.db.put(&key, &value)?,
                None => {
                    self.db.del(&key)?;
                }
            }
        }
        // Push the batch to the operating system, so a reader — including
        // the user's own libpinyin — sees a consistent file once `write`
        // returns.
        self.db.sync()?;
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
        // `DB->sync` — and the file not shrinking is the same shape as
        // the LMDB backend's successful compact.
        self.db.sync()
    }
}

/// The buffered write transaction described in the module note.
struct BdbTxn<'store> {
    store: &'store BdbStore,
    /// Key (framed for table rows, bare for `put_raw` rows) → new value,
    /// or `None` for a removal. Ordered so the apply pass touches keys
    /// in B-tree order.
    buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl BdbTxn<'_> {
    /// Read-your-writes: the buffer wins over the database.
    fn buffered(&self, key: &[u8]) -> Option<Option<Vec<u8>>> {
        self.buffer.get(key).cloned()
    }
}

impl WriteTxn for BdbTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        let framed = frame(table, key);
        match self.buffered(&framed) {
            Some(buffered) => Ok(buffered),
            None => self.store.db.get(&framed),
        }
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.buffer.insert(frame(table, key), Some(value.to_vec()));
        Ok(())
    }

    /// The bare-keyspace half of the raw seam: what `stage_dbm`'s tree
    /// containers and the user-bigram writer write through, and what
    /// `get_raw`/`range_raw` read back on this backend.
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.buffer.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.buffer.insert(frame(table, key), None);
        Ok(())
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        self.merged_walk(table, lo, hi, visit)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        self.merged_walk(table, Bound::Unbounded, Bound::Unbounded, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let mut empty = true;
        self.merged_walk(table, Bound::Unbounded, Bound::Unbounded, &mut |_, _| {
            empty = false;
            // Stop at the first row rather than walking the table.
            Err(StoreError::Backend(STOP.into()))
        })
        .or_else(|error| match &error {
            StoreError::Backend(inner) if inner.to_string() == STOP => Ok(()),
            _ => Err(error),
        })?;
        Ok(empty)
    }
}

/// Sentinel that unwinds `is_empty`'s walk after the first row.
const STOP: &str = "oxpinyin-store: stop walk";

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
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
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

        let mut next_pending = 0_usize;
        let mut result = Ok(());
        self.store.walk(table, lo, hi, &mut |key, value| {
            while next_pending < pending.len() && pending[next_pending].0.as_slice() < key {
                let (buffered_key, buffered_value) = &pending[next_pending];
                next_pending += 1;
                visit(buffered_key, buffered_value)?;
            }
            let overwritten = self.buffer.contains_key(&frame(table, key));
            if overwritten {
                // Either removed, or already emitted from `pending`.
                if next_pending > 0 && pending[next_pending - 1].0.as_slice() == key {
                    return Ok(());
                }
                if next_pending < pending.len() && pending[next_pending].0.as_slice() == key {
                    let (buffered_key, buffered_value) = &pending[next_pending];
                    next_pending += 1;
                    return visit(buffered_key, buffered_value);
                }
                return Ok(());
            }
            visit(key, value)
        })?;
        while next_pending < pending.len() && result.is_ok() {
            let (key, value) = &pending[next_pending];
            next_pending += 1;
            result = visit(key, value);
        }
        result
    }
}
