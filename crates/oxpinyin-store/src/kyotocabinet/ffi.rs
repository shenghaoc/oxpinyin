//! Safe RAII wrappers over the generated Kyoto Cabinet declarations.
//!
//! This is the crate's only Kyoto Cabinet FFI surface. Everything above
//! it is safe Rust.
//!
//! The generated declarations are `unsafe extern "C"`, which the
//! workspace's `unsafe_code = "deny"` would otherwise reject; the allow
//! is scoped to this module and `super`, under the same backend waiver
//! the tkrzw shim carries. Waived safety is not waived correctness: every
//! block below states its invariant, and the shared read and write suites
//! gate this backend like any other.
//!
//! # The four hazards
//!
//! **Unwinding.** Kyoto Cabinet is C++ internally, but this backend calls
//! it only through `kclangc.h`'s C entry points, and Rust is always the
//! caller. No Rust panic unwinds through a foreign frame. [`Db`],
//! [`Cursor`] and [`Buf`] release their resources on an unwind exactly as
//! on a normal return.
//!
//! **`Send`/`Sync`.** [`Db`] holds a raw pointer, so the compiler derives
//! neither — this module supplies both explicitly with `unsafe impl`s,
//! whose SAFETY note (near the bottom of the module) verifies the
//! underlying contract against the Kyoto Cabinet 1.2.80 headers, file
//! and symbol at a time: every operation takes the database object's own
//! reader-writer lock, transactions serialize on that lock, and the error
//! state is thread-specific data, not shared state (see
//! `docs/findings/kyotocabinet-backend.md` for the verification record).
//!
//! **Buffer ownership — and why this differs from Berkeley DB.** The
//! brief for this backend carried Berkeley DB's cursor hazard over
//! ("do not hold the returned pointer past a cursor move"). That is the
//! wrong shape for Kyoto Cabinet, and building to it would have produced
//! a bug. `kcdbget`, `kccurgetkey`, `kccurgetvalue` and `kccurget` each
//! return a **caller-owned, freshly allocated** region — "the region of
//! the return value should be released with the kcfree function when it
//! is no longer in use" (`kclangc.h:577-578`, `:923-924`, `:942-943`).
//! Nothing expires when the cursor moves; the failure mode is a *leak*,
//! or a free through the wrong allocator. So the answer here is
//! ownership, not a lifetime bound: [`Buf`] owns the region and its
//! `Drop` calls `kcfree` — never `libc::free`, which would be freeing
//! across allocators.
//!
//! `kccurget`'s value pointer is an **interior** pointer into the same
//! allocation as its key, so [`Record`] holds one [`Buf`] and an offset,
//! and frees once.
//!
//! **Null returns.** Every one of those four functions returns `NULL` on
//! a miss *or* an error, and the two are told apart only by the error
//! code (`kcdbecode`, named by `kcecodename`). `kcdbnew` and `kcdbcursor`
//! return `NULL` on allocation failure. All are checked; nothing is
//! dereferenced unconditionally.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::ops::Deref;
use std::os::raw::c_char;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use crate::StoreError;

/// The generated Kyoto Cabinet declarations.
///
/// Written by `build.rs` from the system `kclangc.h` on every build; see
/// there for why they are not checked in.
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    missing_docs,
    dead_code,
    clippy::all
)]
mod sys {
    include!(concat!(env!("OUT_DIR"), "/kc_bindings.rs"));
}

/// The Kyoto Cabinet major.minor this backend's format survey covers.
const SUPPORTED: &str = "1.2";

/// The linked library's version string (`KCVERSION`).
pub(crate) fn runtime_version() -> String {
    // SAFETY: `KCVERSION` is a `const char* const` pointing at a static
    // NUL-terminated string in the library.
    unsafe { CStr::from_ptr(sys::KCVERSION) }
        .to_string_lossy()
        .into_owned()
}

/// Refuses a Kyoto Cabinet whose on-disk format this backend has not
/// surveyed.
///
/// This backend *writes* user profiles that the user's own libpinyin must
/// read back, so an unsurveyed version is an error at open rather than a
/// risk taken at write.
pub(crate) fn check_runtime_version() -> Result<(), StoreError> {
    let version = runtime_version();
    let major_minor: String = version.split('.').take(2).collect::<Vec<_>>().join(".");
    if major_minor == SUPPORTED {
        return Ok(());
    }
    Err(StoreError::Backend(
        format!(
            "unsupported Kyoto Cabinet {version}: this backend is surveyed against \
             {SUPPORTED}.x, and it writes user profiles that libpinyin itself has to \
             read back"
        )
        .into(),
    ))
}

/// `KCENOREC` as the error-code functions return it.
///
/// bindgen types the `enum` constant as `u32` and the accessors as `i32`;
/// the value is small and positive, so the conversion is exact.
fn no_record_code() -> i32 {
    i32::try_from(sys::KCENOREC).unwrap_or(i32::MAX)
}

/// Which Kyoto Cabinet class a path opens as.
///
/// The C API is `PolyDB`, which picks the class from the path — see
/// [`Db::open`] for why that matters so much here. The store's peer
/// backend opens `TreeDB` for ordered, ranged access; the enum is kept
/// as an enum so a caller could not open the wrong class by handing a
/// bare suffix string.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DbType {
    /// `TreeDB`, ordered by the record comparator.
    Tree,
}

impl DbType {
    /// The `#type=` value `PolyDB` recognises for this class.
    const fn tuning(self) -> &'static str {
        match self {
            Self::Tree => "kct",
        }
    }
}

/// An owned region Kyoto Cabinet allocated, released with `kcfree`.
///
/// The whole answer to the buffer-ownership hazard: the region is ours,
/// and it must go back through Kyoto Cabinet's allocator rather than
/// Rust's or libc's.
pub(crate) struct Buf {
    ptr: *mut c_char,
    len: usize,
}

impl Buf {
    /// Wraps a region Kyoto Cabinet returned, or `None` for a null.
    ///
    /// # Safety
    ///
    /// `ptr` must be null, or a region of `len` bytes returned by a Kyoto
    /// Cabinet function documented as releasable with `kcfree`, and not
    /// yet released.
    unsafe fn from_raw(ptr: *mut c_char, len: usize) -> Option<Self> {
        (!ptr.is_null()).then_some(Self { ptr, len })
    }
}

impl Deref for Buf {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: the constructor's contract guarantees `ptr` addresses
        // `len` initialised bytes, and `Drop` is the only thing that
        // releases them, so the region outlives this borrow of `self`.
        unsafe { std::slice::from_raw_parts(self.ptr.cast::<u8>(), self.len) }
    }
}

impl Drop for Buf {
    fn drop(&mut self) {
        // SAFETY: `ptr` came from a Kyoto Cabinet call that documents
        // `kcfree` as its release function, and this is its last use.
        // Freeing it with `free` or Rust's allocator instead would be a
        // cross-allocator free.
        unsafe { sys::kcfree(self.ptr.cast::<std::ffi::c_void>()) }
    }
}

/// An owned Kyoto Cabinet database handle.
pub(crate) struct Db {
    handle: *mut sys::KCDB,
    read_only: bool,
}

// SAFETY: verified against the Kyoto Cabinet 1.2.80 headers (the version
// floor this backend builds against), file and symbol at a time:
//
// * Every record operation this backend issues — `kcdbget`/`kcdbset`/
//   `kcdbremove` (all `accept` underneath), `kcdbcount`, `kcdbsync`, and
//   the cursor calls — is documented as "performed atomically and other
//   threads accessing the same record are blocked" (`kchashdb.h`,
//   `accept`'s @note; the same note is on `kcpolydb.h`'s and
//   `kcplantdb.h`'s operations) and implemented by taking the database
//   object's own reader-writer lock (`RWLock mlock_`, `kchashdb.h` and
//   `kcplantdb.h` private members; `ScopedRWLock` at the top of every
//   method body, e.g. HashDB `count`, `synchronize`, and `Cursor::jump`).
// * `kcdbbegintran` is `begin_transaction(false)`: it takes the writer
//   lock and spins (`Thread::yield`/`chill`) while another transaction is
//   live, holding the lock until `kcdbendtran` — so concurrent `write`
//   transactions serialize inside the library rather than racing, and
//   readers exclude with the whole transaction window.
// * The error accessors are NOT shared state: HashDB holds its error as
//   thread-specific data (`TSD<Error> error_`, `kchashdb.h`; `TSD` in
//   `kcthread.h` gives each thread its own instance), and TreeDB and
//   PolyDB forward to that inner database (`Error error() const { return
//   db_.error(); }` in `kcplantdb.h`; PolyDB's `set_error` delegates when
//   open). A thread reading `kcdbecode`/`kcdbemsg` therefore observes its
//   own thread's last failure — no cross-thread race. This wrapper reads
//   them immediately after the failing call on the same thread, before
//   any other call could overwrite the slot.
// * PolyDB's own plain `Error error_` member is touched only in the
//   TYPEVOID (unopened) state; a handle exists here only after `open`
//   succeeded, and `kcdbclose`/`kcdbdel` run in `Drop`, which holds the
//   handle exclusively. Open completes before the handle exists to share.
// * The class-level contract (`kcdb.h`, `BasicDB`'s @note) forbids two
//   database objects in one process opening the same file, and sharing a
//   database object with child processes — cross-thread sharing of one
//   object is inside the supported model.
//
// That is what lets the store keep `get` and `write` on `&self`, and what
// the user-store registry's `static Mutex<… DefaultStore …>` requires of
// a default backend. libpinyin itself (`flexible_ngram_kyotodb.h`,
// `kyotodb_utils.h`) uses the same C++ objects single-threaded with no
// added locking, so it neither contradicts this nor exercises it — the
// guarantee is Kyoto Cabinet's own, cited above.
unsafe impl Send for Db {}
unsafe impl Sync for Db {}

impl Db {
    /// Opens `path` as `db_type`.
    ///
    /// # The `#type=` suffix, and why it is not optional
    ///
    /// The C API is `PolyDB`, which chooses the database class from the
    /// **path suffix**: `.kch` is a file hash database, `.kct` a file
    /// tree database, "otherwise, this function fails"
    /// (`kclangc.h:312-320`).
    ///
    /// libpinyin's files are named `bigram.db` and `user_bigram.db`
    /// (`src/pinyin_internal.h:56-58`) — compile-time constants that do
    /// not vary with the DBM backend it was built against, so a
    /// Kyoto-Cabinet-built libpinyin ships **no `.kch` or `.kct` file at
    /// all**. Measured: `kcdbopen(db, "bigram.db", …)` fails with
    /// `invalid operation`.
    ///
    /// `PolyDB` also accepts tuning parameters after a `#`, among them
    /// `type=` (`kcpolydb.h:496-515`), so `bigram.db#type=kch` opens
    /// libpinyin's actual file as a hash database. That override is the
    /// only way the C API can read these files, which is why every open
    /// here goes through it rather than trusting a name.
    pub(crate) fn open(
        path: &Path,
        db_type: DbType,
        read_only: bool,
        create: bool,
    ) -> Result<Self, StoreError> {
        check_runtime_version()?;

        // The path is handed to Kyoto Cabinet as bytes, with the tuning
        // parameter appended. A `#` in the path itself would be read as
        // the start of tuning parameters, so it is refused rather than
        // silently changing which database is opened.
        let bytes = path.as_os_str().as_bytes();
        if bytes.contains(&b'#') {
            return Err(StoreError::InvalidInput(
                "store path contains '#', which Kyoto Cabinet reads as the start of \
                 tuning parameters",
            ));
        }
        let mut spec = bytes.to_vec();
        spec.extend_from_slice(b"#type=");
        spec.extend_from_slice(db_type.tuning().as_bytes());
        let spec =
            CString::new(spec).map_err(|_| StoreError::InvalidInput("store path contains NUL"))?;

        // SAFETY: takes no arguments and returns either a fresh handle or
        // null.
        let handle = unsafe { sys::kcdbnew() };
        if handle.is_null() {
            return Err(StoreError::Backend(
                "kcdbnew returned null (allocation failure)".into(),
            ));
        }
        // Owned from here: every early return must release it, which
        // `this` does by construction.
        let this = Self { handle, read_only };

        let mode = if read_only {
            sys::KCOREADER
        } else if create {
            sys::KCOWRITER | sys::KCOCREATE
        } else {
            sys::KCOWRITER
        };
        // SAFETY: the handle is live and `spec` outlives the call.
        if unsafe { sys::kcdbopen(this.handle, spec.as_ptr(), mode) } == 0 {
            return Err(this.error("kcdbopen"));
        }
        Ok(this)
    }

    /// The library's own account of the last failure on this handle.
    fn error(&self, what: &str) -> StoreError {
        // SAFETY: the handle is live; both calls are defined for a handle
        // in any state, including one that failed to open.
        let (code, message) = unsafe {
            let code = sys::kcdbecode(self.handle);
            let name = CStr::from_ptr(sys::kcecodename(code))
                .to_string_lossy()
                .into_owned();
            let message = CStr::from_ptr(sys::kcdbemsg(self.handle))
                .to_string_lossy()
                .into_owned();
            (name, message)
        };
        // Kyoto Cabinet reports filesystem trouble through its own codes
        // rather than errno, so everything lands on the backend arm; the
        // library's name for the code is preserved verbatim.
        StoreError::Backend(format!("{what}: {code}: {message}").into())
    }

    /// Whether the last operation failed because the key was absent
    /// rather than because something went wrong.
    ///
    /// Both are a null return, so the code is the only way to tell them
    /// apart — and treating an error as a miss would turn a damaged file
    /// into silently empty data.
    fn last_was_no_record(&self) -> bool {
        // SAFETY: the handle is live and `kcdbecode` is defined for any
        // state.
        unsafe { sys::kcdbecode(self.handle) == no_record_code() }
    }

    /// Whether this handle was opened read-only.
    pub(crate) const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Point read. `None` when the key is absent.
    pub(crate) fn get(&self, key: &[u8]) -> Result<Option<Buf>, StoreError> {
        let mut size = 0_usize;
        // SAFETY: the handle is live and `key` outlives the call. The
        // returned region is ours to release.
        let raw = unsafe {
            sys::kcdbget(
                self.handle,
                key.as_ptr().cast::<c_char>(),
                key.len(),
                &raw mut size,
            )
        };
        // SAFETY: `raw` is null or a `kcfree`-able region of `size` bytes,
        // which is exactly the constructor's contract.
        match unsafe { Buf::from_raw(raw, size) } {
            Some(buf) => Ok(Some(buf)),
            None if self.last_was_no_record() => Ok(None),
            None => Err(self.error("kcdbget")),
        }
    }

    /// Insert or overwrite.
    pub(crate) fn set(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        // SAFETY: the handle is live; both slices outlive the call, and
        // Kyoto Cabinet copies out of them before returning.
        let ok = unsafe {
            sys::kcdbset(
                self.handle,
                key.as_ptr().cast::<c_char>(),
                key.len(),
                value.as_ptr().cast::<c_char>(),
                value.len(),
            )
        };
        if ok == 0 {
            return Err(self.error("kcdbset"));
        }
        Ok(())
    }

    /// Remove `key`; absent is not an error.
    pub(crate) fn remove(&self, key: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        // SAFETY: the handle is live and `key` outlives the call.
        let ok = unsafe { sys::kcdbremove(self.handle, key.as_ptr().cast::<c_char>(), key.len()) };
        if ok == 0 && !self.last_was_no_record() {
            return Err(self.error("kcdbremove"));
        }
        Ok(())
    }

    /// Flush. `hard` also asks for the physical device.
    pub(crate) fn sync(&self, hard: bool) -> Result<(), StoreError> {
        if self.read_only {
            return Ok(());
        }
        // SAFETY: the handle is live; the post-processor callback and its
        // opaque argument are both null, which the API documents as "no
        // callback".
        let ok = unsafe { sys::kcdbsync(self.handle, i32::from(hard), None, ptr::null_mut()) };
        if ok == 0 {
            return Err(self.error("kcdbsync"));
        }
        Ok(())
    }

    /// Begins a transaction.
    ///
    /// Unlike the Berkeley DB backend — where libpinyin's environment-less
    /// opens leave no transaction to use — Kyoto Cabinet gives a
    /// standalone handle real transactions, so this backend's atomic
    /// writes are the library's own rather than a buffered imitation.
    pub(crate) fn begin_transaction(&self) -> Result<(), StoreError> {
        // SAFETY: the handle is live. `hard = 0` asks for a transaction
        // durable against process crash but not against power loss, which
        // matches what the commit's `sync(false)` then provides.
        if unsafe { sys::kcdbbegintran(self.handle, 0) } == 0 {
            return Err(self.error("kcdbbegintran"));
        }
        Ok(())
    }

    /// Ends a transaction, committing or rolling back.
    pub(crate) fn end_transaction(&self, commit: bool) -> Result<(), StoreError> {
        // SAFETY: the handle is live and inside a transaction begun by
        // `begin_transaction`.
        if unsafe { sys::kcdbendtran(self.handle, i32::from(commit)) } == 0 {
            return Err(self.error("kcdbendtran"));
        }
        Ok(())
    }

    /// Opens a cursor, which borrows this database.
    pub(crate) fn cursor(&self) -> Result<Cursor<'_>, StoreError> {
        // SAFETY: the handle is live.
        let handle = unsafe { sys::kcdbcursor(self.handle) };
        if handle.is_null() {
            return Err(self.error("kcdbcursor"));
        }
        Ok(Cursor {
            handle,
            _db: PhantomData,
        })
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        // A close failure has nowhere to go in `Drop`, and the handle must
        // be deleted either way. Callers who need to know a flush
        // succeeded call `sync`, which reports.
        //
        // SAFETY: the handle is live and owned; `kcdbdel` is its last use,
        // and Kyoto Cabinet requires the close before the delete.
        unsafe {
            sys::kcdbclose(self.handle);
            sys::kcdbdel(self.handle);
        }
    }
}

/// A cursor over one database, deleted on drop.
pub(crate) struct Cursor<'db> {
    handle: *mut sys::KCCUR,
    _db: PhantomData<&'db Db>,
}

/// One record read from a cursor.
///
/// Owns the single allocation `kccurget` returns. The value pointer that
/// call hands back is an **interior** pointer into the same region as the
/// key (`kclangc.h:931-943`), so this holds one [`Buf`] and an offset and
/// frees exactly once — freeing the value separately would be a free of
/// an interior pointer.
pub(crate) struct Record {
    region: Buf,
    key_len: usize,
    value_at: usize,
    value_len: usize,
}

impl Record {
    /// The record's key.
    pub(crate) fn key(&self) -> &[u8] {
        &self.region[..self.key_len]
    }

    /// The record's value.
    pub(crate) fn value(&self) -> &[u8] {
        &self.region[self.value_at..self.value_at + self.value_len]
    }
}

impl Cursor<'_> {
    /// Positions at the smallest key at or after `key`; `Ok(false)` when
    /// there is none.
    pub(crate) fn jump_to(&mut self, key: &[u8]) -> Result<bool, StoreError> {
        // SAFETY: the cursor handle is live and `key` outlives the call.
        let ok =
            unsafe { sys::kccurjumpkey(self.handle, key.as_ptr().cast::<c_char>(), key.len()) };
        if ok != 0 {
            return Ok(true);
        }
        self.positioning_outcome("kccurjumpkey")
    }

    /// A positioning call (`kccurjump` / `kccurjumpkey`) returned false:
    /// `Ok(false)` when the cursor's code is "no record" — an empty database
    /// or a key past the end — otherwise the backend error the code names.
    /// The same split [`Cursor::next`] makes on a null region, so a genuine
    /// failure is never silently reported as an empty range.
    fn positioning_outcome(&self, call: &str) -> Result<bool, StoreError> {
        // SAFETY: the cursor handle is live.
        let code = unsafe { sys::kccurecode(self.handle) };
        if code == no_record_code() {
            return Ok(false);
        }
        // SAFETY: `kcecodename` returns a static NUL-terminated string for
        // any code.
        let name = unsafe { CStr::from_ptr(sys::kcecodename(code)) }
            .to_string_lossy()
            .into_owned();
        Err(StoreError::Backend(format!("{call}: {name}").into()))
    }

    /// Reads the record at the cursor, then steps past it.
    ///
    /// `None` at the end of the database. The returned [`Record`] owns its
    /// memory, so — unlike the Berkeley DB backend's borrowed rows — it
    /// stays valid after the cursor has moved on.
    pub(crate) fn next(&mut self) -> Result<Option<Record>, StoreError> {
        let mut key_len = 0_usize;
        let mut value_ptr: *const c_char = ptr::null();
        let mut value_len = 0_usize;
        // SAFETY: the cursor handle is live; the three out-parameters are
        // live locals. `step = 1` advances the cursor after reading,
        // which is what makes this a one-call iteration. The returned
        // region is ours to release.
        let raw = unsafe {
            sys::kccurget(
                self.handle,
                &raw mut key_len,
                &raw mut value_ptr,
                &raw mut value_len,
                1,
            )
        };
        if raw.is_null() {
            // A null is the end of the database as well as a failure;
            // the cursor's own code separates them.
            //
            // SAFETY: the cursor handle is live.
            let code = unsafe { sys::kccurecode(self.handle) };
            if code == no_record_code() {
                return Ok(None);
            }
            // SAFETY: `kcecodename` returns a static NUL-terminated string
            // for any code.
            let name = unsafe { CStr::from_ptr(sys::kcecodename(code)) }
                .to_string_lossy()
                .into_owned();
            return Err(StoreError::Backend(format!("kccurget: {name}").into()));
        }
        // The value pointer is interior to the returned region; its offset
        // is what `Record` stores so the whole thing is freed once.
        let value_at = (value_ptr as usize).saturating_sub(raw as usize);
        // Kyoto Cabinet appends a NUL after each of the key and the value,
        // so the allocation is at least this long.
        let region_len = value_at + value_len;
        // SAFETY: `raw` is non-null and is a `kcfree`-able region; the
        // length covers the key, the separator NUL and the value, which
        // is what the two sizes and the interior offset bound.
        let region = unsafe { Buf::from_raw(raw, region_len) }.ok_or_else(|| {
            StoreError::Backend("kccurget returned a null region after a success".into())
        })?;
        Ok(Some(Record {
            region,
            key_len,
            value_at,
            value_len,
        }))
    }
}

impl Drop for Cursor<'_> {
    fn drop(&mut self) {
        // SAFETY: the cursor handle is live and owned, its database
        // outlives it by the `'db` bound, and this is its last use.
        unsafe { sys::kccurdel(self.handle) }
    }
}
