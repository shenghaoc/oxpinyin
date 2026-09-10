//! LMDB backend for the store capability tiers, over the **system**
//! LMDB C API.
//!
//! Enabled by the `lmdb` cargo feature. The environment, transactions,
//! cursors and record I/O all go through `lmdb.h` as the distribution
//! installs it (Debian `liblmdb-dev` to build, `liblmdb0` at run time);
//! `build.rs` generates the declarations in [`ffi`] from that header and
//! links the library `pkg-config` names. No copy of LMDB's C is
//! compiled into oxpinyin — `docs/runbooks/backends.md` carries the
//! packaging contract and `build.rs` says why the bindings are
//! generated rather than checked in.
//!
//! Key ordering uses the default LMDB byte-lexicographic comparator, so
//! big-endian encoded keys sort identically to the redb backend.
//! Writable environments open with `MDB_WRITEMAP`: commits write through
//! the mapping instead of staging pages in malloc'd buffers and copying
//! them in. Every environment opens with `MDB_NOTLS`, so a read
//! transaction belongs to the value that holds it rather than to a
//! thread slot, and one thread may hold several at once.
//!
//! # What this module owns
//!
//! [`ffi`] is raw declarations; everything that makes calling them sound
//! lives here. Three rules cover it:
//!
//! 1. **Handles are owned by RAII wrappers.** [`Env`] closes on drop,
//!    [`Txn`] aborts on drop unless it was committed or aborted
//!    explicitly, [`Cursor`] closes on drop. No early return can leak
//!    one, which is what makes the `?`-heavy paths below safe.
//! 2. **Borrowed records never outlive their transaction.** LMDB hands
//!    back pointers into the memory map; anything that outlives a call
//!    is copied first, and a visitor's slices are bounded by the call
//!    that invokes it.
//! 3. **A `MDB_dbi` is cached only once its opening transaction has
//!    committed**, per LMDB's own rule — see [`SharedEnv`].
//!
//! # Unsafe waiver
//!
//! This module and its [`ffi`] carry an explicit `allow(unsafe_code)`:
//! the generated declarations are unsafe by nature and every LMDB call
//! goes through one. The waiver is scoped to this backend by decision —
//! the workspace outside it stays `deny` — and it waives safety
//! ceremony, not correctness: every block carries a `SAFETY` comment,
//! and the shared read and write suites gate this backend like any
//! other.
#![expect(
    unsafe_code,
    reason = "FFI over the system liblmdb; every block carries a SAFETY comment"
)]

mod ffi;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::fmt;
use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ops::{Bound, Deref};
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

use crate::common::validate_path;
use crate::{RAW_TABLE, ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

// ── helpers ───────────────────────────────────────────────────────

const MAX_DBS: u32 = 32;
/// LMDB accepts keys of at most 511 bytes and rejects the empty key
/// (`MDB_BAD_VALSIZE`); enforced ahead of insertion so callers get
/// [`StoreError::InvalidInput`] instead of a backend error.
const MAX_KEY_LEN: usize = 511;
/// Default map-size ceiling: 1 GiB of virtual address space.  LMDB
/// commits address space sparsely, so this is a cap on database size,
/// not an up-front allocation.  An open environment is never resized
/// here, so exceeding the cap fails every write with `MDB_MAP_FULL`;
/// users with larger corpora should open via
/// [`LmdbStore::create_with_map_size`].
const MAP_SIZE: usize = 1 << 30;
/// Creation mode for a new environment file, matching LMDB's own tools.
const FILE_MODE: ffi::mdb_mode_t = 0o644;

/// The system page size, which a map size must be a multiple of.
fn page_size() -> usize {
    // SAFETY: `sysconf` is a pure query taking no pointers, and
    // `_SC_PAGESIZE` is generated from the same `<unistd.h>` the linked
    // libc implements.
    let raw = unsafe { ffi::sysconf(ffi::_SC_PAGESIZE as std::ffi::c_int) };
    // `sysconf` answers -1 only for a name it does not know, which
    // `_SC_PAGESIZE` never is. Treat any implausible answer as "cannot
    // tell" and fall back to 4 KiB rather than dividing by zero; the
    // check this feeds only ever gets stricter as a result.
    usize::try_from(raw).unwrap_or(0).max(4096)
}

/// Translates an LMDB result code into the store's error taxonomy.
///
/// The three-way split comes from [`ffi::classify`]: LMDB returns a
/// **positive** `errno` for system failures and a **negative** `MDB_*`
/// constant for its own. That is what keeps I/O failures landing on
/// [`StoreError::Io`] — a missing file on `open_read_only` is `ENOENT`,
/// not an LMDB code — without matching on message text.
fn check(rc: std::ffi::c_int) -> Result<(), StoreError> {
    match ffi::classify(rc) {
        ffi::Code::Success => Ok(()),
        ffi::Code::Errno(errno) => Err(StoreError::Io(std::io::Error::from_raw_os_error(errno))),
        ffi::Code::Mdb(ffi::MDB_MAP_FULL) => Err(StoreError::Backend(Box::new(MapFullError))),
        ffi::Code::Mdb(code) => Err(StoreError::Backend(Box::new(LmdbError {
            code,
            message: ffi::strerror(code),
        }))),
    }
}

/// `Ok(true)` when the call succeeded, `Ok(false)` on `MDB_NOTFOUND`.
///
/// An absent record is a legitimate answer for `get`, `del` and every
/// cursor step, not a failure; only the remaining codes reach [`check`].
fn check_found(rc: std::ffi::c_int) -> Result<bool, StoreError> {
    if rc == ffi::MDB_NOTFOUND {
        return Ok(false);
    }
    check(rc)?;
    Ok(true)
}

/// A failure LMDB reported through its own code space.
#[derive(Debug)]
struct LmdbError {
    code: i32,
    message: String,
}

impl fmt::Display for LmdbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (LMDB {})", self.message, self.code)
    }
}

impl std::error::Error for LmdbError {}

/// The LMDB map-size ceiling was reached; writes fail until the store is
/// reopened with a larger [`LmdbStore::create_with_map_size`] ceiling.
#[derive(Debug)]
struct MapFullError;

impl fmt::Display for MapFullError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("LMDB map-size limit reached; reopen with a larger map size")
    }
}

impl std::error::Error for MapFullError {}

fn normalize_bound(bound: Bound<&[u8]>) -> Bound<&[u8]> {
    match bound {
        Bound::Included([]) | Bound::Excluded([]) => Bound::Unbounded,
        other => other,
    }
}

fn is_empty_upper_bound(bound: Bound<&[u8]>) -> bool {
    matches!(bound, Bound::Included([]) | Bound::Excluded([]))
}

// ── owned handles ─────────────────────────────────────────────────

/// An owned `MDB_env`, closed exactly once when it drops.
struct Env(NonNull<ffi::MDB_env>);

// SAFETY: LMDB documents an environment as usable from several threads,
// and every environment here opens with `MDB_NOTLS`, which removes the
// thread-local reader slot that would otherwise bind a read transaction
// to its creating thread. The one entry point LMDB does not make
// thread-safe is `mdb_dbi_open`, which this crate serializes itself (see
// `SharedEnv`).
unsafe impl Send for Env {}
// SAFETY: as above — shared access from several threads is what the
// library is built for.
unsafe impl Sync for Env {}

impl Env {
    fn as_ptr(&self) -> *mut ffi::MDB_env {
        self.0.as_ptr()
    }

    /// Begins a read-only transaction.
    fn read_txn(&self) -> Result<Txn<'_>, StoreError> {
        self.begin(ffi::MDB_RDONLY)
    }

    /// Begins a write transaction. LMDB serializes writers on a mutex in
    /// the lock file, so this blocks while another write transaction on
    /// the same environment is live — the single-writer guarantee the
    /// store's contract rests on.
    fn write_txn(&self) -> Result<Txn<'_>, StoreError> {
        self.begin(0)
    }

    fn begin(&self, flags: std::ffi::c_uint) -> Result<Txn<'_>, StoreError> {
        let mut txn: *mut ffi::MDB_txn = std::ptr::null_mut();
        // SAFETY: the environment is open, `parent` is null (this backend
        // opens no nested transactions), and `txn` is a live out-pointer
        // LMDB writes on success.
        check(unsafe { ffi::mdb_txn_begin(self.as_ptr(), std::ptr::null_mut(), flags, &mut txn) })?;
        Ok(Txn {
            ptr: Some(NonNull::new(txn).ok_or_else(|| success_without("transaction"))?),
            _env: PhantomData,
        })
    }

    /// Flushes the environment to stable storage, the durability half of
    /// a `MDB_NOSYNC` load.
    fn force_sync(&self) -> Result<(), StoreError> {
        // SAFETY: the environment is open; `force = 1` flushes even when
        // it was opened with MDB_NOSYNC, which is the only reason this is
        // ever called.
        check(unsafe { ffi::mdb_env_sync(self.as_ptr(), 1) })
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        // SAFETY: the handle came from `mdb_env_create`, is closed
        // exactly once (this is its sole owner and Drop runs once), and
        // every transaction and cursor opened on it is already dropped —
        // `SharedEnv` is released only when no store still holds it.
        unsafe { ffi::mdb_env_close(self.as_ptr()) };
    }
}

/// LMDB answered `MDB_SUCCESS` but left the out-parameter null. Not
/// reachable through any documented path; reported rather than
/// unwrapped, because rule 4 of the constitution admits no panic.
fn success_without(what: &'static str) -> StoreError {
    StoreError::Backend(Box::new(std::io::Error::other(format!(
        "LMDB reported success but returned no {what}"
    ))))
}

/// An owned transaction. Aborts on drop unless [`Txn::commit`] or
/// [`Txn::abort`] consumed it first, so no early return can leak one.
struct Txn<'e> {
    /// `None` once the transaction has been handed back to LMDB.
    ptr: Option<NonNull<ffi::MDB_txn>>,
    _env: PhantomData<&'e Env>,
}

impl Txn<'_> {
    fn as_ptr(&self) -> *mut ffi::MDB_txn {
        self.ptr.map_or(std::ptr::null_mut(), NonNull::as_ptr)
    }

    fn commit(mut self) -> Result<(), StoreError> {
        let Some(ptr) = self.ptr.take() else {
            return Ok(());
        };
        // SAFETY: the transaction is live and owned here. `mdb_txn_commit`
        // frees the handle whether it succeeds or fails, which is why the
        // pointer was taken out first: Drop must not abort it again.
        check(unsafe { ffi::mdb_txn_commit(ptr.as_ptr()) })
    }

    fn abort(mut self) {
        if let Some(ptr) = self.ptr.take() {
            // SAFETY: live, owned, and not previously finished; the handle
            // is freed by this call and never named again.
            unsafe { ffi::mdb_txn_abort(ptr.as_ptr()) };
        }
    }
}

impl Drop for Txn<'_> {
    fn drop(&mut self) {
        if let Some(ptr) = self.ptr.take() {
            // SAFETY: as in `abort` — the only way to reach here with a
            // live handle is an early return that never finished it.
            unsafe { ffi::mdb_txn_abort(ptr.as_ptr()) };
        }
    }
}

/// One row a cursor step yields: key and value, both borrowing the
/// transaction's mapping for as long as the cursor lives.
type Row<'a> = (&'a [u8], &'a [u8]);

/// An owned cursor, closed on drop and bounded by its transaction.
struct Cursor<'t> {
    ptr: NonNull<ffi::MDB_cursor>,
    _txn: PhantomData<&'t ()>,
}

impl<'t> Cursor<'t> {
    fn open(txn: &'t Txn<'_>, dbi: ffi::MDB_dbi) -> Result<Self, StoreError> {
        let mut cursor: *mut ffi::MDB_cursor = std::ptr::null_mut();
        // SAFETY: the transaction is live for `'t`, `dbi` was opened on
        // its environment, and `cursor` is a live out-pointer.
        check(unsafe { ffi::mdb_cursor_open(txn.as_ptr(), dbi, &mut cursor) })?;
        Ok(Self {
            ptr: NonNull::new(cursor).ok_or_else(|| success_without("cursor"))?,
            _txn: PhantomData,
        })
    }

    /// One cursor step. Returns the borrowed key and value, or `None` at
    /// the end of the range.
    ///
    /// The slices point into the transaction's mapping. Borrowing
    /// `&self` ties them to the cursor, which cannot outlive the
    /// transaction it was opened on.
    fn step(
        &self,
        op: ffi::MDB_cursor_op::Type,
        key: Option<&[u8]>,
    ) -> Result<Option<Row<'_>>, StoreError> {
        let mut k = key.map_or_else(ffi::empty_val, ffi::val);
        let mut v = ffi::empty_val();
        // SAFETY: the cursor is live; `k` and `v` are live out-params for
        // the call, and when `key` is `Some` the slice it borrows is this
        // function's own argument and so outlives the call.
        let rc = unsafe { ffi::mdb_cursor_get(self.ptr.as_ptr(), &mut k, &mut v, op) };
        if !check_found(rc)? {
            return Ok(None);
        }
        // SAFETY: the step succeeded, so both vals describe records in
        // the live transaction's map, with `mv_size` their true lengths.
        Ok(Some(unsafe { (ffi::as_slice(&k), ffi::as_slice(&v)) }))
    }
}

impl Drop for Cursor<'_> {
    fn drop(&mut self) {
        // SAFETY: live, owned, closed exactly once, and always before the
        // transaction it was opened on ends.
        unsafe { ffi::mdb_cursor_close(self.ptr.as_ptr()) };
    }
}

// ── record access ─────────────────────────────────────────────────

/// Point read. `None` when the key is absent.
fn db_get(txn: &Txn<'_>, dbi: ffi::MDB_dbi, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
    let mut k = ffi::val(key);
    let mut v = ffi::empty_val();
    // SAFETY: the transaction is live, `dbi` belongs to its environment,
    // `k` borrows `key` for the duration of the call, and `v` is a live
    // out-param LMDB points into the map.
    let rc = unsafe { ffi::mdb_get(txn.as_ptr(), dbi, &mut k, &mut v) };
    if !check_found(rc)? {
        return Ok(None);
    }
    // SAFETY: the read succeeded, so `v` describes a record in the live
    // transaction's map. Copied here, before the transaction can end,
    // because the result outlives it.
    Ok(Some(unsafe { ffi::as_slice(&v) }.to_vec()))
}

/// Inserts or replaces one record. `flags` carries `MDB_APPEND` for the
/// bulk loader and 0 everywhere else.
fn db_put(
    txn: &Txn<'_>,
    dbi: ffi::MDB_dbi,
    key: &[u8],
    value: &[u8],
    flags: std::ffi::c_uint,
) -> Result<(), StoreError> {
    let mut k = ffi::val(key);
    let mut v = ffi::val(value);
    // SAFETY: the transaction is live and writable, `dbi` belongs to its
    // environment, and both vals borrow slices that outlive the call.
    // LMDB copies the record before returning, so neither borrow is
    // retained past it.
    check(unsafe { ffi::mdb_put(txn.as_ptr(), dbi, &mut k, &mut v, flags) })
}

/// Deletes one record; an absent key is not an error, matching every
/// other backend's `remove`.
fn db_del(txn: &Txn<'_>, dbi: ffi::MDB_dbi, key: &[u8]) -> Result<(), StoreError> {
    let mut k = ffi::val(key);
    // SAFETY: the transaction is live and writable, `dbi` belongs to its
    // environment, `k` borrows `key` for the call, and a null data
    // pointer is how LMDB is told to delete whatever value the key holds
    // (this backend never uses MDB_DUPSORT).
    let rc = unsafe { ffi::mdb_del(txn.as_ptr(), dbi, &mut k, std::ptr::null_mut()) };
    check_found(rc)?;
    Ok(())
}

/// Whether the table holds no records.
fn db_is_empty(txn: &Txn<'_>, dbi: ffi::MDB_dbi) -> Result<bool, StoreError> {
    // SAFETY: `MDB_stat` is generated from the same header as the linked
    // library, so an all-zero value has exactly the layout `mdb_stat`
    // writes into.
    let mut stat: ffi::MDB_stat = unsafe { std::mem::zeroed() };
    // SAFETY: the transaction is live, `dbi` belongs to its environment,
    // and `stat` is a live out-param.
    check(unsafe { ffi::mdb_stat(txn.as_ptr(), dbi, &mut stat) })?;
    Ok(stat.ms_entries == 0)
}

/// Walks `[lo, hi]` in ascending key order, handing every row to
/// `visit`.
///
/// The visitor's slices borrow the transaction's mapping and are valid
/// only for its call, exactly as they were under the iterator this
/// replaced.
fn db_range(
    txn: &Txn<'_>,
    dbi: ffi::MDB_dbi,
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
    visit: &mut Visitor<'_>,
) -> Result<(), StoreError> {
    let cursor = Cursor::open(txn, dbi)?;
    // Position at the first candidate: `MDB_SET_RANGE` lands on the
    // first key >= its argument, which *is* an inclusive lower bound; an
    // exclusive one steps past an exact hit just below.
    let (start_op, start_key) = match lo {
        Bound::Unbounded => (ffi::MDB_cursor_op::MDB_FIRST, None),
        Bound::Included(k) | Bound::Excluded(k) => (ffi::MDB_cursor_op::MDB_SET_RANGE, Some(k)),
    };
    let mut current = cursor.step(start_op, start_key)?;
    if let (Bound::Excluded(low), Some((key, _))) = (lo, current)
        && key == low
    {
        current = cursor.step(ffi::MDB_cursor_op::MDB_NEXT, None)?;
    }
    while let Some((key, value)) = current {
        let past_end = match hi {
            Bound::Unbounded => false,
            Bound::Included(high) => key > high,
            Bound::Excluded(high) => key >= high,
        };
        if past_end {
            break;
        }
        visit(key, value)?;
        current = cursor.step(ffi::MDB_cursor_op::MDB_NEXT, None)?;
    }
    Ok(())
}

/// Walks every row of a table in ascending key order.
fn db_for_each(
    txn: &Txn<'_>,
    dbi: ffi::MDB_dbi,
    visit: &mut Visitor<'_>,
) -> Result<(), StoreError> {
    db_range(txn, dbi, Bound::Unbounded, Bound::Unbounded, visit)
}

/// Opens a table handle on `txn`. `create` adds `MDB_CREATE`; without it
/// an absent table answers `Ok(None)` rather than failing.
///
/// Every caller holds [`SharedEnv::dbi_open`] across this call *and*
/// until its transaction finishes, which is LMDB's exclusion requirement
/// for this entry point.
fn dbi_open(txn: &Txn<'_>, table: &str, create: bool) -> Result<Option<ffi::MDB_dbi>, StoreError> {
    let name =
        CString::new(table).map_err(|_| StoreError::InvalidInput("table name contains NUL"))?;
    let flags = if create { ffi::MDB_CREATE } else { 0 };
    let mut dbi: ffi::MDB_dbi = 0;
    // SAFETY: the transaction is live, `name` outlives the call as a
    // NUL-terminated string, and `dbi` is a live out-param.
    let rc = unsafe { ffi::mdb_dbi_open(txn.as_ptr(), name.as_ptr(), flags, &mut dbi) };
    if rc == ffi::MDB_NOTFOUND {
        return Ok(None);
    }
    if rc == ffi::MDB_DBS_FULL {
        // LMDB caps the environment at MAX_DBS named tables; the
        // over-limit table name is caller input, so surface it as
        // InvalidInput rather than an opaque backend error.
        return Err(StoreError::InvalidInput(
            "too many distinct tables (LMDB caps a store at 32)",
        ));
    }
    check(rc)?;
    Ok(Some(dbi))
}

// ── environment open ──────────────────────────────────────────────

fn open_env(
    path: &Path,
    read_only: bool,
    map_size: usize,
    no_sync: bool,
) -> Result<Env, StoreError> {
    validate_path(path)?;
    if map_size == 0 {
        return Err(StoreError::InvalidInput("map size must be nonzero"));
    }
    // LMDB rounds a map size up to the next page multiple without saying
    // so, which would hand the caller a ceiling other than the one they
    // asked for; refuse instead, as this backend always has.
    if !map_size.is_multiple_of(page_size()) {
        return Err(StoreError::InvalidInput(
            "map size must be a multiple of the system page size",
        ));
    }
    let c_path = CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| StoreError::InvalidInput("path contains NUL"))?;

    let mut raw: *mut ffi::MDB_env = std::ptr::null_mut();
    // SAFETY: `raw` is a live out-pointer; on success LMDB writes a fresh
    // environment handle into it.
    check(unsafe { ffi::mdb_env_create(&mut raw) })?;
    let env = Env(NonNull::new(raw).ok_or_else(|| success_without("environment"))?);
    // From here every `?` drops `env`, which closes the handle. LMDB
    // requires `mdb_env_close` even when `mdb_env_open` fails, and the
    // RAII wrapper is what guarantees it on every early return.

    // SAFETY: the environment is created and not yet open, which is when
    // both setters must be called.
    check(unsafe { ffi::mdb_env_set_maxdbs(env.as_ptr(), MAX_DBS) })?;
    // SAFETY: as above; `map_size` is a validated page multiple.
    check(unsafe { ffi::mdb_env_set_mapsize(env.as_ptr(), map_size) })?;

    // MDB_NOSUBDIR: the store is a single file plus its `-lock` sidecar,
    // not a directory. MDB_NOTLS: a read transaction belongs to the value
    // holding it rather than to a thread slot, so one thread may hold
    // several and a reader is not pinned to its creating thread.
    let mut flags = ffi::MDB_NOSUBDIR | ffi::MDB_NOTLS;
    if read_only {
        flags |= ffi::MDB_RDONLY;
    } else {
        // MDB_WRITEMAP: commits write directly into the writeable
        // mapping, removing the memcpy from the write transaction's
        // staging buffers into the map. Sound here because this backend
        // is single-writer and no database uses MDB_DUPSORT.
        flags |= ffi::MDB_WRITEMAP;
    }
    if no_sync {
        // MDB_NOSYNC: skip the fsync per commit. Only the one-shot bulk
        // loader opens this way; it forces one sync after its final
        // commit (see [`LmdbStore::bulk_load_raw`]), so the file is
        // durable before the environment is shared or closed.
        flags |= ffi::MDB_NOSYNC;
    }

    // SAFETY: the environment is configured and unopened, and `c_path`
    // outlives the call as a NUL-terminated path.
    check(unsafe { ffi::mdb_env_open(env.as_ptr(), c_path.as_ptr(), flags, FILE_MODE) })?;
    Ok(env)
}

// ── env sharing ───────────────────────────────────────────────────
//
// LMDB must not have one environment file open twice in a single
// process — its own documentation is explicit, and doing so breaks the
// POSIX advisory locking the reader table depends on. Every other
// backend tolerates concurrent opens of one path, and the engine's tests
// and adapters lean on that (many capi tests open the same fixture
// tables in parallel). This registry restores the common contract for
// LMDB: one shared `Env` per canonical path, handed out by `Weak`
// upgrade, dropped (and the path reopenable) when the last store using
// it goes away — the same shape as `oxpinyin-user`'s store registry.

/// One registry row: the live environment, whether it was opened
/// writable, the map-size ceiling it was opened with, and whether it was
/// opened with per-commit syncing disabled — a live environment is
/// neither reopened at a different ceiling nor resized here, and a
/// NO_SYNC environment must not be silently handed to an opener that
/// expects durable commits, so a mismatching request is refused rather
/// than served with the live environment's mode.
type EnvSlot = (Weak<SharedEnv>, bool, usize, bool);

static OPEN_ENVS: OnceLock<Mutex<HashMap<PathBuf, EnvSlot>>> = OnceLock::new();

fn open_envs() -> &'static Mutex<HashMap<PathBuf, EnvSlot>> {
    OPEN_ENVS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The canonical registry key for `path` (falls back to the path itself
/// when it does not exist yet, e.g. `create` of a new file).
fn env_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// One shared environment plus the machinery that keeps LMDB operations
/// on it safe to call from many threads at once.
///
/// Two independent hazards live here.
///
/// **Env close/reopen.** A second `mdb_env_open` on a path whose
/// previous environment is still open is exactly what LMDB forbids. The
/// last `Arc<SharedEnv>` drop decrements the strong count to zero
/// *before* Rust runs this Drop impl, so a concurrent [`shared_env`]
/// caller can see [`Weak::upgrade`] return `None` while `mdb_env_close`
/// has not even started. [`Drop`] takes the registry mutex, evicts the
/// (dead) entry, and only then closes the environment — so the close
/// runs while we hold the same mutex [`shared_env`] takes for its
/// live-check and its `open_env` call, and a caller that finds a dead
/// entry still in the map knows a close is in flight
/// ([`close_in_flight`]) and retries instead of opening. Under a newer
/// glibc the un-serialized version of this window manifested as
/// `malloc(): unaligned tcache chunk detected` aborts in parallel capi
/// and workspace test runs (main tip `fcf0559`).
///
/// **[`mdb_dbi_open`](http://www.lmdb.tech/doc/group__mdb.html#gac08cad5b096925642ca359a6d6f0562a).**
/// LMDB's own contract: *"A transaction that uses this function must
/// finish (either commit or abort) before any other transaction in the
/// process may use this function"*. Its body writes into the env-wide
/// `me_dbxs`/`me_dbiseqs` arrays (through the `mt_dbxs` pointer
/// `mdb_txn_begin` aliases at `mdb.c:3283`), and its `mdb_txn_end`
/// counterpart on abort walks the same arrays and `free()`s
/// `me_dbxs[i].md_name.mv_data` for every `DB_NEW` slot the aborting
/// txn opened (`mdb.c:3399-3405`). Every store op — `get`/`for_each`/
/// `range`/`is_empty` for reads, the write txn's `put`/`remove`/… for
/// writes — would otherwise re-open the same table by name on every
/// call, colliding on `me_dbxs` writes when two threads on one env
/// raced (the tcache trip surfaced in CI). The fix takes LMDB's own
/// hint: *"once the transaction that called `mdb_dbi_open` successfully
/// commits, the handle resides in the shared environment and may be
/// used by other transactions"* — cache the `MDB_dbi` once, reuse it on
/// every subsequent op without holding any lock. Two mutexes carry
/// this: [`SharedEnv::dbis`] wraps a `HashMap` and is held only long
/// enough to read or write one entry; [`SharedEnv::dbi_open`] is the
/// process-wide serialization LMDB needs across the actual
/// `mdb_dbi_open` call. Read-side cache misses hold `dbi_open` for
/// the whole open-and-commit sequence and release it before returning;
/// write-side cache misses (through [`LmdbWriteTxn`]) hold `dbi_open`
/// from the first miss until the write txn commits or aborts, per the
/// spec's "opening txn must finish" clause. After the miss path
/// caches the handle, later store ops open their txn, use the cached
/// handle, run their reads or writes, and finish the txn — all with
/// no lock. That restores LMDB's concurrent-reader model.
///
/// Negative results are deliberately *not* cached. LMDB supports
/// several processes on one data file; a table that does not exist
/// when this env first probes it may be created later by another
/// process, and a cached "absent" would hide it for this env's
/// lifetime. Only present handles enter the cache; a miss re-probes
/// on every call, cheap next to the read txn that follows.
/// Newly-created tables land in [`LmdbWriteTxn::pending_cache`] and
/// are only promoted after the write txn commits (LMDB frees a
/// `DB_NEW` DBI on abort, so caching earlier would leave a dangling
/// handle).
///
/// Two documented nesting restrictions:
///
/// 1. **Uncached-table re-entry inside a write closure.** A
///    [`WriteStore::write`] closure that has already hit a cache
///    miss (and therefore holds [`Self::dbi_open`] until the txn
///    commits) must not re-enter a store on the same path with a
///    read call that itself hits a cache miss. `std::sync::Mutex`
///    is not reentrant, so the second [`Self::lock_dbi_open`] on
///    the same thread would deadlock the caller. Cache-hit re-entry
///    is safe **for reads only** — it never touches `dbi_open`.
///
/// 2. **Nested writes on the same path.** A [`WriteStore::write`]
///    closure that opens a second [`LmdbStore`] on the same path
///    and calls [`WriteStore::write`] on it blocks on LMDB's
///    single-writer mutex on the shared env — the outer txn cannot
///    return and commit while the inner `write_txn()` waits, so
///    the whole thread deadlocks. This is LMDB's own single-writer
///    guarantee, not the DBI-cache serialization, and it holds
///    even on a cache hit. Nested writes on the same path are
///    unsupported until the outer transaction completes.
///
/// Neither oxpinyin's own writers nor its fixtures hit either case:
/// the user store's schema is fixed at first use, every fixture
/// table is pre-created, and no write closure in the tree opens a
/// second store on the same path from inside itself.
/// [`LmdbWriteTxn::open_existing`] carries the same warning at the
/// method level for readers who reach it before this docblock.
struct SharedEnv {
    /// Kept in a [`ManuallyDrop`] so [`Drop::drop`] can run
    /// [`ManuallyDrop::drop`] on it explicitly, before releasing the
    /// registry mutex. Without that indirection Rust would drop the
    /// field after the impl returned and the serialization we rely on
    /// would be gone.
    inner: ManuallyDrop<Env>,
    key: PathBuf,
    /// Committed table handles keyed by table name — positive cache
    /// only. The mutex is held only for the one map read or write; the
    /// actual `mdb_dbi_open` runs under [`Self::dbi_open`].
    dbis: Mutex<HashMap<String, ffi::MDB_dbi>>,
    /// Serializes `mdb_dbi_open` calls across all txns on this env,
    /// per LMDB's exclusion rule. Read-side misses in
    /// [`SharedEnv::database`] hold it across the open + commit;
    /// write-side misses via [`LmdbWriteTxn`] hold it until the write
    /// txn commits or aborts (see [`LmdbWriteTxn::dbi_open_guard`]).
    /// Cache-hit paths never touch this mutex.
    dbi_open: Mutex<()>,
}

impl Deref for SharedEnv {
    type Target = Env;

    fn deref(&self) -> &Env {
        &self.inner
    }
}

impl SharedEnv {
    /// Return the table handle for `name` if it exists, `None` when it
    /// does not. Positive results are cached. Misses are *not* cached —
    /// see the [`SharedEnv`] docblock for why.
    ///
    /// On a cache miss opens a private read txn, runs `mdb_dbi_open`,
    /// commits it (so the DBI persists env-wide per the LMDB spec),
    /// and — if the table exists — inserts the handle into
    /// [`Self::dbis`]. The [`Self::dbi_open`] mutex is held across
    /// the whole open-and-commit sequence to satisfy LMDB's
    /// exclusion rule; [`Self::dbis`] is held only for the one map
    /// operation on each side. Cache hits take neither mutex beyond
    /// the one map read.
    fn database(&self, name: &str) -> Result<Option<ffi::MDB_dbi>, StoreError> {
        // Fast path: positive cache hit.
        {
            let cache = self.lock_dbis();
            if let Some(dbi) = cache.get(name) {
                return Ok(Some(*dbi));
            }
        }
        // Slow path: serialize the `mdb_dbi_open` under `dbi_open`,
        // then re-check the cache — another thread may have promoted
        // the DBI while we waited on the mutex.
        let _open_guard = self.lock_dbi_open();
        {
            let cache = self.lock_dbis();
            if let Some(dbi) = cache.get(name) {
                return Ok(Some(*dbi));
            }
        }
        let txn = self.inner.read_txn()?;
        let dbi = dbi_open(&txn, name, false)?;
        txn.commit()?;
        if let Some(d) = dbi {
            self.lock_dbis().insert(name.to_owned(), d);
        }
        Ok(dbi)
    }

    /// Insert tables a just-committed write txn created into the
    /// shared cache, so subsequent reads and writes find them on the
    /// fast path.
    fn promote_created(&self, entries: &[(String, ffi::MDB_dbi)]) {
        if entries.is_empty() {
            return;
        }
        let mut cache = self.lock_dbis();
        for (name, dbi) in entries {
            cache.insert(name.clone(), *dbi);
        }
    }

    fn lock_dbis(&self) -> MutexGuard<'_, HashMap<String, ffi::MDB_dbi>> {
        self.dbis
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_dbi_open(&self) -> MutexGuard<'_, ()> {
        self.dbi_open
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl Drop for SharedEnv {
    fn drop(&mut self) {
        let mut map = open_envs()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Evict our own entry only. A concurrent `shared_env` waiting on
        // this mutex may have already registered a fresh env under the
        // same key (e.g. after a re-open replaced our dead Weak), and
        // that entry is not ours to clear.
        if map
            .get(&self.key)
            .is_some_and(|slot| slot.0.strong_count() == 0)
        {
            map.remove(&self.key);
        }
        // SAFETY: `self` is being dropped and `inner` is not read again.
        // We drop it here (rather than letting Rust drop it after this
        // function returns) so `mdb_env_close` runs while we still hold
        // the registry mutex. A concurrent `shared_env` caller waits on
        // that mutex, so it cannot re-enter liblmdb on this or any other
        // path before the close is fully done.
        unsafe { ManuallyDrop::drop(&mut self.inner) };
    }
}

/// The registry's answer for a live environment: the shared handle
/// paired with a compatibility check (`Ok(())` on a match, an
/// [`StoreError::InvalidInput`] refusal on a mismatch), or `None` when
/// no live environment exists for `key`. The caller holds the registry
/// lock; nothing here waits.
///
/// The handle comes back paired with — never swallowed by — the check
/// so the caller can release the registry lock before it drops. If a
/// concurrent thread already dropped its Arc, `weak.upgrade()` here can
/// hold the LAST strong reference; dropping that Arc while the registry
/// mutex is held would re-enter [`SharedEnv::drop`], which locks the
/// same non-reentrant mutex — a self-deadlock that would freeze every
/// later `shared_env` call and every `SharedEnv::drop` in the process.
fn live_env(
    map: &HashMap<PathBuf, EnvSlot>,
    key: &Path,
    read_only: bool,
    map_size: usize,
    no_sync: bool,
) -> Option<(Arc<SharedEnv>, Result<(), StoreError>)> {
    let (weak, writable, live_map_size, live_no_sync) = map.get(key)?;
    let env = weak.upgrade()?;
    if *live_map_size != map_size {
        return Some((
            env,
            Err(StoreError::InvalidInput(
                "this LMDB file is already open in this process with a different map size; close those handles before opening it with this ceiling",
            )),
        ));
    }
    if *live_no_sync != no_sync {
        return Some((
            env,
            Err(StoreError::InvalidInput(
                "this LMDB file is already open in this process with a different durability mode; close those handles before opening it with this one",
            )),
        ));
    }
    if read_only || *writable {
        return Some((env, Ok(())));
    }
    Some((
        env,
        Err(StoreError::InvalidInput(
            "this LMDB file is already open read-only in this process; close those handles before opening it writable",
        )),
    ))
}

/// Whether `key` names an entry whose environment is mid-teardown: the
/// last `Arc` is gone but [`SharedEnv::drop`] has not yet taken the
/// registry mutex to evict the row and close the handle.
///
/// This is the one state a caller may neither share nor open through.
/// Sharing is impossible (the `Weak` cannot upgrade), and opening would
/// be the second `mdb_env_open` on a path whose first environment is
/// still open — precisely what LMDB forbids — so the caller backs off
/// and retries; the retry finds either the evicted row or a fresh
/// environment.
fn close_in_flight(map: &HashMap<PathBuf, EnvSlot>, key: &Path) -> bool {
    map.get(key).is_some_and(|slot| slot.0.strong_count() == 0)
}

/// One shared environment per path. A read-only request shares any live
/// env (the store-level `read_only` flag still refuses writes); a
/// writable request shares only a writable env — an env opened read-only
/// cannot be upgraded, so that mismatch is refused rather than handed a
/// handle that cannot write. A live env is shared only at the map size it
/// was opened with: a different ceiling cannot be applied to it, so that
/// mismatch is refused too, before the caller grows data past a ceiling
/// it does not actually hold. Likewise a NO_SYNC env (the bulk loader)
/// and a syncing env never share: the durability difference is a mode the
/// caller chose, not one to silently override.
///
/// The whole open sequence — live-env check, then `open_env` on a miss —
/// runs while the registry mutex is held, so it serializes with
/// [`SharedEnv::drop`] (which takes the same mutex around
/// `mdb_env_close`). A short backoff loop remains only to absorb the
/// `Arc`-decrement/Drop-start window that [`close_in_flight`] detects.
fn shared_env(
    path: &Path,
    read_only: bool,
    map_size: usize,
    no_sync: bool,
) -> Result<Arc<SharedEnv>, StoreError> {
    let registry = open_envs();
    // 1ms doubling to 256ms: ~500ms total, orders of magnitude past the
    // Arc-decrement window we retry against, while a genuine stuck close
    // still fails in bounded time rather than hanging the caller.
    let mut backoff = std::time::Duration::from_millis(1);
    for attempt in 0..10 {
        if attempt > 0 {
            // Sleep without the registry lock: the closing thread needs
            // that mutex to make progress, and unrelated paths keep
            // flowing through the registry.
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(std::time::Duration::from_millis(256));
        }
        let key = env_key(path);
        let mut map = registry.lock().unwrap_or_else(|p| p.into_inner());
        if close_in_flight(&map, &key) {
            // A teardown owns this path. Release the mutex so the closing
            // thread can take it, then look again.
            drop(map);
            continue;
        }
        if let Some((env, check)) = live_env(&map, &key, read_only, map_size, no_sync) {
            // Release the registry mutex before `env` can drop: on the
            // mismatch paths `check` is `Err`, so `env` is dropped when
            // `check.map(..)` discards it, and that drop must not
            // re-enter `SharedEnv::drop` while we still hold the mutex.
            drop(map);
            return check.map(|()| env);
        }
        let env = open_env(path, read_only, map_size, no_sync)?;
        let env = Arc::new(SharedEnv {
            inner: ManuallyDrop::new(env),
            key: env_key(path),
            dbis: Mutex::new(HashMap::new()),
            dbi_open: Mutex::new(()),
        });
        // Re-key by the now-existing file so later opens of the same file
        // through a different spelling collide correctly.
        map.insert(
            env.key.clone(),
            (Arc::downgrade(&env), !read_only, map_size, no_sync),
        );
        return Ok(env);
    }
    Err(StoreError::Backend(Box::new(std::io::Error::other(
        "LMDB environment teardown did not complete in time to reopen it",
    ))))
}

// ── store ─────────────────────────────────────────────────────────

/// An LMDB-backed store implementing both capability tiers.
///
/// Feature-gated behind `lmdb`, over the system liblmdb. Uses a single
/// file (`MDB_NOSUBDIR`), the default byte-lexicographic comparator, and
/// a writeable mapping (`MDB_WRITEMAP`) on writable handles.
///
/// LMDB caps an environment at 32 named tables (`MAX_DBS`). Writing to a
/// 33rd distinct table fails with [`StoreError::InvalidInput`]; the redb
/// backend has no such limit. Keep the total number of distinct table
/// names at or below 32 for cross-backend parity.
///
/// LMDB additionally forbids a second in-process open of the same
/// environment file; stores therefore share one environment per path
/// through a process-wide registry (see [`shared_env`]), matching the
/// other backends' open-many contract.
pub struct LmdbStore {
    env: Arc<SharedEnv>,
    read_only: bool,
}

impl LmdbStore {
    /// Open or create the store with a non-default map-size ceiling
    /// (`bytes` of virtual address space; LMDB commits it sparsely).
    ///
    /// `map_size` must be a multiple of the system page size; other
    /// values fail with [`StoreError::InvalidInput`].
    ///
    /// An open environment is never resized here, so the ceiling chosen
    /// at open time is fixed for the store's lifetime.  Use this instead
    /// of [`WriteStore::create`] when the 1 GiB default is too small; use
    /// one consistent ceiling for a given file across processes.
    pub fn create_with_map_size(path: &Path, map_size: usize) -> Result<Self, StoreError> {
        let env = shared_env(path, false, map_size, false)?;
        Ok(Self {
            env,
            read_only: false,
        })
    }

    /// Open the store read-only with a non-default map-size ceiling.
    ///
    /// [`ReadStore::open_read_only`] uses the 1 GiB default, which
    /// cannot reopen a store that was grown past that ceiling with
    /// [`LmdbStore::create_with_map_size`]: LMDB rejects a map size smaller
    /// than the data already on disk.  Pass the same (or a larger) ceiling
    /// used to create the store.
    ///
    /// `map_size` must be a multiple of the system page size; other values
    /// fail with [`StoreError::InvalidInput`].
    pub fn open_read_only_with_map_size(path: &Path, map_size: usize) -> Result<Self, StoreError> {
        let env = shared_env(path, true, map_size, false)?;
        Ok(Self {
            env,
            read_only: true,
        })
    }

    /// One-shot bulk loader for the raw (unframed) keyspace: writes every
    /// entry into the well-known [`RAW_TABLE`] in a single transaction
    /// using `MDB_APPEND`, into an environment opened with `MDB_NOSYNC`,
    /// and forces one sync after the final commit.
    ///
    /// This is the data-prep fast path (`oxpinyin-datagen`); the runtime
    /// read and training paths keep per-commit syncing and plain puts.
    /// `MDB_APPEND` lets LMDB skip the B-tree descent and spill each
    /// written key at the current tail, which is only correct when keys
    /// arrive in ascending order — LMDB returns `MDB_KEYEXIST` (or worse)
    /// otherwise, so the strictly-ascending precondition is checked up
    /// front and a violation is reported as [`StoreError::InvalidInput`]
    /// rather than surfacing as a mid-load backend error.
    ///
    /// The `MDB_NOSYNC` environment skips the per-commit fsync; the
    /// explicit forced sync after the commit restores durability before
    /// the environment is dropped, so the file on disk is complete once
    /// this function returns `Ok`. A caller holding the same file open
    /// through a normal (syncing) handle will be refused by the env
    /// registry, and vice versa.
    ///
    /// An existing file at `path` is left untouched (the caller replaces
    /// it); the store is not returned — the environment is closed when
    /// this function returns, leaving the path reopenable read-only.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the entries are not strictly ascending
    /// by key (including duplicate keys), a key is outside 1..=511 bytes,
    /// or the environment, transaction, or final sync fails.
    pub fn bulk_load_raw(path: &Path, entries: &[(Vec<u8>, Vec<u8>)]) -> Result<(), StoreError> {
        for window in entries.windows(2) {
            if window[0].0 >= window[1].0 {
                return Err(StoreError::InvalidInput(
                    "bulk load entries must be strictly ascending by key",
                ));
            }
        }
        for (key, _) in entries {
            if key.is_empty() || key.len() > MAX_KEY_LEN {
                return Err(StoreError::InvalidInput("key length must be 1..=511 bytes"));
            }
        }
        let env = shared_env(path, false, MAP_SIZE, true)?;
        if entries.is_empty() {
            return Ok(());
        }
        let txn = env.inner.write_txn()?;
        let mut wtxn = LmdbWriteTxn {
            env: &env,
            txn,
            pending_cache: Vec::new(),
            dbi_open_guard: RefCell::new(None),
        };
        let result = {
            let dbi = wtxn.ensure_created(RAW_TABLE)?;
            let mut outcome = Ok(());
            for (key, value) in entries {
                if let Err(e) = db_put(&wtxn.txn, dbi, key, value, ffi::MDB_APPEND) {
                    outcome = Err(e);
                    break;
                }
            }
            outcome
        };
        match result {
            Ok(()) => {
                let LmdbWriteTxn {
                    env,
                    txn,
                    pending_cache,
                    dbi_open_guard,
                } = wtxn;
                txn.commit()?;
                env.promote_created(&pending_cache);
                // Release `dbi_open` before the forced sync: the DBI the
                // txn opened is committed env-wide now, so other txns may
                // call `mdb_dbi_open` while the sync runs.
                drop(dbi_open_guard);
                // The environment was opened with MDB_NOSYNC, so the
                // commit above is not yet durable; flush it once here,
                // while this loader is still the only writer.
                env.inner.force_sync()?;
                Ok(())
            }
            Err(error) => {
                let LmdbWriteTxn {
                    txn,
                    dbi_open_guard,
                    ..
                } = wtxn;
                txn.abort();
                drop(dbi_open_guard);
                Err(error)
            }
        }
    }
}

impl ReadStore for LmdbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let env = shared_env(path, true, MAP_SIZE, false)?;
        Ok(Self {
            env,
            read_only: true,
        })
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        let Some(dbi) = self.env.database(table)? else {
            return Ok(None);
        };
        let txn = self.env.read_txn()?;
        db_get(&txn, dbi, key)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let Some(dbi) = self.env.database(table)? else {
            return Ok(());
        };
        let txn = self.env.read_txn()?;
        db_for_each(&txn, dbi, visit)
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        validate_table_name(table)?;
        if is_empty_upper_bound(hi) {
            return Ok(());
        }
        let Some(dbi) = self.env.database(table)? else {
            return Ok(());
        };
        let txn = self.env.read_txn()?;
        db_range(&txn, dbi, normalize_bound(lo), normalize_bound(hi), visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let Some(dbi) = self.env.database(table)? else {
            return Ok(true);
        };
        let txn = self.env.read_txn()?;
        db_is_empty(&txn, dbi)
    }
}

impl crate::RawReadStore for LmdbStore {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        self.get(crate::RAW_TABLE, key)
    }
}

impl WriteStore for LmdbStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        let env = shared_env(path, false, MAP_SIZE, false)?;
        Ok(Self {
            env,
            read_only: false,
        })
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let txn = self.env.write_txn()?;
        let mut wtxn = LmdbWriteTxn {
            env: &self.env,
            txn,
            pending_cache: Vec::new(),
            dbi_open_guard: RefCell::new(None),
        };
        match f(&mut wtxn) {
            Ok(result) => {
                let LmdbWriteTxn {
                    env,
                    txn,
                    pending_cache,
                    dbi_open_guard,
                } = wtxn;
                txn.commit()?;
                // Only after the write txn's `mdb_txn_end` commits are
                // the DBIs it opened valid env-wide; promote them into
                // the shared cache now so subsequent reads and writes
                // find them on the fast path.
                env.promote_created(&pending_cache);
                // Release `dbi_open` last: LMDB's exclusion rule keeps
                // no other txn's `mdb_dbi_open` from running until the
                // opening txn's `mdb_txn_end` has fully returned, and
                // that only completes above.
                drop(dbi_open_guard);
                Ok(result)
            }
            Err(error) => {
                let LmdbWriteTxn {
                    env: _,
                    txn,
                    pending_cache: _,
                    dbi_open_guard,
                } = wtxn;
                // Abort under the guard: `mdb_txn_end` walks
                // `me_dbxs` and `free()`s every `DB_NEW` slot the
                // txn opened (`mdb.c:3399-3405`); a concurrent
                // reader's `mdb_dbi_open` scan must be blocked while
                // that free runs.
                txn.abort();
                drop(dbi_open_guard);
                // Aborted DBIs were freed by `mdb_txn_end`; do not
                // touch the shared cache.
                Err(error)
            }
        }
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        // LMDB reclaims freed pages in place, so compaction itself does no
        // work here. redb's compaction can no longer fail either, now that
        // no read view outlives a call, so both backends succeed.
        Ok(())
    }
}

// ── write transaction ─────────────────────────────────────────────

struct LmdbWriteTxn<'a> {
    env: &'a SharedEnv,
    txn: Txn<'a>,
    /// Tables the closure `put`s into this write txn that were not
    /// already in the shared cache. Promoted into the cache in
    /// [`WriteStore::write`] on commit; discarded on abort, because
    /// LMDB frees a `DB_NEW` DBI when the txn ends without commit.
    pending_cache: Vec<(String, ffi::MDB_dbi)>,
    /// Populated the first time this txn hits a cache miss and calls
    /// `mdb_dbi_open` (via [`Self::open_existing`] or
    /// [`Self::ensure_created`]). LMDB's spec requires the opening txn
    /// to finish (commit or abort) before any other txn on the env may
    /// call `mdb_dbi_open`, so [`WriteStore::write`] holds the guard
    /// until after the commit or abort returns and only then drops it.
    /// A [`RefCell`] gives interior mutability so the `&self` read
    /// methods on [`WriteTxn`] can lazily acquire the guard too.
    dbi_open_guard: RefCell<Option<MutexGuard<'a, ()>>>,
}

impl LmdbWriteTxn<'_> {
    /// Ensure this txn holds the env's [`SharedEnv::dbi_open`] guard
    /// before we call `mdb_dbi_open`. First-miss on the txn acquires
    /// the guard; every later miss finds it already held and just
    /// keeps it.
    ///
    /// # Nesting
    ///
    /// Once this method returns with the guard populated, a nested
    /// call on the same thread into a store on the same path that
    /// *also* hits a cache miss will deadlock — `std::sync::Mutex`
    /// is not reentrant. Cache-hit re-entry is safe **for reads
    /// only**: a nested [`WriteStore::write`] on the same path
    /// blocks on LMDB's single-writer mutex regardless of cache
    /// state, and that deadlock lives in LMDB, not in
    /// [`SharedEnv::dbi_open`]. See the [`SharedEnv`] docblock for
    /// the wider context; oxpinyin's own code neither nests a
    /// fresh-table open nor nests a write on the same path inside a
    /// write closure.
    fn hold_dbi_open(&self) {
        let mut slot = self.dbi_open_guard.borrow_mut();
        if slot.is_none() {
            *slot = Some(self.env.lock_dbi_open());
        }
    }

    /// Look the table up in the shared cache or open it through the
    /// write txn without creating it. Read-side ops on the write txn
    /// never need `MDB_CREATE` — an absent table just answers "empty"
    /// per the trait contract. On a cache miss the open calls
    /// `mdb_dbi_open`, so hold [`SharedEnv::dbi_open`] for the rest of
    /// the txn's life.
    fn open_existing(&self, table: &str) -> Result<Option<ffi::MDB_dbi>, StoreError> {
        {
            let cache = self.env.lock_dbis();
            if let Some(dbi) = cache.get(table) {
                return Ok(Some(*dbi));
            }
        }
        self.hold_dbi_open();
        // Re-check under the guard: another writer's `promote_created`
        // may have raced us into the miss path and already inserted the
        // handle. This is unlikely (write txns on one env serialize
        // through LMDB's writer mutex and would have blocked us on
        // `write_txn()`), but the check keeps the invariant tight for
        // any cross-env-instance sharing.
        {
            let cache = self.env.lock_dbis();
            if let Some(dbi) = cache.get(table) {
                return Ok(Some(*dbi));
            }
        }
        dbi_open(&self.txn, table, false)
    }

    /// Return the cached handle for `table` if positive, else open it
    /// with `MDB_CREATE` on this txn (staging the new DBI in
    /// [`Self::pending_cache`] for promotion after commit). The
    /// returned handle is *not* published to the shared cache from
    /// here — that would leak a dangling DBI on abort, since LMDB
    /// frees the slot when a `DB_NEW` txn ends without commit.
    fn ensure_created(&mut self, table: &str) -> Result<ffi::MDB_dbi, StoreError> {
        {
            let cache = self.env.lock_dbis();
            if let Some(dbi) = cache.get(table) {
                return Ok(*dbi);
            }
        }
        // We're about to call `mdb_dbi_open` on the write txn; hold
        // the env's `dbi_open` guard until the txn commits or aborts.
        self.hold_dbi_open();
        {
            let cache = self.env.lock_dbis();
            if let Some(dbi) = cache.get(table) {
                return Ok(*dbi);
            }
        }
        let dbi = dbi_open(&self.txn, table, true)?
            .ok_or_else(|| success_without("table it was asked to create"))?;
        // Stage the handle for post-commit promotion.
        if !self.pending_cache.iter().any(|(name, _)| name == table) {
            self.pending_cache.push((table.to_owned(), dbi));
        }
        Ok(dbi)
    }
}

impl WriteTxn for LmdbWriteTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        let Some(dbi) = self.open_existing(table)? else {
            return Ok(None);
        };
        db_get(&self.txn, dbi, key)
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        if key.is_empty() || key.len() > MAX_KEY_LEN {
            return Err(StoreError::InvalidInput("key length must be 1..=511 bytes"));
        }
        let dbi = self.ensure_created(table)?;
        db_put(&self.txn, dbi, key, value, 0)
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let Some(dbi) = self.open_existing(table)? else {
            return Ok(());
        };
        db_del(&self.txn, dbi, key)
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        validate_table_name(table)?;
        if is_empty_upper_bound(hi) {
            return Ok(());
        }
        let Some(dbi) = self.open_existing(table)? else {
            return Ok(());
        };
        db_range(
            &self.txn,
            dbi,
            normalize_bound(lo),
            normalize_bound(hi),
            visit,
        )
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let Some(dbi) = self.open_existing(table)? else {
            return Ok(());
        };
        db_for_each(&self.txn, dbi, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let Some(dbi) = self.open_existing(table)? else {
            return Ok(true);
        };
        db_is_empty(&self.txn, dbi)
    }
}
