//! tkrzw backend for the store capability tiers, over TreeDBM.
//!
//! Enabled by the `tkrzw` cargo feature. Mirrors libpinyin at `0c5e80e`
//! (`src/storage/tkrzwdb_utils.h`, `chewing_large_table2_tkrzwdb.cpp`,
//! `ngram_tkrzwdb.cpp`): TreeDBM only, opened with tkrzw's default
//! tuning and therefore its default `LexicalKeyComparator`. No custom
//! comparator is installed, so records sort by plain unsigned byte
//! order and oxpinyin's big-endian key codec keeps the ordering it has
//! under redb and LMDB.
//!
//! # Zero-copy reads
//!
//! Reads borrow tkrzw's record memory instead of copying it, the way
//! libpinyin's own tkrzw code does — its `KeyCollectProcessor` walks
//! records through `ProcessEach` reading in-place `string_view`s. Here
//! the shim's walk drives an iterator's `Process` and hands every key
//! and value to a Rust callback as an `&[u8]` slice over tkrzw's
//! record buffer: no intermediary `std::string`, no per-byte
//! `push_back`. A borrow lasts only for the callback's duration; a
//! visitor that wants to keep a row copies exactly what it retains, and
//! a point `get` makes the one owned copy a result that outlives the
//! call requires. Carrying a borrowed slice past its callback is the
//! one thing the design forbids.
//!
//! This module and its `bridge` carry an explicit `allow(unsafe_code)`:
//! the callback plumbing needs a handful of hand-written `unsafe`
//! blocks (raw token derefs, documented at each site). That waiver is
//! scoped to the tkrzw backend by decision — the workspace outside it
//! stays `deny` — and it waives safety ceremony, not correctness: the
//! shared read and write suites gate this backend like any other.
//!
//! # One keyspace, many tables
//!
//! A TreeDBM file is a single flat keyspace, while [`ReadStore`] and
//! [`WriteStore`] are addressed by `(table, key)`. Records are therefore
//! stored under `table-name || 0x00 || key`. The framing is
//! prefix-free — table names are validated NUL-free, so no framed
//! prefix is a prefix of another — which makes each table a contiguous
//! run whose internal order is the caller's key order, and makes
//! `for_each`, `range` and `is_empty` prefix scans. The framing is
//! internal: nothing outside this module sees it, and no file written
//! by another backend is readable here or vice versa.
//!
//! # Atomicity
//!
//! [`WriteStore::write`] buffers the closure's puts and removes,
//! answers in-closure reads from that buffer over the database, and on
//! `Ok` applies the whole buffer in one `ProcessMulti` call; on `Err`
//! the buffer is dropped and nothing is written. `ProcessMulti` locks
//! every named record for the duration, so the batch lands as a unit
//! against any other reader or writer.
//!
//! This is weaker than what redb and LMDB give. Their commits are
//! crash-atomic: a torn write is rolled back on the next open. TreeDBM
//! has no write-ahead log, so a crash *during* the `ProcessMulti` apply
//! can leave part of a batch on disk. Every commit calls
//! `Synchronize(hard=false)`: buffered data is flushed to the operating
//! system, so once the call returns the file on disk is consistent and
//! visible to any reader, including after a process crash. That is not
//! stable storage, though — the bytes are in the kernel's hands, and a
//! machine crash or power loss can still lose them. A backend
//! comparison should record the difference rather than read `write` as
//! three equivalent implementations.
//!
//! # Platform and the library build
//!
//! Linux-first, like the other evaluation backend: paths are handed to
//! tkrzw as bytes.
//!
//! tkrzw must be built from source. `DBM::RecordProcessor::NOOP` and
//! `REMOVE` are sentinels recognised by *pointer* identity — tkrzw's
//! own header says to compare `your_value.data() == NOOP.data()` — and
//! Ubuntu noble's `libtkrzw-dev 1.0.27-1.1build1` is built so that a
//! client and the shared library disagree about those addresses. The
//! symptom is silent: `Remove` writes the five-byte `REMOVE` sentinel
//! as the record's value instead of deleting it, a no-op processor
//! stores `NOOP`'s bytes, and `Rebuild` aborts with `CANCELED_ERROR`
//! (`tkrzw_dbm_hash_impl.cc:424` compares against `NOOP.data()`).
//! That build's own `tkrzw_dbm_util` cannot reopen a TreeDBM it just
//! created, failing `BROKEN_DATA_ERROR: invalid_key_comparator` — the
//! same divergence, applied to the comparator function pointer. The
//! identical 1.0.27 sources built with `./configure && make` behave
//! correctly, and this backend's tests pass against them; `build.rs`
//! says as much when the library is missing.
#![allow(unsafe_code)]

mod bridge;

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;
use std::ops::Bound;
use std::path::Path;

use cxx::UniquePtr;

use bridge::ffi;

use crate::{ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

/// `tkrzw::Status::Code::SUCCESS`.
const STATUS_SUCCESS: i32 = 0;
/// `tkrzw::Status::Code::SYSTEM_ERROR` — the codes that came from the
/// operating system, reported as [`StoreError::Io`] so callers can
/// branch on I/O failures the way they do for the other backends.
const STATUS_SYSTEM_ERROR: i32 = 2;

// ── errors ────────────────────────────────────────────────────────

/// A non-I/O tkrzw status, carrying the code and message verbatim.
#[derive(Debug)]
struct TkrzwError {
    code: i32,
    message: String,
}

impl fmt::Display for TkrzwError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.message.is_empty() {
            write!(f, "tkrzw status {}", self.code)
        } else {
            write!(f, "tkrzw status {}: {}", self.code, self.message)
        }
    }
}

impl std::error::Error for TkrzwError {}

/// The handle was closed, or a `UniquePtr` was unexpectedly null.
///
/// Unreachable through the public API — a `TkrzwStore` only exists once
/// `open_db` returned a live handle — but reported instead of unwrapped
/// so no input can panic (constitution §4).
#[derive(Debug)]
struct ClosedError;

impl fmt::Display for ClosedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("tkrzw database handle is closed")
    }
}

impl std::error::Error for ClosedError {}

/// A scan visitor panicked. The panic is caught at the shim boundary and
/// reported as this error: a visitor runs inside tkrzw's call stack, and
/// an unwind escaping through the C++ bridge would be undefined
/// behaviour, so it must never be allowed to propagate.
#[derive(Debug)]
struct VisitorPanicked;

impl fmt::Display for VisitorPanicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("store scan visitor panicked")
    }
}

impl std::error::Error for VisitorPanicked {}

fn check(status: ffi::ShimStatus) -> Result<(), StoreError> {
    match status.code {
        STATUS_SUCCESS => Ok(()),
        STATUS_SYSTEM_ERROR => Err(StoreError::Io(std::io::Error::other(status.message))),
        code => Err(StoreError::Backend(Box::new(TkrzwError {
            code,
            message: status.message,
        }))),
    }
}

fn blank_status() -> ffi::ShimStatus {
    ffi::ShimStatus {
        code: STATUS_SUCCESS,
        message: String::new(),
    }
}

// ── key framing ───────────────────────────────────────────────────

/// The prefix every record of `table` is stored under.
///
/// `validate_table_name` has already rejected the empty name and any
/// name containing NUL, which is exactly what makes appending a NUL a
/// prefix-free framing.
fn table_prefix(table: &str) -> Result<Vec<u8>, StoreError> {
    validate_table_name(table)?;
    let mut prefix = Vec::with_capacity(table.len() + 1);
    prefix.extend_from_slice(table.as_bytes());
    prefix.push(0);
    Ok(prefix)
}

fn framed(prefix: &[u8], key: &[u8]) -> Vec<u8> {
    let mut framed = Vec::with_capacity(prefix.len() + key.len());
    framed.extend_from_slice(prefix);
    framed.extend_from_slice(key);
    framed
}

/// Whether `key` falls inside `(lo, hi)` under the same semantics the
/// other backends' `range` uses.
fn in_bounds(key: &[u8], lo: Bound<&[u8]>, hi: Bound<&[u8]>) -> bool {
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
}

/// Row callback for [`scan`]. Returning `false` stops the walk.
type Row<'a> = dyn FnMut(&[u8], &[u8]) -> Result<bool, StoreError> + 'a;

/// Whether iteration has walked past `hi` and can stop.
fn past_upper(key: &[u8], hi: Bound<&[u8]>) -> bool {
    match hi {
        Bound::Unbounded => false,
        Bound::Included(bound) => key > bound,
        Bound::Excluded(bound) => key >= bound,
    }
}

// ── store ─────────────────────────────────────────────────────────

/// A tkrzw-backed store implementing both capability tiers.
///
/// Feature-gated behind `tkrzw`. See the module documentation for the
/// table framing and for how `write`'s atomicity differs from redb's
/// and LMDB's.
pub struct TkrzwStore {
    db: UniquePtr<ffi::Db>,
    read_only: bool,
}

fn validate_path(path: &Path) -> Result<(), StoreError> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(StoreError::InvalidInput("path contains NUL"));
    }
    Ok(())
}

fn open(path: &Path, writable: bool) -> Result<TkrzwStore, StoreError> {
    validate_path(path)?;
    let mut status = blank_status();
    let db = ffi::open_db(
        path.as_os_str().as_encoded_bytes(),
        writable,
        !writable,
        &mut status,
    );
    check(status)?;
    if db.is_null() {
        return Err(StoreError::Backend(Box::new(ClosedError)));
    }
    Ok(TkrzwStore {
        db,
        read_only: !writable,
    })
}

impl TkrzwStore {
    fn handle(&self) -> Result<&ffi::Db, StoreError> {
        self.db
            .as_ref()
            .ok_or_else(|| StoreError::Backend(Box::new(ClosedError)))
    }
}

// ── zero-copy record callbacks ────────────────────────────────────

/// Everything [`scan_row`] needs for one walk: the table's framed
/// prefix, the caller's bounds, the visitor, and the slot that carries
/// a failed visit out of the C++ callback — an error cannot unwind
/// through the shim, so it is stored and re-raised after the call.
struct ScanCtx<'a, 'row> {
    prefix: &'a [u8],
    lo: Bound<&'a [u8]>,
    hi: Bound<&'a [u8]>,
    row: &'a mut Row<'row>,
    error: Option<StoreError>,
}

/// The [`ffi::db_scan`] record callback: assembles the borrowed key and
/// value slices, frames out the table prefix, and dispatches to the
/// visitor. The slices borrow tkrzw's record memory and are valid only
/// until this returns; `row` copies whatever it must retain. Returns
/// `false` to stop the walk.
///
/// The pointer contract is the shim's: `key_ptr`/`value_ptr` point at
/// the record being processed, pinned by the `Process` call that reaches
/// this callback, non-null (the shim substitutes an empty literal for a
/// null view), with `key_len`/`value_len` their true sizes.
fn scan_row(
    ctx: usize,
    key_ptr: *const u8,
    key_len: usize,
    value_ptr: *const u8,
    value_len: usize,
) -> bool {
    // SAFETY: the pointer contract above — pinned, non-null, true
    // lengths — is kept by the only caller that can reach this.
    let (key, value) = unsafe {
        (
            std::slice::from_raw_parts(key_ptr, key_len),
            std::slice::from_raw_parts(value_ptr, value_len),
        )
    };
    // SAFETY: `ctx` is the address of the `ScanCtx` local in `scan`,
    // cast to a token. Only this callback receives it, only the one
    // `ffi::db_scan` call that took the token can invoke the callback,
    // and that call returns before `scan` drops its context — so the
    // reference lives for every invocation. Nothing else aliases the
    // context: the walk is single-threaded and reentrant only through
    // `row`, which cannot reach the token.
    let ctx = unsafe { &mut *(ctx as *mut ScanCtx<'_, '_>) };
    // Ascending order means the first key outside the table's run is
    // the end of it.
    let Some(user_key) = key.strip_prefix(ctx.prefix) else {
        return false;
    };
    if past_upper(user_key, ctx.hi) {
        return false;
    }
    if in_bounds(user_key, ctx.lo, ctx.hi) {
        // The visitor runs inside tkrzw's call stack, reached through the
        // shim's C++ frames. cxx's `rust::Fn` bridge is `noexcept` and
        // installs no handler, so a panic escaping `row` would unwind into
        // C++ and on into Rust — undefined behaviour. Catch it here and
        // surface it as an error instead.
        let visited =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (ctx.row)(user_key, value)));
        match visited {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => return false,
            Ok(Err(error)) => {
                ctx.error = Some(error);
                return false;
            }
            Err(_panic) => {
                ctx.error = Some(StoreError::Backend(Box::new(VisitorPanicked)));
                return false;
            }
        }
    }
    true
}

/// The [`ffi::db_get`] record callback: assembles the borrowed value
/// slice and copies it into the caller's slot. This is the single owned
/// copy a point read whose result outlives the call needs; a missing
/// record never invokes it, so the slot staying `None` is the "not
/// found" answer.
///
/// The pointer contract is the shim's, as in [`scan_row`].
fn get_value(ctx: usize, value_ptr: *const u8, value_len: usize) {
    // SAFETY: `value_ptr` is tkrzw's record memory for the record being
    // processed — pinned, non-null, with `value_len` its true size.
    let value = unsafe { std::slice::from_raw_parts(value_ptr, value_len) };
    // SAFETY: `ctx` is the address of the `Option<Vec<u8>>` local in
    // the calling `get`, cast to a token. Only this callback receives
    // it, only the one `ffi::db_get` call that took the token can
    // invoke the callback, and that call returns before the local is
    // read or dropped.
    unsafe {
        *(ctx as *mut Option<Vec<u8>>) = Some(value.to_vec());
    };
}

/// Walks `table`'s records whose key lies in `(lo, hi)`, ascending,
/// handing each to `row` borrowed from tkrzw's record buffer. `row`
/// returns `false` to stop early.
///
/// Every row the walk yields is already inside the bounds, so callers
/// do not re-check them.
fn scan(
    db: &ffi::Db,
    prefix: &[u8],
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
    row: &mut Row<'_>,
) -> Result<(), StoreError> {
    // TreeDBM's Jump is a lower-bound seek, so an Excluded lower bound
    // lands on the excluded key itself and is skipped by scan_row.
    let start = match lo {
        Bound::Unbounded => prefix.to_vec(),
        Bound::Included(key) | Bound::Excluded(key) => framed(prefix, key),
    };
    let mut ctx = ScanCtx {
        prefix,
        lo,
        hi,
        row,
        error: None,
    };
    let token = std::ptr::addr_of_mut!(ctx) as usize;
    check(ffi::db_scan(db, &start, scan_row, token))?;
    match ctx.error.take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

impl ReadStore for TkrzwStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        open(path, false)
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let prefix = table_prefix(table)?;
        let mut slot = None;
        let token = std::ptr::addr_of_mut!(slot) as usize;
        check(ffi::db_get(
            self.handle()?,
            &framed(&prefix, key),
            get_value,
            token,
        ))?;
        Ok(slot)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        self.range(table, Bound::Unbounded, Bound::Unbounded, visit)
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        scan(self.handle()?, &prefix, lo, hi, &mut |key, value| {
            visit(key, value)?;
            Ok(true)
        })
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let prefix = table_prefix(table)?;
        let mut empty = true;
        scan(
            self.handle()?,
            &prefix,
            Bound::Unbounded,
            Bound::Unbounded,
            &mut |_key, _value| {
                empty = false;
                Ok(false)
            },
        )?;
        Ok(empty)
    }
}

impl WriteStore for TkrzwStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        open(path, true)
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let db = self.handle()?;
        let mut txn = TkrzwWriteTxn {
            db,
            buffer: BTreeMap::new(),
        };
        // On `Err` the buffer dies with the transaction and nothing was
        // ever written: rollback is the absence of the apply below.
        let result = f(&mut txn)?;
        let mutations: Vec<ffi::Mutation> = txn
            .buffer
            .into_iter()
            .map(|(key, slot)| match slot {
                Some(value) => ffi::Mutation {
                    key,
                    value,
                    remove: false,
                },
                None => ffi::Mutation {
                    key,
                    value: Vec::new(),
                    remove: true,
                },
            })
            .collect();
        if !mutations.is_empty() {
            check(ffi::db_apply(db, &mutations))?;
            check(ffi::db_synchronize(db, false))?;
        }
        Ok(result)
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        check(ffi::db_rebuild(self.handle()?))?;
        check(ffi::db_synchronize(self.handle()?, false))
    }
}

// ── write transaction ─────────────────────────────────────────────

/// Buffers a write transaction's mutations until the single
/// `ProcessMulti` apply in [`WriteStore::write`].
///
/// Keys are framed (`table || 0x00 || key`), and the map is ordered so
/// the merged scans below yield rows in the same key order the store
/// does. A `None` slot is a pending removal.
struct TkrzwWriteTxn<'db> {
    db: &'db ffi::Db,
    buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl TkrzwWriteTxn<'_> {
    /// The buffered rows of `table` inside `(lo, hi)`, as
    /// `(user key, slot)`.
    fn buffered<'a>(
        &'a self,
        prefix: &'a [u8],
        lo: Bound<&'a [u8]>,
        hi: Bound<&'a [u8]>,
    ) -> impl Iterator<Item = (&'a [u8], &'a Option<Vec<u8>>)> + 'a {
        self.buffer
            .range(prefix.to_vec()..)
            .map_while(move |(key, slot)| key.strip_prefix(prefix).map(|user| (user, slot)))
            .filter(move |(user, _)| in_bounds(user, lo, hi))
    }

    /// Feeds `visit` every live row of `table` inside `(lo, hi)`, with
    /// this transaction's buffer laid over storage. The stored cursor
    /// and the buffered rows are merged in ascending framed-key order as
    /// a stream — a buffered value overrides its stored twin, a buffered
    /// tombstone hides it — so no call materialises the result set.
    fn merged_visit(
        &self,
        prefix: &[u8],
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let mut buffered = self.buffered(prefix, lo, hi).peekable();
        scan(self.db, prefix, lo, hi, &mut |stored_key, stored_value| {
            // Buffered keys below the stored one have no stored twin:
            // the live ones are inserts, the dead ones tombstones.
            while let Some(&(key, slot)) = buffered.peek() {
                match key.cmp(stored_key) {
                    Ordering::Less => {
                        buffered.next();
                        if let Some(value) = slot {
                            visit(key, value)?;
                        }
                    }
                    // Equal key: the buffer overrides storage either way.
                    Ordering::Equal => {
                        buffered.next();
                        if let Some(value) = slot {
                            visit(key, value)?;
                        }
                        return Ok(true);
                    }
                    Ordering::Greater => break,
                }
            }
            visit(stored_key, stored_value)?;
            Ok(true)
        })?;
        // Past the last stored row, whatever buffering remains is all
        // inserts.
        for (key, slot) in buffered {
            if let Some(value) = slot {
                visit(key, value)?;
            }
        }
        Ok(())
    }
}

impl WriteTxn for TkrzwWriteTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let prefix = table_prefix(table)?;
        let framed_key = framed(&prefix, key);
        if let Some(slot) = self.buffer.get(&framed_key) {
            return Ok(slot.clone());
        }
        let mut slot = None;
        let token = std::ptr::addr_of_mut!(slot) as usize;
        check(ffi::db_get(self.db, &framed_key, get_value, token))?;
        Ok(slot)
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        self.buffer
            .insert(framed(&prefix, key), Some(value.to_vec()));
        Ok(())
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        self.buffer.insert(framed(&prefix, key), None);
        Ok(())
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        self.merged_visit(&prefix, lo, hi, visit)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        self.range(table, Bound::Unbounded, Bound::Unbounded, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let prefix = table_prefix(table)?;
        // A buffered value settles it without touching storage.
        if self
            .buffered(&prefix, Bound::Unbounded, Bound::Unbounded)
            .any(|(_, slot)| slot.is_some())
        {
            return Ok(false);
        }
        // Otherwise the first stored row this transaction has not
        // removed does, so the walk stops there rather than counting.
        let mut empty = true;
        scan(
            self.db,
            &prefix,
            Bound::Unbounded,
            Bound::Unbounded,
            &mut |key, _value| {
                if self
                    .buffer
                    .get(&framed(&prefix, key))
                    .is_some_and(Option::is_none)
                {
                    return Ok(true);
                }
                empty = false;
                Ok(false)
            },
        )?;
        Ok(empty)
    }
}
