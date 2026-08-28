//! Berkeley DB 5.3 backend — the libpinyin compatibility route.
//!
//! Enabled by the `bdb` cargo feature. Two things live here, and keeping
//! them apart matters:
//!
//! * [`BdbStore`] — a `DB_BTREE` implementation of the store's two
//!   capability tiers, addressed by `(table, key)` like every other
//!   backend.
//! * [`BigramDb`] — raw `DB_HASH` access to libpinyin's own `bigram.db`,
//!   keyed by the four native-endian bytes of a `phrase_token_t` and
//!   valued by a [`SingleGram`] chunk. This is deliberately **not** a
//!   [`ReadStore`]: a hash database has no ordering, so it cannot honour
//!   `range` or an ordered `for_each`, and pretending otherwise would put
//!   an ordering contract on a file that has none.
//!
//! # Key ordering
//!
//! `DB_BTREE` opened without `set_bt_compare` — which is how libpinyin
//! opens `phrase_large_table3_bdb.cpp` and `chewing_large_table2_bdb.cpp`
//! — uses Berkeley DB's default comparator: byte-wise `memcmp`, shorter
//! key first on a shared prefix. That is the store's one rule exactly, so
//! this backend satisfies it without configuration, and setting a
//! comparator would silently reorder files libpinyin wrote.
//!
//! Confirmed experimentally rather than taken from the documentation
//! (`docs/findings/berkeleydb-backend.md`): a `DB_BTREE` loaded with
//! little-endian `u32` array keys that cross 256 in the first and in a
//! later element walks in raw-byte order, not integer order — the
//! 1-element key `00000001` immediately precedes the 2-element key
//! `0000000102010000` that extends it, and the decoded values run
//! `0x01000000, 0x00010000, 0x00ff0000, 0x00000100, …`, which is not
//! ascending. The cross-backend conformance tests in `super` assert this
//! backend walks identically to redb, LMDB and tkrzw over exactly those
//! keys.
//!
//! # Atomicity
//!
//! Weaker than redb's and LMDB's, and for the same reason as tkrzw's.
//! libpinyin uses no Berkeley DB environment and no transactions — every
//! `open` passes `NULL` for both — so a standalone `DB` handle has no
//! transaction to commit. [`WriteStore::write`] therefore buffers the
//! closure's puts and removes, answers in-closure reads from that buffer
//! over the database, and applies the buffer in one pass on `Ok`;
//! on `Err` the buffer is dropped and nothing is written. A crash *during*
//! that apply can leave part of a batch on disk. Each commit ends with
//! `DB->sync`, so once it returns the bytes are in the operating system's
//! hands and visible to any reader, including after a process crash —
//! but that is not stable storage against power loss.
//!
//! Matching libpinyin here is the point: a transactional environment
//! would write logs and a region directory beside the user's profile,
//! which the user's own libpinyin does not expect and would not clean up.
//!
//! # Threading
//!
//! [`BdbStore`] is `!Send` and `!Sync`, by construction rather than by
//! choice: it holds a libdb handle opened without `DB_THREAD`, exactly as
//! libpinyin opens its own. `DB_THREAD` would make the handle
//! free-threaded, but it also requires `DB_DBT_MALLOC`, `DB_DBT_REALLOC`
//! or `DB_DBT_USERMEM` on every `DBT`, which means a copy of every record
//! on every read — including the full-file walks. The zero-copy reads
//! this backend does instead are only sound single-threaded, so the type
//! says so. `docs/findings/berkeleydb-backend.md` records what this costs
//! and why it blocks the default-backend switch.
#![allow(unsafe_code)]

mod ffi;
pub mod single_gram;

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::Path;

use crate::{ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

pub use single_gram::SingleGram;

use ffi::{Cursor, Db, Seek};

/// The framing separator between a table name and a key.
///
/// Table names are validated NUL-free, so no framed prefix is a prefix of
/// another: every table is a contiguous run of the B-tree whose internal
/// order is the caller's key order.
const SEPARATOR: u8 = 0;

/// `table || 0x00 || key`.
fn frame(table: &str, key: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(table.len() + 1 + key.len());
    framed.extend_from_slice(table.as_bytes());
    framed.push(SEPARATOR);
    framed.extend_from_slice(key);
    framed
}

/// `table || 0x00` — the prefix every one of `table`'s rows carries.
fn prefix(table: &str) -> Vec<u8> {
    frame(table, &[])
}

/// The caller's key inside a framed one, or `None` if the framed key
/// belongs to another table.
fn unframe<'a>(prefix: &[u8], framed: &'a [u8]) -> Option<&'a [u8]> {
    framed.strip_prefix(prefix)
}

/// A Berkeley DB `DB_BTREE` store implementing both capability tiers.
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
            // The record borrows the cursor, so everything this iteration
            // keeps is copied out before the next `get` — which is what
            // makes the borrow check the enforcement of libdb's rule.
            let (key, value) = match cursor.get(seek)? {
                None => return Ok(()),
                Some(record) => {
                    let Some(key) = unframe(&table_prefix, record.key) else {
                        // Past the last row of this table.
                        return Ok(());
                    };
                    (key.to_vec(), record.value.to_vec())
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
                Some(record) => unframe(&table_prefix, record.key).is_some(),
            },
        )
    }

    /// Opens a libpinyin `bigram.db` beside this store's own file.
    ///
    /// Nothing about [`BdbStore`] is involved; this is here so a caller
    /// that already links the backend does not need a second import.
    ///
    /// # Errors
    ///
    /// Whatever [`BigramDb::open`] reports.
    pub fn open_bigram(path: &Path, read_only: bool) -> Result<BigramDb, StoreError> {
        BigramDb::open(path, read_only)
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

impl WriteStore for BdbStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_BTREE, false, true)?,
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
        for (framed, value) in buffer {
            match value {
                Some(value) => self.db.put(&framed, &value)?,
                None => self.db.del(&framed)?,
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
        // Berkeley DB reclaims freed pages into its own free list and
        // reuses them in place. `DB->compact` exists in 5.3 but needs a
        // transactional environment to move data, which this backend
        // deliberately does not create (see the module note); without one
        // there is nothing to do beyond making the current state durable.
        // Same shape as the LMDB backend, whose successful `compact` also
        // does not shrink the file.
        self.db.sync()
    }
}

/// The buffered write transaction described in the module note.
struct BdbTxn<'store> {
    store: &'store BdbStore,
    /// Framed key → new value, or `None` for a removal. Ordered so the
    /// apply pass touches keys in B-tree order.
    buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl BdbTxn<'_> {
    /// Read-your-writes: the buffer wins over the database.
    fn buffered(&self, framed: &[u8]) -> Option<Option<Vec<u8>>> {
        self.buffer.get(framed).cloned()
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
        let in_bounds = |key: &[u8]| {
            let above_lo = match lo {
                Bound::Unbounded => true,
                Bound::Included(bound) => key >= bound,
                Bound::Excluded(bound) => key > bound,
            };
            let below_hi = match hi {
                Bound::Unbounded => true,
                Bound::Included(bound) => key <= bound,
                Bound::Excluded(bound) => key < bound,
            };
            above_lo && below_hi
        };

        let mut pending: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        for (framed, value) in &self.buffer {
            let Some(key) = unframe(&table_prefix, framed) else {
                continue;
            };
            if let Some(value) = value
                && in_bounds(key)
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

// ── libpinyin's own bigram.db ──────────────────────────────────────

/// libpinyin's `bigram.db`: a `DB_HASH` database keyed by the four
/// native-endian bytes of a `phrase_token_t`, valued by a whole
/// [`SingleGram`] chunk.
///
/// Blob-per-previous-token, which is upstream's model: every successor of
/// `prev` lives in one value, and every access is a point `get`, `put` or
/// `del`. There is no ordering contract — a hash database has no order —
/// which is why this type is not a [`ReadStore`] and offers no `range`.
///
/// The same format serves the system `bigram.db` and the user's, so one
/// type reads both; only the open mode differs.
pub struct BigramDb {
    db: Db,
}

impl BigramDb {
    /// Opens a `bigram.db`, exactly as `ngram_bdb.cpp` does: `DB_HASH`,
    /// no environment, no transaction, mode 0644.
    ///
    /// A read-only open never creates; a writable open creates when the
    /// file is absent, which is how a user's first training run makes one.
    ///
    /// # Errors
    ///
    /// [`StoreError`] when libdb refuses the open — a missing file for a
    /// read-only open, a wrong database type, or an unsupported libdb.
    pub fn open(path: &Path, read_only: bool) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, ffi::DB_HASH, read_only, !read_only)?,
        })
    }

    /// The four raw bytes libpinyin uses as a key: `&index` with
    /// `size = sizeof(phrase_token_t)`, native-endian.
    fn key(prev: u32) -> [u8; 4] {
        prev.to_ne_bytes()
    }

    /// The gram stored for `prev`, or `None`.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from libdb, or from a chunk that does not satisfy
    /// the layout invariants ([`SingleGram::decode`]).
    pub fn get(&self, prev: u32) -> Result<Option<SingleGram>, StoreError> {
        match self.db.get(&Self::key(prev))? {
            None => Ok(None),
            Some(bytes) => SingleGram::decode(&bytes).map(Some),
        }
    }

    /// Stores `gram` under `prev`, in libpinyin's byte layout.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from libdb, or [`StoreError::ReadOnly`].
    pub fn put(&self, prev: u32, gram: &SingleGram) -> Result<(), StoreError> {
        self.db.put(&Self::key(prev), &gram.encode())
    }

    /// Removes `prev`'s gram; absent is not an error.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from libdb, or [`StoreError::ReadOnly`].
    pub fn remove(&self, prev: u32) -> Result<(), StoreError> {
        self.db.del(&Self::key(prev))
    }

    /// The chunk stored for `prev`, undecoded.
    ///
    /// The compatibility gate uses this to compare what
    /// [`SingleGram::encode`] produces against the bytes already in the
    /// file; ordinary callers want [`Self::get`].
    ///
    /// # Errors
    ///
    /// [`StoreError`] from libdb.
    pub fn raw(&self, prev: u32) -> Result<Option<Vec<u8>>, StoreError> {
        self.db.get(&Self::key(prev))
    }

    /// Flushes to the operating system.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from libdb.
    pub fn sync(&self) -> Result<(), StoreError> {
        self.db.sync()
    }

    /// Visits every `(prev, gram)` record.
    ///
    /// The order is the hash database's, which is not the key order and
    /// carries no contract — callers that need an order impose their own.
    ///
    /// # Errors
    ///
    /// [`StoreError`] from libdb, from a malformed key, from a chunk that
    /// fails [`SingleGram::decode`], or whatever `visit` returns.
    pub fn for_each(
        &self,
        visit: &mut dyn FnMut(u32, &SingleGram) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        let mut cursor: Cursor<'_> = self.db.cursor()?;
        let mut seek = Seek::First;
        loop {
            let (prev, gram) = match cursor.get(seek)? {
                None => return Ok(()),
                Some(record) => {
                    let key: [u8; 4] = record.key.try_into().map_err(|_| {
                        StoreError::Backend(
                            format!(
                                "corrupt bigram key: {} bytes, expected the 4 of a \
                                 phrase_token_t",
                                record.key.len()
                            )
                            .into(),
                        )
                    })?;
                    (u32::from_ne_bytes(key), SingleGram::decode(record.value)?)
                }
            };
            seek = Seek::Next;
            visit(prev, &gram)?;
        }
    }
}
