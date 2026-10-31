//! tkrzw backend for the store capability tiers, over the plain-C API.
//!
//! Enabled by the `tkrzw` cargo feature. Mirrors libpinyin at `0c5e80e`
//! (`src/storage/tkrzwdb_utils.h`, `chewing_large_table2_tkrzwdb.cpp`,
//! `ngram_tkrzwdb.cpp`): the database is tkrzw's `TreeDBM`. The binding
//! reaches it through `tkrzw_langc.h` — `tkrzw_dbm_open` with `dbm=tree`
//! selects `TreeDBM`, and passing no comparator parameter leaves its
//! default `LexicalKeyComparator` in place, so records sort by plain
//! unsigned byte order and oxpinyin's big-endian key codec keeps the
//! ordering the byte-order contract demands. No C++ header, class, or
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
//! A `TreeDBM` file is a single flat keyspace, while [`ReadStore`] and
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
//! Every commit then calls `Synchronize(hard=false)`: on a file-backed
//! store buffered data is flushed to the operating system, so once the
//! call returns the file is consistent and visible to any reader, and
//! the write survives a process crash. It is not yet on the device.
//! [`WriteStore::compact`] is the hard sync (`Synchronize(hard=true)`),
//! the one point at which a file-backed store reaches stable storage.
//!
//! **That split is measured, not assumed.** `hard=true` on every commit
//! costs +1.18–1.85 ms per commit here, because `TreeDBM` is mmap-backed
//! and the sync is two `msync(MS_SYNC)` calls over the whole mapped
//! region (~539 KiB) however few records the batch touched — a 13–31×
//! per-commit regression on a path the C ABI runs synchronously from a
//! keystroke. `docs/findings/perf-train-commit-fsync-2026-09-09.md`
//! carries the measurement and the decision.
//!
//! `compact` is no longer where `pinyin_save` reaches storage, though.
//! `UserStore::save` still calls it, but since the libpinyin session
//! moved into memory (#621) the store it compacts is the `BabyDBM` of
//! [`WriteStore::create_in_memory`], which has no file: a session's
//! commits stay in process memory until a save, and that store's
//! rebuild and synchronize have nothing to act on. The profile
//! `pinyin_save` writes goes out through `write_user_index` and the
//! user bigram's `save_db`, which synchronize soft (`hard=false`)
//! before closing — the level the pin's own `save_db` uses
//! (`chewing_large_table2_tkrzwdb.cpp:126`,
//! `phrase_large_table3_tkrzwdb.cpp:117`, `ngram_tkrzwdb.cpp:80` at
//! `074a2219`).
//!
//! What `TreeDBM` cannot give at any sync level is crash-*atomic*
//! application: it has no write-ahead log, so a crash *during* the
//! `ProcessMulti` apply can leave part of a batch on disk.
//! This backend can tear mid-batch. That residual is the documented
//! contract difference — recorded here rather than papered over,
//! because closing it would need a WAL tkrzw does not offer without
//! changing the file format libpinyin installs must share.
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
//! What the C API cannot remove is a libtkrzw that disagrees with
//! *itself*. Past the wrapper's comparison the sentinel is the C++
//! `string_view` again, recognised by the address of its backing bytes,
//! and a library built with GCC's LTO before tkrzw 1.0.34 holds several
//! copies of those bytes (Ubuntu's 1.0.27 and 1.0.32 packages; a source
//! build under Arch's default flags). Against such a library this
//! backend opens and writes without an error and is wrong three ways:
//! removing an absent record stores one, removing a stored record
//! leaves it in place — each carrying the sentinel's five bytes as its
//! value — and on the two file classes the rebuild behind
//! [`WriteStore::compact`] fails with status 6 (`CANCELED_ERROR`).
//! The in-memory container of [`WriteStore::create_in_memory`] shares
//! the first two and not the third: `BabyDBM`'s rebuild does nothing,
//! so compacting a libpinyin session cannot fail this way, and what is
//! left there is the two removal faults, whose writes report success.
//! tkrzw 1.0.34 backs each sentinel with one named array and is sound
//! under the same flags; the C API's two values and its callback
//! contract are the same before and after. Nothing here checks for the
//! fault at open. The store suite's
//! `tkrzw_library_honours_its_sentinel_protocol` names it, and
//! `docs/findings/tkrzw-distro-compat.md` carries the measurements.
//!
//! # Unsafe waiver
//!
//! This module and its `ffi` each carry an explicit
//! `expect(unsafe_code)`: bindgen's declarations are unsafe by nature,
//! and the callback plumbing needs a handful of hand-written `unsafe`
//! blocks (raw token derefs, documented at each site). This module
//! needs its own, unlike the Kyoto Cabinet backend's safe wrapper — the
//! `unsafe extern "C"` record callbacks (`walk_row`, `get_value`,
//! `apply_one`) and the `unsafe impl Send`/`Sync` for `Db` live here,
//! not in `ffi`. That waiver is scoped to the tkrzw
//! backend by decision — the workspace outside it stays `deny` — and it
//! waives safety ceremony, not correctness: the shared read and write
//! suites gate this backend like any other.
#![expect(
    unsafe_code,
    reason = "FFI shim over libtkrzw; every block carries a SAFETY comment"
)]

mod ffi;

use std::cell::Cell;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::{c_char, c_void};
use std::ops::Bound;
use std::path::Path;
use std::ptr::NonNull;

use crate::common::{fill_buffered, framed, in_bounds, table_prefix, validate_path};
use crate::{Flow, ReadStore, StoreError, WriteStore, WriteTxn};

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
// with the pinned numbers above or every mapping below is wrong. bindgen
// types the enum's constants as the target's C enum type (i32 on MSVC,
// u32 elsewhere), so both sides widen to i64.
const _: () = assert!(ffi::TKRZW_STATUS_SUCCESS as i64 == STATUS_SUCCESS as i64);
const _: () = assert!(ffi::TKRZW_STATUS_SYSTEM_ERROR as i64 == STATUS_SYSTEM_ERROR as i64);
const _: () = assert!(ffi::TKRZW_STATUS_NOT_FOUND_ERROR as i64 == STATUS_NOT_FOUND_ERROR as i64);

// ── errors ────────────────────────────────────────────────────────

/// The text of a non-I/O tkrzw status, carrying the code and message
/// verbatim.
fn status_message(code: i32, message: &str) -> String {
    if message.is_empty() {
        format!("tkrzw status {code}")
    } else {
        format!("tkrzw status {code}: {message}")
    }
}

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
    let message = if status.message.is_null() {
        Vec::new()
    } else {
        // SAFETY: the message pointer is non-null (checked above) and
        // points into libtkrzw's thread-local NUL-terminated buffer; the
        // copy below happens before any other tkrzw call on this thread
        // can overwrite that buffer.
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
        code => StoreError::Backend(crate::BackendError::with_code(
            code,
            status_message(code, &message),
        )),
    }
}

/// Maps a C call's boolean result: `true` is success, `false` is the
/// status the call left behind.
fn check(ok: bool) -> Result<(), StoreError> {
    if ok { Ok(()) } else { Err(status_error()) }
}

/// Like [`check`], but `TKRZW_STATUS_NOT_FOUND_ERROR` means "the walk
/// ended" (or, for an existence check, "no such record"), so it comes
/// back as `Ok(true)` instead of an error. The code alone decides; the
/// message is copied only when the status is a failure.
fn check_end(ok: bool) -> Result<bool, StoreError> {
    if ok {
        return Ok(false);
    }
    // SAFETY: `tkrzw_get_last_status_code` only reads thread-local state
    // this thread owns; it leaves the message region untouched, so
    // `status_error` below still reads this call's message.
    if unsafe { ffi::tkrzw_get_last_status_code() } == STATUS_NOT_FOUND_ERROR {
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
// SAFETY: same argument as Send — PolyDBM operations (other than Open/Close,
// which cannot race here) are internally synchronised.
unsafe impl Sync for Db {}

/// An iterator over one [`Db`], freed when dropped.
///
/// It exists only as a local of [`scan_rows`]: created, positioned, walked
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
/// table framing and for how `write`'s atomicity is weaker than a
/// write-ahead-log backend's.
pub struct TkrzwStore {
    db: Db,
    read_only: bool,
}

fn open_hash(path: &Path) -> Result<TkrzwStore, StoreError> {
    validate_path(path)?;
    let params = c"dbm=hash,no_create=true";
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| StoreError::InvalidInput("path contains NUL"))?;
    // SAFETY: both strings outlive the call; the returned handle is
    // NULL on failure, taken as an error immediately.
    let db = unsafe { ffi::tkrzw_dbm_open(path.as_ptr(), false, params.as_ptr()) };
    let Some(db) = NonNull::new(db) else {
        return Err(status_error());
    };
    Ok(TkrzwStore {
        db: Db(db),
        read_only: true,
    })
}

fn open_hash_create(path: &Path) -> Result<TkrzwStore, StoreError> {
    validate_path(path)?;
    let params = c"dbm=hash";
    let path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| StoreError::InvalidInput("path contains NUL"))?;
    // SAFETY: both strings outlive the call; the returned handle is
    // NULL on failure, taken as an error immediately.
    let db = unsafe { ffi::tkrzw_dbm_open(path.as_ptr(), true, params.as_ptr()) };
    let Some(db) = NonNull::new(db) else {
        return Err(status_error());
    };
    Ok(TkrzwStore {
        db: Db(db),
        read_only: false,
    })
}

/// `BabyDBM`, tkrzw's on-memory ordered container, through PolyDBM's
/// empty-path form (the same form the user bigram's `TinyDBM` takes
/// below): ordered like `TreeDBM`, so the table framing's prefix walks
/// and `jump`s hold, and bound to no file.
fn open_in_memory() -> Result<TkrzwStore, StoreError> {
    // SAFETY: both strings outlive the call; the returned handle is
    // NULL on failure, taken as an error immediately.
    let db = unsafe { ffi::tkrzw_dbm_open(c"".as_ptr(), true, c"dbm=baby".as_ptr()) };
    let Some(db) = NonNull::new(db) else {
        return Err(status_error());
    };
    Ok(TkrzwStore {
        db: Db(db),
        read_only: false,
    })
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

/// What a walk and its [`walk_row`] callback share, none of it generic:
/// the table's framed prefix, the caller's bounds, the slot that carries
/// a failed visit out of the C callback — an error cannot unwind through
/// the C ABI, so it is stored and re-raised after the call — and the flag
/// by which the callback stops the walk. The callback writes the slot and
/// the flag; [`drive_scan`] reads them between records.
struct ScanShared<'a> {
    prefix: &'a [u8],
    lo: Bound<&'a [u8]>,
    hi: Bound<&'a [u8]>,
    error: Cell<Option<StoreError>>,
    stop: Cell<bool>,
}

/// The callback argument [`walk_row`] receives: the shared walk state and
/// the visitor. Only the visitor's type is generic, so [`walk_row`] is the
/// one function that is instantiated per visitor; the driver loop that
/// calls it is [`drive_scan`], shared by every visitor.
struct ScanCtx<'a, F> {
    shared: &'a ScanShared<'a>,
    row: F,
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
unsafe extern "C" fn walk_row<F>(
    arg: *mut c_void,
    key_ptr: *const c_char,
    key_len: i32,
    value_ptr: *const c_char,
    value_len: i32,
    _new_value_size: *mut i32,
) -> *const c_char
where
    F: FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
{
    // SAFETY: the pointer contract above — pinned, non-null, true
    // lengths — is kept by the only caller that can reach this.
    let (key, value) = unsafe {
        (
            std::slice::from_raw_parts(key_ptr.cast::<u8>(), usize::try_from(key_len).unwrap_or(0)),
            std::slice::from_raw_parts(
                value_ptr.cast::<u8>(),
                usize::try_from(value_len).unwrap_or(0),
            ),
        )
    };
    // SAFETY: `arg` is the address of the `ScanCtx<F>` local in `scan_rows`,
    // passed as the callback argument together with `walk_row::<F>`, so
    // the cast names the type it was made from. Only this callback receives it,
    // only the one `tkrzw_dbm_iter_process` call that took the argument
    // can invoke the callback, and that call returns before `scan_rows`
    // drops its context — so the reference lives for every invocation.
    // Nothing else aliases the context: the walk is single-threaded and
    // reentrant only through `row`, which cannot reach the argument. The
    // shared state it points at is only ever touched through `Cell`s and
    // shared references.
    let ctx = unsafe { &mut *(arg.cast::<ScanCtx<'_, F>>()) };
    let shared = ctx.shared;
    // Ascending order means the first key outside the table's run is
    // the end of it.
    let Some(user_key) = key.strip_prefix(shared.prefix) else {
        shared.stop.set(true);
        return rec_proc_noop();
    };
    if past_upper(user_key, shared.hi) {
        shared.stop.set(true);
        return rec_proc_noop();
    }
    if in_bounds(user_key, shared.lo, shared.hi) {
        // The visitor runs inside tkrzw's call stack, reached through the
        // C ABI. Store errors surface through `shared.error` and stop the
        // walk. Visitor failure paths follow constitution §4; the lint
        // denials alone do not prove panic freedom. A panic in `row`
        // reaches this callback's `extern "C"` boundary,
        // where Rust aborts the process; it is never allowed to run on
        // into tkrzw's frames.
        match (ctx.row)(user_key, value) {
            Ok(Flow::Continue(())) => {}
            Ok(Flow::Break(())) => shared.stop.set(true),
            Err(error) => {
                shared.error.set(Some(error));
                shared.stop.set(true);
            }
        }
    }
    rec_proc_noop()
}

/// Where a point read lands: the caller's buffer, cleared before the
/// call, and whether a record was found.
struct Fill<'a> {
    value: &'a mut Vec<u8>,
    found: bool,
}

/// The `tkrzw_dbm_process` record callback for a point read: assembles
/// the borrowed value slice and copies it into the caller's buffer. This
/// is the single copy a point read whose result outlives the call needs.
/// A missing record invokes the callback with a null value pointer (the
/// header's documented absent marker — `ProcessEmpty` in the C++
/// underneath), so leaving `found` false is the "not found" answer.
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
        let value = unsafe {
            std::slice::from_raw_parts(
                value_ptr.cast::<u8>(),
                usize::try_from(value_len).unwrap_or(0),
            )
        };
        // SAFETY: `arg` is the address of the `Fill` local in the
        // calling `db_get`, passed as the callback argument. Only this
        // callback receives it, only the one `tkrzw_dbm_process` call that
        // took the argument can invoke the callback, and that call returns
        // before the local is read or dropped.
        let fill = unsafe { &mut *(arg.cast::<Fill<'_>>()) };
        fill.value.reserve_exact(value.len());
        fill.value.extend_from_slice(value);
        fill.found = true;
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
/// [`get_value`] — returns NOOP, matching the trait contract's
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

/// The record count — `tkrzw_dbm_count`, which returns it or -1 on
/// failure (the failure reason lands in the thread-local last status,
/// like every other call).
fn db_count(db: &Db) -> Result<u64, StoreError> {
    // SAFETY: the handle is open; the call only reads the database's
    // record counter and touches no borrowed buffers.
    let count = unsafe { ffi::tkrzw_dbm_count(db.0.as_ptr()) };
    if count >= 0 {
        return Ok(u64::try_from(count).unwrap_or(0));
    }
    Err(status_error())
}

/// Whether the hash file open as `db` at `path` holds no record, so that
/// the pin's iterator over it finds none: it is healthy, its header
/// counts no records, and the file on disk ends at or before its record
/// section. The last condition does not rely on the header's count, so
/// a file with record bytes is walked even when its header reads healthy
/// with 0 records. Anything that cannot be read answers no.
fn holds_no_record(db: &Db, path: &Path) -> bool {
    is_healthy(db) && matches!(db_count(db), Ok(0)) && ends_before_records(path)
}

/// `tkrzw_dbm_is_healthy`: whether the open file was closed cleanly and
/// its size agrees with its metadata (`HashDBMImpl::OpenImpl`).
fn is_healthy(db: &Db) -> bool {
    // SAFETY: the handle is open; the call only reads the database's
    // health flag and touches no borrowed buffers.
    unsafe { ffi::tkrzw_dbm_is_healthy(db.0.as_ptr()) }
}

/// The size of a HashDBM file's metadata, its first bytes
/// (`METADATA_SIZE`, `tkrzw_dbm_hash.cc:33` at tkrzw 9ee8416).
const HASH_METADATA_SIZE: usize = 128;

/// Whether the file at `path`, by `fstat` of its open descriptor, ends at
/// or before the record section its own header places.
fn ends_before_records(path: &Path) -> bool {
    use std::io::Read as _;
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(metadata) = file.metadata() else {
        return false;
    };
    let mut header = [0_u8; HASH_METADATA_SIZE];
    if file.read_exact(&mut header).is_err() {
        return false;
    }
    record_base(&header).is_some_and(|base| metadata.len() <= base)
}

/// The offset of a HashDBM file's record section, computed from the
/// three header fields that place it as `HashDBMImpl::SetRecordBase`
/// computes it (`tkrzw_dbm_hash.cc:1351-1356` at tkrzw 9ee8416): the
/// metadata, one `offset_width`-byte slot per bucket, the 1,008-byte free
/// block pool and the 16-byte record-base header, rounded up to a
/// multiple of `max(4096, 1 << align_pow)` (`:50-57`, `AlignNumber`).
/// `HashDBM::ReadMetadata` reads the fields as big-endian numbers:
/// `offset_width` and `align_pow` one byte each at 13 and 14, and
/// `num_buckets` eight bytes at 16 (`:39-42`, `:2676-2679`). `None` when
/// the result does not fit in 64 bits.
fn record_base(header: &[u8; HASH_METADATA_SIZE]) -> Option<u64> {
    const FBP_SECTION_SIZE: u64 = 1008;
    const RECORD_BASE_HEADER_SIZE: u64 = 16;
    const RECORD_BASE_ALIGN: u64 = 4096;
    let offset_width = u64::from(header[13]);
    let align_pow = u32::from(header[14]);
    let mut num_buckets = [0_u8; 8];
    num_buckets.copy_from_slice(&header[16..24]);
    let num_buckets = u64::from_be_bytes(num_buckets);
    let align = RECORD_BASE_ALIGN.max(1_u64.checked_shl(align_pow)?);
    num_buckets
        .checked_mul(offset_width)?
        .checked_add(HASH_METADATA_SIZE as u64 + FBP_SECTION_SIZE + RECORD_BASE_HEADER_SIZE)?
        .checked_next_multiple_of(align)
}

/// Reads one record into `value`, which is cleared first, handing the
/// borrowed value to `get_value` exactly once, or not at all when the key
/// is absent.
fn db_get(db: &Db, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
    value.clear();
    let mut fill = Fill {
        value,
        found: false,
    };
    // SAFETY: the handle is open; `key` outlives the call with a true
    // length; the callback argument is `&mut fill`, which outlives the
    // call; writable=false makes this a pure read.
    let ok = unsafe {
        ffi::tkrzw_dbm_process(
            db.0.as_ptr(),
            key.as_ptr().cast::<c_char>(),
            c_len(key)?,
            Some(get_value),
            (&raw mut fill).cast::<c_void>(),
            false,
        )
    };
    check(ok)?;
    Ok(fill.found)
}

/// Whether `key` exists, an empty value included, copying nothing:
/// `tkrzw_dbm_check`, which is `Get(key)` with no value string
/// (`tkrzw_langc.cc:608-620` at tkrzw 9ee8416) — the call libpinyin's
/// suggestion walk opens with, `m_db->Get(query_key, nullptr)`
/// (`phrase_large_table3_tkrzwdb.cpp:166` at 074a2219). A miss leaves
/// `NOT_FOUND_ERROR` in the last status; any other status is a failure.
fn db_check(db: &Db, key: &[u8]) -> Result<bool, StoreError> {
    // SAFETY: the handle is open; `key` outlives the call with a true
    // length; nothing is copied out.
    let ok =
        unsafe { ffi::tkrzw_dbm_check(db.0.as_ptr(), key.as_ptr().cast::<c_char>(), c_len(key)?) };
    check_end(ok).map(|missing| !missing)
}

/// Walks `table`'s records whose key lies in `(lo, hi)`, ascending,
/// handing each to `row` borrowed from tkrzw's record buffer. `row`
/// returns [`Flow::Break`] to stop early.
///
/// Every row the walk yields is already inside the bounds, so callers
/// do not re-check them.
///
/// Only this wrapper and [`walk_row`] are instantiated per visitor; the
/// iterator, the positioning and the record loop are [`drive_scan`], one
/// copy for every call site.
fn scan_rows<F>(
    db: &Db,
    prefix: &[u8],
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
    row: F,
) -> Result<(), StoreError>
where
    F: FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
{
    let shared = ScanShared {
        prefix,
        lo,
        hi,
        error: Cell::new(None),
        stop: Cell::new(false),
    };
    let mut ctx = ScanCtx {
        shared: &shared,
        row,
    };
    drive_scan(
        db,
        &shared,
        Some(walk_row::<F>),
        (&raw mut ctx).cast::<c_void>(),
    )
}

/// The iterator and record loop of [`scan_rows`], for the callback `walk`
/// and its argument `arg`, which the caller pairs: `arg` is the address of
/// the `ScanCtx<F>` that `walk` is `walk_row::<F>` for, and it outlives
/// this call.
#[inline(never)]
fn drive_scan(
    db: &Db,
    shared: &ScanShared<'_>,
    walk: ffi::tkrzw_record_processor,
    arg: *mut c_void,
) -> Result<(), StoreError> {
    let prefix = shared.prefix;
    let lo = shared.lo;
    // The C iterator's Jump is a lower-bound seek, so an Excluded lower
    // bound lands on the excluded key itself and is skipped by
    // walk_row.
    let start = match lo {
        Bound::Unbounded => prefix.to_vec(),
        Bound::Included(key) | Bound::Excluded(key) => framed(prefix, key),
    };
    // SAFETY: the handle is open. The returned iterator is freed by
    // `Iter`'s Drop at the end of this scope, before the database can
    // close, so no iterator can outlive its DBM.
    let iter = unsafe { ffi::tkrzw_dbm_make_iterator(db.0.as_ptr()) };
    let Some(iter) = NonNull::new(iter) else {
        return Err(status_error());
    };
    let iter = Iter(iter);
    // An empty start is the unbounded walk's first record: the no-key
    // iter-first form. A key jump with the empty string is not
    // equivalent on the hash containers — tkrzw's HashDBM answers
    // NOT_FOUND for `Jump("")` on a file that has records — and a
    // *bounded* jump answers NOT_FOUND for a non-empty start when no
    // record meets the lower bound (an empty database, or one whose keys
    // all fall before it), which is an empty scan, not an error.
    // SAFETY: the iterator belongs to the open database above, and
    // `start` outlives the call with a true length.
    let positioned = unsafe {
        if start.is_empty() {
            ffi::tkrzw_dbm_iter_first(iter.0.as_ptr())
        } else {
            ffi::tkrzw_dbm_iter_jump(
                iter.0.as_ptr(),
                start.as_ptr().cast::<c_char>(),
                c_len(&start)?,
            )
        }
    };
    if !positioned {
        if last_status().0 == STATUS_NOT_FOUND_ERROR {
            return Ok(());
        }
        return Err(status_error());
    }
    loop {
        // SAFETY: the iterator is positioned and open; `walk` and `arg`
        // are the pair `scan_rows` made, the argument outliving this call,
        // and the callback honours the contract documented at `walk_row`.
        // writable=false keeps the walk a pure read.
        let ok = unsafe { ffi::tkrzw_dbm_iter_process(iter.0.as_ptr(), walk, arg, false) };
        // The records ran out: the walk ended successfully.
        if check_end(ok)? {
            break;
        }
        if shared.stop.get() {
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
    shared.error.take().map_or(Ok(()), Err)
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

/// Flushes buffered writes.
///
/// `hard = false` hands the bytes to the operating system: the file is
/// consistent and visible to every reader, and the write outlives a
/// process crash. `hard = true` additionally puts them on the device,
/// surviving power loss. Commits take the former and
/// [`WriteStore::compact`] the latter — see the module docs for why the
/// split is where it is.
fn db_synchronize(db: &Db, hard: bool) -> Result<(), StoreError> {
    // SAFETY: the handle is open; the empty params string satisfies the
    // non-null assertion the C wrapper makes; no file processor is
    // wanted, so both its slots are null.
    check(unsafe {
        ffi::tkrzw_dbm_synchronize(
            db.0.as_ptr(),
            hard,
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

/// The key collector [`process_each_keys`] hands to tkrzw. tkrzw calls the
/// callback once before and once after the walk with a null key; those
/// calls carry no record.
struct KeySink<'a> {
    keys: &'a mut Vec<Vec<u8>>,
}

/// The `tkrzw_dbm_process_each` callback: copies the record's key.
///
/// # Safety
///
/// Called only by tkrzw through the one `tkrzw_dbm_process_each` call in
/// [`process_each_keys`], with `arg` the exclusive `*mut KeySink` that call
/// passed, and key bytes valid for the callback.
unsafe extern "C" fn collect_key(
    arg: *mut c_void,
    key_ptr: *const c_char,
    key_len: i32,
    _value_ptr: *const c_char,
    _value_len: i32,
    _new_value_size: *mut i32,
) -> *const c_char {
    if !key_ptr.is_null() {
        // SAFETY: a non-null key pointer from tkrzw's callback contract
        // points at `key_len` readable bytes for this call.
        let key = unsafe {
            std::slice::from_raw_parts(key_ptr.cast::<u8>(), usize::try_from(key_len).unwrap_or(0))
        };
        // SAFETY: `arg` is the address of the `KeySink` local in
        // `process_each_keys`, passed only to this callback and outliving
        // the call; no other reference to it exists during the walk.
        let sink = unsafe { &mut *(arg.cast::<KeySink<'_>>()) };
        sink.keys.push(key.to_vec());
    }
    // NOOP keeps the record untouched.
    rec_proc_noop()
}

/// Every raw key in `DBM::ProcessEach` order — tkrzw's
/// `tkrzw_dbm_process_each`, which visits hash buckets in an order of its
/// own that is not the iterator's.
fn process_each_keys(db: &Db) -> Result<Vec<Vec<u8>>, StoreError> {
    let mut keys = Vec::new();
    {
        let mut sink = KeySink { keys: &mut keys };
        // SAFETY: the database is open; `collect_key` and `sink` are the
        // pair that call takes, the argument outliving the call; the walk
        // is read-only (`writable=false`), so the callback's NOOP answer
        // is never applied.
        let ok = unsafe {
            ffi::tkrzw_dbm_process_each(
                db.0.as_ptr(),
                Some(collect_key),
                (&raw mut sink).cast::<c_void>(),
                false,
            )
        };
        check(ok)?;
    }
    Ok(keys)
}

impl crate::RawReadStore for TkrzwStore {
    fn get_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        db_get(&self.db, key, value)
    }

    fn contains_raw(&self, key: &[u8]) -> Result<bool, StoreError> {
        db_check(&self.db, key)
    }

    fn scan_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        // `scan_rows` over the file's bare keyspace — an empty prefix
        // leaves every stored key verbatim, the raw counterpart of the
        // framed `scan` walk.
        scan_rows(&self.db, &[], lo, hi, visit)
    }

    /// `Bigram::get_all_items`' `ProcessEach` (`ngram_tkrzwdb.cpp:143-172`):
    /// the hash container's bucket order, which is **not** the iterator
    /// order [`RawReadStore::range_raw`] gives.
    fn native_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        process_each_keys(&self.db)
    }

    /// The library's own record count — over the whole file, which on
    /// this backend is exactly the raw keyspace.
    fn count_raw(&self) -> Result<u64, StoreError> {
        db_count(&self.db)
    }

    fn open_hash_read_only(path: &Path) -> Result<Self, crate::StoreError> {
        open_hash(path)
    }
}

impl ReadStore for TkrzwStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        open(path, false)
    }

    fn get(&self, table: &str, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        let prefix = table_prefix(table)?;
        db_get(&self.db, &framed(&prefix, key), value)
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        scan_rows(&self.db, &prefix, lo, hi, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let prefix = table_prefix(table)?;
        let mut empty = true;
        scan_rows(
            &self.db,
            &prefix,
            Bound::Unbounded,
            Bound::Unbounded,
            |_key, _value| {
                empty = false;
                Ok(Flow::Break(()))
            },
        )?;
        Ok(empty)
    }
}

impl WriteStore for TkrzwStore {
    type Txn<'a> = TkrzwTxn<'a>;

    fn create(path: &Path) -> Result<Self, StoreError> {
        open(path, true)
    }

    fn create_hash(path: &Path) -> Result<Self, StoreError> {
        open_hash_create(path)
    }

    fn create_in_memory() -> Result<Self, StoreError> {
        open_in_memory()
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut Self::Txn<'_>) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let db = &self.db;
        let mut txn = TkrzwTxn {
            db,
            buffer: BTreeMap::new(),
        };
        // On `Err` the buffer dies with the transaction and nothing was
        // ever written: rollback is the absence of the apply below.
        let result = f(&mut txn)?;
        commit(db, txn.buffer)?;
        Ok(result)
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        db_rebuild(&self.db)?;
        // A file-backed store's one stable-storage point. It is no
        // longer where `pinyin_save` reaches storage: `UserStore::save`
        // still calls this, but on the in-memory session store (#621),
        // where the rebuild and the synchronize have nothing to do.
        db_synchronize(&self.db, true)
    }
}

// ── write transaction ─────────────────────────────────────────────

/// Applies a finished transaction's buffer in one batch. Not generic, so
/// the many `write` closures share one copy of it.
#[inline(never)]
fn commit(db: &Db, buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>) -> Result<(), StoreError> {
    let mut mutations: Vec<Mutation> = Vec::with_capacity(buffer.len());
    for (key, slot) in buffer {
        let (value, remove) = slot.map_or((Vec::new(), true), |value| (value, false));
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
        // Soft: on a file-backed store the commit is visible and
        // process-crash durable, and `compact` is its hard sync. The
        // in-memory session store has no file for either level.
        db_synchronize(db, false)?;
    }
    Ok(())
}

/// Buffers a write transaction's mutations until the single
/// `ProcessMulti` apply in [`WriteStore::write`].
///
/// Keys are framed (`table || 0x00 || key`), and the map is ordered so
/// the merged scans below yield rows in the same key order the store
/// does. A `None` slot is a pending removal.
pub struct TkrzwTxn<'db> {
    db: &'db Db,
    buffer: BTreeMap<Vec<u8>, Option<Vec<u8>>>,
}

impl TkrzwTxn<'_> {
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
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        let mut buffered = self.buffered(prefix, lo, hi).peekable();
        let mut stopped = false;
        scan_rows(self.db, prefix, lo, hi, |stored_key, stored_value| {
            // Buffered keys below the stored one have no stored twin:
            // the live ones are inserts, the dead ones tombstones.
            while let Some(&(key, slot)) = buffered.peek() {
                match key.cmp(stored_key) {
                    Ordering::Less => {
                        buffered.next();
                        if let Some(value) = slot
                            && visit(key, value)?.is_break()
                        {
                            stopped = true;
                            return Ok(Flow::Break(()));
                        }
                    }
                    // Equal key: the buffer overrides storage either way.
                    Ordering::Equal => {
                        buffered.next();
                        if let Some(value) = slot
                            && visit(key, value)?.is_break()
                        {
                            stopped = true;
                            return Ok(Flow::Break(()));
                        }
                        return Ok(Flow::Continue(()));
                    }
                    Ordering::Greater => break,
                }
            }
            let flow = visit(stored_key, stored_value)?;
            stopped = flow.is_break();
            Ok(flow)
        })?;
        if stopped {
            return Ok(());
        }
        // Past the last stored row, whatever buffering remains is all
        // inserts.
        for (key, slot) in buffered {
            if let Some(value) = slot
                && visit(key, value)?.is_break()
            {
                break;
            }
        }
        Ok(())
    }
}

impl WriteTxn for TkrzwTxn<'_> {
    // The point operations stay out of line: the user store calls them from
    // hundreds of sites, and inlining each one's validation, framing and
    // buffer insert (or FFI call) into every caller costs far more code
    // than a direct call. Dispatch is static either way.
    #[inline(never)]
    fn get(&self, table: &str, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        let prefix = table_prefix(table)?;
        let framed_key = framed(&prefix, key);
        if let Some(slot) = self.buffer.get(&framed_key) {
            return Ok(fill_buffered(slot.as_deref(), value));
        }
        db_get(self.db, &framed_key, value)
    }

    #[inline(never)]
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        self.buffer
            .insert(framed(&prefix, key), Some(value.to_vec()));
        Ok(())
    }

    #[inline(never)]
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        // The file's bare keyspace — no framing prefix, so the row is
        // exactly what `get_raw`/`range_raw` read back on this backend.
        self.buffer.insert(key.to_vec(), Some(value.to_vec()));
        Ok(())
    }

    #[inline(never)]
    fn remove_raw(&mut self, key: &[u8]) -> Result<(), StoreError> {
        // The removal half of `put_raw`, on the same bare keyspace.
        self.buffer.insert(key.to_vec(), None);
        Ok(())
    }

    #[inline(never)]
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        self.buffer.insert(framed(&prefix, key), None);
        Ok(())
    }

    fn scan(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        let prefix = table_prefix(table)?;
        self.merged_visit(&prefix, lo, hi, visit)
    }

    #[inline(never)]
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
        scan_rows(
            self.db,
            &prefix,
            Bound::Unbounded,
            Bound::Unbounded,
            |key, _value| {
                if self
                    .buffer
                    .get(&framed(&prefix, key))
                    .is_some_and(Option::is_none)
                {
                    return Ok(Flow::Continue(()));
                }
                empty = false;
                Ok(Flow::Break(()))
            },
        )?;
        Ok(empty)
    }
}

// ── the user bigram's in-memory container ─────────────────────────

/// libpinyin's tkrzw user bigram as `Bigram::load_db` leaves it: an
/// in-memory `TinyDBM` (`ngram_tkrzwdb.cpp:48-63`) at its default bucket
/// count, filled from the `HashDBM` file by `copy_tkrzwdb`'s
/// iterator walk (`tkrzwdb_utils.h`), so its bucket walk is the pin's.
pub struct TkrzwUserBigramDb {
    db: Db,
}

/// Copies a borrowed record without allocating a single-record batch.
/// `DBM::Set` and `ProcessMulti` both reach `TinyDBMImpl::ProcessImpl`;
/// with the same walk order, each bucket therefore has the same chain.
fn db_set_record(db: &Db, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
    let key_size = c_len(key)?;
    let value_size = c_len(value)?;
    // SAFETY: the destination is open and writable. Both slices (including
    // empty slices, whose pointers are non-null) outlive this synchronous
    // call, with validated lengths. Set copies their bytes before returning;
    // it retains neither pointer. overwrite=true matches the pin's Set.
    let ok = unsafe {
        ffi::tkrzw_dbm_set(
            db.0.as_ptr(),
            key.as_ptr().cast::<c_char>(),
            key_size,
            value.as_ptr().cast::<c_char>(),
            value_size,
            true,
        )
    };
    check(ok)
}

/// One `Set` or `Remove` against `db`, through the same record-processor
/// path every commit uses.
fn db_set_one(db: &Db, key: &[u8], value: Option<&[u8]>) -> Result<(), StoreError> {
    let (value, remove) = value.map_or((Vec::new(), true), |value| (value.to_vec(), false));
    let mutation = Mutation {
        key_size: c_len(key)?,
        value_size: c_len(&value)?,
        key: key.to_vec(),
        value,
        remove,
    };
    db_apply(db, std::slice::from_ref(&mutation))
}

/// Visits one borrowed record at a time in the database's native order.
fn visit_db(
    db: &Db,
    mut visit: impl FnMut(&[u8], &[u8]) -> Result<(), StoreError>,
) -> Result<(), StoreError> {
    scan_rows(db, b"", Bound::Unbounded, Bound::Unbounded, |key, value| {
        visit(key, value)?;
        Ok(Flow::Continue(()))
    })
}

impl crate::UserBigramDb for TkrzwUserBigramDb {
    fn empty() -> Result<Self, StoreError> {
        // `m_db = new TinyDBM;` — an in-memory database at the default
        // bucket count; the empty path is PolyDBM's on-memory form.
        // SAFETY: both strings outlive the call; the returned handle is
        // NULL on failure, taken as an error immediately.
        let db = unsafe { ffi::tkrzw_dbm_open(c"".as_ptr(), true, c"dbm=tiny".as_ptr()) };
        let Some(db) = NonNull::new(db) else {
            return Err(status_error());
        };
        Ok(Self { db: Db(db) })
    }

    fn load_db(path: &Path) -> Result<Self, StoreError> {
        let this = Self::empty()?;
        // `HashDBM tmp_db; tmp_db.Open(dbfile, false, OPEN_NO_CREATE)`
        // then `copy_tkrzwdb(&tmp_db, m_db)`; a file that does not open
        // leaves the container empty, as the pin's does.
        if let Ok(file) = open_hash(path) {
            // `copy_tkrzwdb` copies nothing from a file with no live
            // record, but its HashDBM iterator still reads every bucket
            // slot, and `save_db` writes 1,048,583 of them whatever the
            // rows: about 329M instructions per init
            // (`docs/findings/tkrzw-init-1099.md`). A file that holds no
            // record leaves the container exactly as that copy would, so
            // it is not walked; every other file is.
            if holds_no_record(&file.db, path) {
                return Ok(this);
            }
            // A failed source walk must not discard unrelated learned data.
            if let Err(error) = visit_db(&file.db, |key, value| db_set_record(&this.db, key, value))
            {
                eprintln!("user-bigram load {}: {error}", path.display());
            }
        }
        Ok(this)
    }

    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        db_get(&self.db, key, value)
    }

    fn store(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        db_set_one(&self.db, key, Some(value))
    }

    fn remove(&self, key: &[u8]) -> Result<(), StoreError> {
        db_set_one(&self.db, key, None)
    }

    fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError> {
        // One vector of four-byte tokens reserved for the record count, as
        // `get_all_items` appends them to one `GArray`
        // (`ngram_tkrzwdb.cpp:143-172` at 074a2219).
        let mut keys = Vec::new();
        keys.reserve_exact(usize::try_from(db_count(&self.db)?).unwrap_or(0));
        visit_db(&self.db, |key, _| {
            keys.push(<[u8; 4]>::try_from(key).ok());
            Ok(())
        })?;
        Ok(keys)
    }

    fn save_db(&self, path: &Path) -> Result<(), StoreError> {
        // `Bigram::save_db` (`ngram_tkrzwdb.cpp:66-87`): remove the file,
        // open a fresh `HashDBM` at its default tuning, copy this
        // container into it in iterator order, synchronize, close.
        crate::remove_if_present(path)?;
        let file = open_hash_create(path)?;
        visit_db(&self.db, |key, value| {
            db_set_one(&file.db, key, Some(value))
        })?;
        db_synchronize(&file.db, false)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{
        HASH_METADATA_SIZE, TkrzwUserBigramDb, db_count, db_set_one, db_set_record,
        holds_no_record, is_healthy, open_hash, open_hash_create, record_base, visit_db,
    };
    use crate::UserBigramDb;

    /// A directory under the temp directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let path = std::env::temp_dir()
                .join(format!("oxpinyin-store-tkrzw-{tag}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn sorted_keys(db: &TkrzwUserBigramDb) -> Vec<Option<[u8; 4]>> {
        let mut keys = db.keys().unwrap();
        keys.sort();
        keys
    }

    /// The record-section offset the header of the file at `path` places.
    fn file_record_base(path: &std::path::Path) -> Option<u64> {
        let bytes = std::fs::read(path).unwrap();
        record_base(bytes[..HASH_METADATA_SIZE].try_into().unwrap())
    }

    #[test]
    fn a_file_saved_with_no_rows_holds_no_record_and_loads_empty() {
        // What every profile the pin saves before learning a bigram holds:
        // 1,048,583 buckets and nothing after them, 4,198,400 bytes
        // (`docs/findings/tkrzw-init-1099.md`).
        let dir = TempDir::new("no-rows");
        let path = dir.0.join("user_bigram.db");
        TkrzwUserBigramDb::empty().unwrap().save_db(&path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 4_198_400);
        assert_eq!(file_record_base(&path), Some(4_198_400));
        assert!(holds_no_record(&open_hash(&path).unwrap().db, &path));
        assert!(
            TkrzwUserBigramDb::load_db(&path)
                .unwrap()
                .keys()
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_file_saved_with_rows_is_walked() {
        let dir = TempDir::new("rows");
        let path = dir.0.join("user_bigram.db");
        let db = TkrzwUserBigramDb::empty().unwrap();
        for token in 1_u32..=3 {
            db.store(&token.to_le_bytes(), b"gram").unwrap();
        }
        db.save_db(&path).unwrap();
        assert!(!holds_no_record(&open_hash(&path).unwrap().db, &path));
        let expected: Vec<_> = (1_u32..=3).map(|token| Some(token.to_le_bytes())).collect();
        assert_eq!(
            sorted_keys(&TkrzwUserBigramDb::load_db(&path).unwrap()),
            expected
        );
    }

    #[test]
    fn a_healthy_zero_count_header_over_live_records_is_walked() {
        // A cleanly saved file with three records whose header count alone
        // is rewritten to 0: the closure flag is still set and the size
        // still agrees with the metadata, so the file opens healthy with
        // no records counted, while its record section holds all three.
        // The pin's `copy_tkrzwdb` walks the buckets and copies them; so
        // must `load_db`.
        let dir = TempDir::new("stale-count");
        let path = dir.0.join("user_bigram.db");
        let db = TkrzwUserBigramDb::empty().unwrap();
        for token in 1_u32..=3 {
            db.store(&token.to_le_bytes(), b"gram").unwrap();
        }
        db.save_db(&path).unwrap();
        let mut bytes = std::fs::read(&path).unwrap();
        // `num_records`, eight big-endian bytes at 24 (`META_OFFSET_NUM_RECORDS`).
        bytes[24..32].copy_from_slice(&0_u64.to_be_bytes());
        std::fs::write(&path, &bytes).unwrap();
        let file = open_hash(&path).unwrap();
        assert!(is_healthy(&file.db));
        assert_eq!(db_count(&file.db).unwrap(), 0);
        assert!(std::fs::metadata(&path).unwrap().len() > file_record_base(&path).unwrap());
        assert!(!holds_no_record(&file.db, &path));
        drop(file);
        let expected: Vec<_> = (1_u32..=3).map(|token| Some(token.to_le_bytes())).collect();
        assert_eq!(
            sorted_keys(&TkrzwUserBigramDb::load_db(&path).unwrap()),
            expected
        );
    }

    #[test]
    fn loading_preserves_the_pins_colliding_bucket_chain_and_values() {
        // PrimaryHash at tkrzw 9ee8416 puts these three little-endian
        // tokens in default bucket 74121. Derived from HashMurmur's
        // four-byte case and PrimaryHash's fold, not from an oracle run.
        // The HashDBM iterator sorts their byte strings as b, a, c;
        // TinyDBM prepends, so its chain (and save walk) must be c, a, b.
        let a = 0x0100_0d46_u32.to_le_bytes();
        let b = 0x0100_4404_u32.to_le_bytes();
        let c = 0x0100_4dd8_u32.to_le_bytes();
        let rows = [
            (a.to_vec(), vec![0, 1, 0, 255]),
            (b.to_vec(), Vec::new()),
            (c.to_vec(), (0_u8..=255).cycle().take(804).collect()),
        ];
        let dir = TempDir::new("collision-order");
        let path = dir.0.join("user_bigram.db");
        let file = open_hash_create(&path).unwrap();
        // Deliberately differs from the source's iterator order.
        for (key, value) in &rows {
            db_set_one(&file.db, key, Some(value)).unwrap();
        }
        drop(file);

        let file = open_hash(&path).unwrap();
        let reference = TkrzwUserBigramDb::empty().unwrap();
        let mut source_keys = Vec::new();
        visit_db(&file.db, |key, value| {
            source_keys.push(key.to_vec());
            // The previous path, retained as an independent reference.
            db_set_one(&reference.db, key, Some(value))
        })
        .unwrap();
        assert_eq!(source_keys, [b.to_vec(), a.to_vec(), c.to_vec()]);
        drop(file);

        let loaded = TkrzwUserBigramDb::load_db(&path).unwrap();
        let expected = vec![rows[2].clone(), rows[0].clone(), rows[1].clone()];
        for db in [&reference, &loaded] {
            let mut actual = Vec::new();
            visit_db(&db.db, |key, value| {
                actual.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .unwrap();
            assert_eq!(
                actual, expected,
                "native bucket-chain order, not sorted rows"
            );
        }
    }

    #[test]
    fn single_record_set_copies_empty_binary_and_replacement_values() {
        let db = TkrzwUserBigramDb::empty().unwrap();
        db_set_record(&db.db, b"", b"").unwrap();
        db_set_record(&db.db, b"\0k\xff!", b"\0value\xff").unwrap();
        assert_eq!(db_count(&db.db).unwrap(), 2);
        let mut value = Vec::new();
        assert!(db.get(b"", &mut value).unwrap());
        assert!(value.is_empty());
        assert!(db.get(b"\0k\xff!", &mut value).unwrap());
        assert_eq!(value, b"\0value\xff");

        let before = db.keys().unwrap();
        db_set_record(&db.db, b"\0k\xff!", b"").unwrap();
        assert!(db.get(b"\0k\xff!", &mut value).unwrap());
        assert!(value.is_empty());
        assert_eq!(db_count(&db.db).unwrap(), 2);
        assert_eq!(
            db.keys().unwrap(),
            before,
            "overwrite preserves chain position"
        );
    }

    #[test]
    fn an_unhealthy_file_is_walked_even_when_its_metadata_counts_no_rows() {
        // A copy taken while the writer still has the file open: its
        // metadata is the one the writer's open wrote (not closed, no
        // records), while its buckets already reach three records. The
        // pin's `copy_tkrzwdb` walks the buckets and copies them; so must
        // `load_db`.
        let dir = TempDir::new("unhealthy");
        let live = dir.0.join("live.db");
        let path = dir.0.join("user_bigram.db");
        let writer = open_hash_create(&live).unwrap();
        for token in 1_u32..=3 {
            db_set_one(&writer.db, &token.to_le_bytes(), Some(b"gram")).unwrap();
        }
        std::fs::copy(&live, &path).unwrap();
        drop(writer);
        let file = open_hash(&path).unwrap();
        assert_eq!(db_count(&file.db).unwrap(), 0);
        assert!(!is_healthy(&file.db));
        assert!(!holds_no_record(&file.db, &path));
        drop(file);
        let expected: Vec<_> = (1_u32..=3).map(|token| Some(token.to_le_bytes())).collect();
        assert_eq!(
            sorted_keys(&TkrzwUserBigramDb::load_db(&path).unwrap()),
            expected
        );
    }
}
