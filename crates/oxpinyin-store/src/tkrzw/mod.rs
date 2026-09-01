//! tkrzw backend for the store capability tiers, over the plain-C API.
//!
//! Enabled by the `tkrzw` cargo feature. Mirrors libpinyin at `0c5e80e`
//! (`src/storage/tkrzwdb_utils.h`, `chewing_large_table2_tkrzwdb.cpp`,
//! `ngram_tkrzwdb.cpp`): the database is tkrzw's TreeDBM. The binding
//! reaches it through `tkrzw_langc.h` — `tkrzw_dbm_open` with `dbm=tree`
//! selects TreeDBM, and passing no comparator parameter leaves its
//! default `LexicalKeyComparator` in place, so records sort by plain
//! unsigned byte order and oxpinyin's big-endian key codec keeps the
//! ordering it has under redb and LMDB. No C++ header, class, or
//! exception ever crosses the ABI.
//!
//! # Zero-copy reads
//!
//! Reads borrow tkrzw's record memory instead of copying it, the way
//! libpinyin's own tkrzw code does — its `KeyCollectProcessor` walks
//! records through `ProcessEach` reading in-place `string_view`s. Here
//! the C API's callback plumbing does the same job:
//! `tkrzw_dbm_process` drives one record and `tkrzw_dbm_iter_process`
//! drives the walk, handing every key and value to a Rust callback as a
//! `(pointer, size)` pair into tkrzw's record buffer: no intermediary
//! buffer, no per-record allocation. A borrow lasts only for the
//! callback's duration; a visitor that wants to keep a row copies
//! exactly what it retains, and a point `get` makes the one owned copy
//! a result that outlives the call requires. Carrying a borrowed slice
//! past its callback is the one thing the design forbids.
//!
//! # Errors and status
//!
//! The C API is exception-free: every C++ exception is caught inside
//! libtkrzw and reported through a thread-local "last status",
//! retrieved with `tkrzw_get_last_status` and copied immediately — its
//! message region is valid only until the next tkrzw call on the same
//! thread. Statuses map as this backend always has: success to `Ok`,
//! `SYSTEM_ERROR` to [`StoreError::Io`], everything else to a backend
//! error carrying the code and message verbatim. One registered
//! divergence: exceptions caught inside the C wrapper report
//! `SYSTEM_ERROR` — so an allocation failure classifies as
//! [`StoreError::Io`] — where the retired cxx shim reported
//! `UNKNOWN_ERROR` (a backend error); the C ABI cannot distinguish the
//! two `SYSTEM_ERROR` origins. Ruled accepted 2026-08-28; the register
//! entry with the full analysis is
//! `docs/findings/tkrzw-langc-exception-classification.md`.
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
//! `Ok` applies the whole buffer in one `ProcessMulti` batch (through
//! `tkrzw_dbm_process_multi`); on `Err` the buffer is dropped and
//! nothing is written. `ProcessMulti` locks every named record for the
//! duration, so the batch lands as a unit against any other reader or
//! writer.
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
//! tkrzw as bytes. The library must be discoverable via `pkg-config`
//! and must ship `tkrzw_langc.h`; `build.rs` says as much when it is
//! missing.
//!
//! The old C++ binding had to warn against distro tkrzw builds that
//! break the `RecordProcessor::NOOP`/`REMOVE` pointer-identity
//! protocol, because the shim's own processor objects had to match
//! sentinel addresses across the ABI (the bisect is recorded in
//! `docs/findings/store-key-ordering.md`). The C API removes that
//! failure mode by construction: a callback's return value is compared
//! against the `TKRZW_REC_PROC_NOOP`/`TKRZW_REC_PROC_REMOVE` values
//! *inside* libtkrzw, and this module returns those values by reading
//! the library's own globals, so client and library cannot disagree
//! about what a sentinel is.
//!
//! # Unsafe waiver
//!
//! This module and its `ffi` carry an explicit `allow(unsafe_code)`:
//! bindgen's declarations are unsafe by nature, and the callback
//! plumbing needs a handful of hand-written `unsafe` blocks (raw token
//! derefs, documented at each site). That waiver is scoped to the tkrzw
//! backend by decision — the workspace outside it stays `deny` — and it
//! waives safety ceremony, not correctness: the shared read and write
//! suites gate this backend like any other.
#![allow(unsafe_code)]

mod ffi;

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::{c_char, c_void};
use std::fmt;
use std::ops::Bound;
use std::path::Path;
use std::ptr::NonNull;

use crate::{ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

/// `TKRZW_STATUS_SUCCESS` — pinned by number so an upstream renumbering
/// fails the build instead of silently misclassifying statuses.
const STATUS_SUCCESS: i32 = 0;
/// `TKRZW_STATUS_SYSTEM_ERROR` — the codes that came from the operating
/// system, reported as [`StoreError::Io`] so callers can branch on I/O
/// failures the way they do for the other backends.
const STATUS_SYSTEM_ERROR: i32 = 2;
/// `TKRZW_STATUS_NOT_FOUND_ERROR` — how the ordered walk signals that
/// the records ran out, which ends a scan successfully rather than
/// failing it.
const STATUS_NOT_FOUND_ERROR: i32 = 7;

// The C enum in tkrzw_langc.h is the ABI (numbering identical to the
// C++ Status::Code it casts from); the generated constants must agree
// with the pinned numbers above or every mapping below is wrong.
const _: () = assert!(ffi::TKRZW_STATUS_SUCCESS == STATUS_SUCCESS as u32);
const _: () = assert!(ffi::TKRZW_STATUS_SYSTEM_ERROR == STATUS_SYSTEM_ERROR as u32);
const _: () = assert!(ffi::TKRZW_STATUS_NOT_FOUND_ERROR == STATUS_NOT_FOUND_ERROR as u32);

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

/// A scan visitor panicked. The panic is caught at the callback boundary
/// and reported as this error: a visitor runs inside tkrzw's call stack,
/// and an unwind escaping through the C ABI would be undefined
/// behaviour, so it must never be allowed to propagate.
#[derive(Debug)]
struct VisitorPanicked;

impl fmt::Display for VisitorPanicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("store scan visitor panicked")
    }
}

impl std::error::Error for VisitorPanicked {}

/// Reads the thread-local "last status" and copies it out — the
/// message region is valid only until the next tkrzw call on this
/// thread, so this runs immediately after every failed call, before
/// anything else can touch tkrzw. The bytes are not guaranteed to be
/// UTF-8, so they are taken lossily: no input can panic (constitution
/// §4), and a mangled message must not turn into one.
fn last_status() -> (i32, String) {
    // SAFETY: `tkrzw_get_last_status` only reads thread-local state
    // this thread owns and returns the struct by value.
    let status = unsafe { ffi::tkrzw_get_last_status() };
    // SAFETY: the message pointer is either the empty literal or points
    // into libtkrzw's thread-local message buffer, NUL-terminated; the
    // copy below happens before any other tkrzw call on this thread can
    // overwrite that buffer, and `size_of::<c_char>() == 1` makes the
    // length arithmetic exact.
    let message = if status.message.is_null() {
        Vec::new()
    } else {
        unsafe { CStr::from_ptr(status.message) }
            .to_bytes()
            .to_vec()
    };
    (status.code, String::from_utf8_lossy(&message).into_owned())
}

/// Builds the error a failed call has left in the last status, with the
/// shape every backend uses: I/O failures surface as [`StoreError::Io`],
/// other statuses as a backend error carrying the code and message
/// verbatim.
fn status_error() -> StoreError {
    let (code, message) = last_status();
    match code {
        STATUS_SYSTEM_ERROR => StoreError::Io(std::io::Error::other(message)),
        code => StoreError::Backend(Box::new(TkrzwError { code, message })),
    }
}

/// Maps a C call's boolean result: `true` is success, `false` is the
/// status the call left behind.
fn check(ok: bool) -> Result<(), StoreError> {
    if ok { Ok(()) } else { Err(status_error()) }
}

/// Like [`check`], but `TKRZW_STATUS_NOT_FOUND_ERROR` means "the walk
/// ended", so it comes back as `Ok(true)` instead of an error.
fn check_end(ok: bool) -> Result<bool, StoreError> {
    if ok {
        return Ok(false);
    }
    if last_status().0 == STATUS_NOT_FOUND_ERROR {
        return Ok(true);
    }
    Err(status_error())
}

/// The `TKRZW_REC_PROC_NOOP` sentinel, read from the library's own
/// global so the value this module returns is the value libtkrzw
/// compares against — by construction, never by assumption.
fn rec_proc_noop() -> *const c_char {
    // SAFETY: the global is initialised when libtkrzw is loaded and is
    // never written afterwards; reading the pointer value is a plain
    // load of constant data.
    unsafe { std::ptr::addr_of!(ffi::TKRZW_REC_PROC_NOOP).read() }
}

/// The `TKRZW_REC_PROC_REMOVE` sentinel, read as [`rec_proc_noop`] is.
fn rec_proc_remove() -> *const c_char {
    // SAFETY: as `rec_proc_noop` — a load of a constant global.
    unsafe { std::ptr::addr_of!(ffi::TKRZW_REC_PROC_REMOVE).read() }
}

/// A key or value length as the C API's `int32_t`. tkrzw sizes records
/// in `int32_t`, so anything larger cannot be stored anyway; reporting
/// it as invalid input keeps the cast from wrapping into a negative
/// length the C side would reinterpret as "use `strlen`".
fn c_len(bytes: &[u8]) -> Result<i32, StoreError> {
    i32::try_from(bytes.len()).map_err(|_| StoreError::InvalidInput("record too large for tkrzw"))
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

// ── handles ───────────────────────────────────────────────────────

/// An open database, closed and freed exactly once when dropped.
struct Db(NonNull<ffi::TkrzwDBM>);

impl Drop for Db {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `tkrzw_dbm_open`, has not been
        // passed to `tkrzw_dbm_close` before (this Drop is its only
        // call site, and it runs once), and no iterator can still hold
        // a position into it: iterators never escape `scan`, which
        // returns before the store holding this handle can drop.
        // Close's own status is ignored — Drop cannot propagate it, and
        // every write commit has already synchronized explicitly.
        unsafe { ffi::tkrzw_dbm_close(self.0.as_ptr()) };
    }
}

// SAFETY: the handle wraps a PolyDBM, whose documented contract is that
// all operations except Open and Close are thread-safe — the database
// carries its own locking — so a handle can be moved between threads
// and shared by reference (which is also what lets the store keep `get`
// and `write` on `&self`). The two excluded operations cannot race
// here: `tkrzw_dbm_open` completes before the handle exists to share,
// and `tkrzw_dbm_close` runs in Drop, which holds the handle
// exclusively. The last-status channel the error mapping reads is
// thread-local, so calls from other threads cannot interleave with it.
unsafe impl Send for Db {}
unsafe impl Sync for Db {}

/// An iterator over one [`Db`], freed when dropped.
///
/// It exists only as a local of [`scan`]: created, positioned, walked
/// and destroyed before the database handle can go away, so the
/// DBM-outlives-iterator invariant holds by construction. The raw
/// pointer keeps it `!Send`/`!Sync`, which nothing here needs.
struct Iter(NonNull<ffi::TkrzwDBMIter>);

impl Drop for Iter {
    fn drop(&mut self) {
        // SAFETY: `self.0` came from `tkrzw_dbm_make_iterator` on a
        // database this thread still holds open, and has not been
        // passed to `tkrzw_dbm_iter_free` before (this Drop is its only
        // call site, and it runs once).
        unsafe { ffi::tkrzw_dbm_iter_free(self.0.as_ptr()) };
    }
}

// ── store ─────────────────────────────────────────────────────────

/// A tkrzw-backed store implementing both capability tiers.
///
/// Feature-gated behind `tkrzw`. See the module documentation for the
/// table framing and for how `write`'s atomicity differs from redb's
/// and LMDB's.
pub struct TkrzwStore {
    db: Db,
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
    // `dbm=tree` selects TreeDBM; `no_create=true` reproduces
    // OPEN_NO_CREATE, by which a read-only open fails when the file is
    // missing instead of creating it.
    let params = if writable {
        c"dbm=tree"
    } else {
        c"dbm=tree,no_create=true"
    };
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| StoreError::InvalidInput("path contains NUL"))?;
    // SAFETY: both strings outlive the call; the returned handle is
    // NULL on failure, taken as an error immediately — the failure's
    // status is already set by the call.
    let db = unsafe { ffi::tkrzw_dbm_open(path.as_ptr(), writable, params.as_ptr()) };
    let Some(db) = NonNull::new(db) else {
        return Err(status_error());
    };
    Ok(TkrzwStore {
        db: Db(db),
        read_only: !writable,
    })
}

// ── zero-copy record callbacks ────────────────────────────────────

/// Everything [`walk_row`] needs for one walk: the table's framed
/// prefix, the caller's bounds, the visitor, the slot that carries a
/// failed visit out of the C callback — an error cannot unwind through
/// the C ABI, so it is stored and re-raised after the call — and the
/// flag by which the callback stops the walk.
struct ScanCtx<'a, 'row> {
    prefix: &'a [u8],
    lo: Bound<&'a [u8]>,
    hi: Bound<&'a [u8]>,
    row: &'a mut Row<'row>,
    error: Option<StoreError>,
    stop: bool,
}

/// The `tkrzw_dbm_iter_process` record callback: assembles the borrowed
/// key and value slices, frames out the table prefix, and dispatches to
/// the visitor. The slices borrow tkrzw's record memory and are valid
/// only until this returns; `row` copies whatever it must retain.
/// Returning the NOOP sentinel keeps the record untouched; setting
/// `ctx.stop` makes the driver loop end the walk after this record.
///
/// The pointer contract is libtkrzw's: `key_ptr`/`value_ptr` point at
/// the record being processed, pinned by the call that reaches this
/// callback, non-null with `key_len`/`value_len` their true sizes.
unsafe extern "C" fn walk_row(
    arg: *mut c_void,
    key_ptr: *const c_char,
    key_len: i32,
    value_ptr: *const c_char,
    value_len: i32,
    _new_value_size: *mut i32,
) -> *const c_char {
    // SAFETY: the pointer contract above — pinned, non-null, true
    // lengths — is kept by the only caller that can reach this.
    let (key, value) = unsafe {
        (
            std::slice::from_raw_parts(key_ptr.cast::<u8>(), key_len as usize),
            std::slice::from_raw_parts(value_ptr.cast::<u8>(), value_len as usize),
        )
    };
    // SAFETY: `arg` is the address of the `ScanCtx` local in `scan`,
    // passed as the callback argument. Only this callback receives it,
    // only the one `tkrzw_dbm_iter_process` call that took the argument
    // can invoke the callback, and that call returns before `scan`
    // drops its context — so the reference lives for every invocation.
    // Nothing else aliases the context: the walk is single-threaded and
    // reentrant only through `row`, which cannot reach the argument.
    let ctx = unsafe { &mut *(arg.cast::<ScanCtx<'_, '_>>()) };
    // Ascending order means the first key outside the table's run is
    // the end of it.
    let Some(user_key) = key.strip_prefix(ctx.prefix) else {
        ctx.stop = true;
        return rec_proc_noop();
    };
    if past_upper(user_key, ctx.hi) {
        ctx.stop = true;
        return rec_proc_noop();
    }
    if in_bounds(user_key, ctx.lo, ctx.hi) {
        // The visitor runs inside tkrzw's call stack, reached through
        // the C ABI. An unwind escaping `row` would cross that boundary
        // into undefined behaviour (Rust aborts `extern "C"` unwinds,
        // losing the error mapping entirely). Catch it here and surface
        // it as an error instead.
        let visited =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (ctx.row)(user_key, value)));
        match visited {
            Ok(Ok(true)) => {}
            Ok(Ok(false)) => ctx.stop = true,
            Ok(Err(error)) => {
                ctx.error = Some(error);
                ctx.stop = true;
            }
            Err(_panic) => {
                ctx.error = Some(StoreError::Backend(Box::new(VisitorPanicked)));
                ctx.stop = true;
            }
        }
    }
    rec_proc_noop()
}

/// The `tkrzw_dbm_process` record callback for a point read: assembles
/// the borrowed value slice and copies it into the caller's slot. This
/// is the single owned copy a point read whose result outlives the call
/// needs. A missing record invokes the callback with a null value
/// pointer (the header's documented absent marker — `ProcessEmpty` in
/// the C++ underneath), so leaving the slot `None` is the "not found"
/// answer.
///
/// The pointer contract is libtkrzw's, as in [`walk_row`].
unsafe extern "C" fn get_value(
    arg: *mut c_void,
    _key_ptr: *const c_char,
    _key_len: i32,
    value_ptr: *const c_char,
    value_len: i32,
    _new_value_size: *mut i32,
) -> *const c_char {
    if !value_ptr.is_null() {
        // SAFETY: `value_ptr` is tkrzw's record memory for the record
        // being processed — pinned, non-null, with `value_len` its true
        // size.
        let value =
            unsafe { std::slice::from_raw_parts(value_ptr.cast::<u8>(), value_len as usize) };
        // SAFETY: `arg` is the address of the `Option<Vec<u8>>` local
        // in the calling `get`, passed as the callback argument. Only
        // this callback receives it, only the one `tkrzw_dbm_process`
        // call that took the argument can invoke the callback, and that
        // call returns before the local is read or dropped.
        unsafe { *(arg.cast::<Option<Vec<u8>>>()) = Some(value.to_vec()) };
    }
    rec_proc_noop()
}

/// One buffered mutation of a write transaction, as the C batch sees
/// it. `remove` picks between writing `value` and deleting the record;
/// `value` is ignored when `remove` is set. The sizes are the C API's
/// `int32_t` forms, validated once when the batch was built, so the
/// callback that reports them cannot wrap.
struct Mutation {
    key: Vec<u8>,
    key_size: i32,
    value: Vec<u8>,
    value_size: i32,
    remove: bool,
}

/// The `tkrzw_dbm_process_multi` record callback: writes this
/// mutation's value, or removes the record. A removal of a record that
/// exists returns the REMOVE sentinel; a removal of an absent record —
/// signalled by the null `existing_value`, exactly as in
/// [`get_value`] — returns NOOP, matching the redb and LMDB backends'
/// no-op `WriteTxn::remove`. The value this returns is copied by
/// tkrzw before the call completes, so lending `mutation.value`'s
/// pointer is the whole contract.
unsafe extern "C" fn apply_one(
    arg: *mut c_void,
    _key_ptr: *const c_char,
    _key_len: i32,
    existing_value: *const c_char,
    _existing_size: i32,
    new_value_size: *mut i32,
) -> *const c_char {
    // SAFETY: `arg` is the address of a `Mutation` in the caller's
    // batch, passed as the callback argument; the batch outlives the
    // one `tkrzw_dbm_process_multi` call that can invoke this.
    let mutation = unsafe { &*(arg.cast::<Mutation>()) };
    if mutation.remove {
        if existing_value.is_null() {
            rec_proc_noop()
        } else {
            rec_proc_remove()
        }
    } else {
        // SAFETY: `new_value_size` is the out-parameter the C wrapper
        // always passes (how it learns the new value's length), and
        // `mutation.value_size` was validated when the batch was built.
        unsafe { *new_value_size = mutation.value_size };
        mutation.value.as_ptr().cast::<c_char>()
    }
}

// ── engine-level operations ───────────────────────────────────────

/// Reads one record, handing the borrowed value to `get_value` exactly
/// once, or not at all when the key is absent.
fn db_get(db: &Db, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
    let mut slot = None;
    // SAFETY: the handle is open; `key` outlives the call with a true
    // length; the callback argument is `&mut slot`, which outlives the
    // call; writable=false makes this a pure read.
    let ok = unsafe {
        ffi::tkrzw_dbm_process(
            db.0.as_ptr(),
            key.as_ptr().cast::<c_char>(),
            c_len(key)?,
            Some(get_value),
            (&mut slot as *mut Option<Vec<u8>>).cast::<c_void>(),
            false,
        )
    };
    check(ok)?;
    Ok(slot)
}

/// Walks `table`'s records whose key lies in `(lo, hi)`, ascending,
/// handing each to `row` borrowed from tkrzw's record buffer. `row`
/// returns `false` to stop early.
///
/// Every row the walk yields is already inside the bounds, so callers
/// do not re-check them.
fn scan(
    db: &Db,
    prefix: &[u8],
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
    row: &mut Row<'_>,
) -> Result<(), StoreError> {
    // The C iterator's Jump is a lower-bound seek, so an Excluded lower
    // bound lands on the excluded key itself and is skipped by
    // walk_row.
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
        stop: false,
    };
    // SAFETY: the handle is open. The returned iterator is freed by
    // `Iter`'s Drop at the end of this scope, before the database can
    // close, so no iterator can outlive its DBM.
    let iter = unsafe { ffi::tkrzw_dbm_make_iterator(db.0.as_ptr()) };
    let Some(iter) = NonNull::new(iter) else {
        return Err(status_error());
    };
    let iter = Iter(iter);
    // SAFETY: `start` outlives the call with a true length, and the
    // iterator belongs to the open database above.
    check(unsafe {
        ffi::tkrzw_dbm_iter_jump(
            iter.0.as_ptr(),
            start.as_ptr().cast::<c_char>(),
            c_len(&start)?,
        )
    })?;
    loop {
        // SAFETY: the iterator is positioned and open; the callback
        // argument is `&mut ctx`, which outlives this call, and the
        // callback honours the contract documented at `walk_row`.
        // writable=false keeps the walk a pure read.
        let ok = unsafe {
            ffi::tkrzw_dbm_iter_process(
                iter.0.as_ptr(),
                Some(walk_row),
                (&mut ctx as *mut ScanCtx<'_, '_>).cast::<c_void>(),
                false,
            )
        };
        // The records ran out: the walk ended successfully.
        if check_end(ok)? {
            break;
        }
        if ctx.stop {
            break;
        }
        // SAFETY: as above. Next past the last record reports
        // NOT_FOUND_ERROR, which `check_end` turns into a successful
        // end of walk.
        let ok = unsafe { ffi::tkrzw_dbm_iter_next(iter.0.as_ptr()) };
        if check_end(ok)? {
            break;
        }
    }
    match ctx.error.take() {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Applies every buffered mutation in one `ProcessMulti` batch: tkrzw
/// locks all the named records for the duration, so the batch lands as
/// a unit against any other reader or writer of the same database.
///
/// Every mutation's C-side sizes were validated when the batch was
/// built, so the pairs carry them without rewrapping.
fn db_apply(db: &Db, mutations: &[Mutation]) -> Result<(), StoreError> {
    let mut pairs: Vec<ffi::TkrzwKeyProcPair> = mutations
        .iter()
        .map(|mutation| ffi::TkrzwKeyProcPair {
            key_ptr: mutation.key.as_ptr().cast::<c_char>(),
            key_size: mutation.key_size,
            proc_: Some(apply_one),
            // The argument is the mutation this pair was built from; the
            // batch outlives the call, as documented at `apply_one`.
            proc_arg: (mutation as *const Mutation).cast_mut().cast::<c_void>(),
        })
        .collect();
    // The element count crosses the same `int32_t` boundary the key and
    // value lengths do (`c_len`): a batch larger than `i32::MAX` cannot
    // be represented, and a wrapped negative count would surface as a
    // bogus allocation failure inside the C wrapper instead of invalid
    // input here.
    let num_pairs = i32::try_from(pairs.len())
        .map_err(|_| StoreError::InvalidInput("too many buffered mutations for tkrzw"))?;
    // SAFETY: the handle is open and writable (the caller checked
    // `read_only`); `pairs` outlives the call with `num_pairs` its true
    // element count, every `key_ptr` points into a `Mutation` that
    // outlives the call, and the callback honours the contract
    // documented at `apply_one`. writable=true is what applies the
    // batch.
    let ok =
        unsafe { ffi::tkrzw_dbm_process_multi(db.0.as_ptr(), pairs.as_mut_ptr(), num_pairs, true) };
    check(ok)
}

/// Flushes buffered writes to the operating system (`hard=false`).
fn db_synchronize(db: &Db) -> Result<(), StoreError> {
    // SAFETY: the handle is open; the empty params string satisfies the
    // non-null assertion the C wrapper makes; no file processor is
    // wanted, so both its slots are null.
    check(unsafe {
        ffi::tkrzw_dbm_synchronize(
            db.0.as_ptr(),
            false,
            None,
            std::ptr::null_mut(),
            c"".as_ptr(),
        )
    })
}

/// Rebuilds the database file, reclaiming space.
fn db_rebuild(db: &Db) -> Result<(), StoreError> {
    // SAFETY: the handle is open; the empty params string satisfies the
    // non-null assertion the C wrapper makes and selects tkrzw's
    // default rebuild tuning.
    check(unsafe { ffi::tkrzw_dbm_rebuild(db.0.as_ptr(), c"".as_ptr()) })
}

impl crate::RawReadStore for TkrzwStore {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, crate::StoreError> {
        db_get(&self.db, key)
    }
}

impl ReadStore for TkrzwStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        open(path, false)
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let prefix = table_prefix(table)?;
        db_get(&self.db, &framed(&prefix, key))
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
        scan(&self.db, &prefix, lo, hi, &mut |key, value| {
            visit(key, value)?;
            Ok(true)
        })
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let prefix = table_prefix(table)?;
        let mut empty = true;
        scan(
            &self.db,
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
        let db = &self.db;
        let mut txn = TkrzwWriteTxn {
            db,
            buffer: BTreeMap::new(),
        };
        // On `Err` the buffer dies with the transaction and nothing was
        // ever written: rollback is the absence of the apply below.
        let result = f(&mut txn)?;
        let mut mutations: Vec<Mutation> = Vec::with_capacity(txn.buffer.len());
        for (key, slot) in txn.buffer {
            let (value, remove) = match slot {
                Some(value) => (value, false),
                None => (Vec::new(), true),
            };
            mutations.push(Mutation {
                key_size: c_len(&key)?,
                value_size: c_len(&value)?,
                key,
                value,
                remove,
            });
        }
        if !mutations.is_empty() {
            db_apply(db, &mutations)?;
            db_synchronize(db)?;
        }
        Ok(result)
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        db_rebuild(&self.db)?;
        db_synchronize(&self.db)
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
    db: &'db Db,
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
        db_get(self.db, &framed_key)
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
