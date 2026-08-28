//! Safe RAII wrappers over the generated Berkeley DB declarations.
//!
//! This is the crate's only Berkeley DB FFI surface. Everything above it
//! — [`super::BdbStore`], [`super::compat`] — is safe Rust.
//!
//! The generated declarations are `unsafe extern "C"`, which the
//! workspace's `unsafe_code = "deny"` would otherwise reject; the allow
//! is scoped to this module and `super`'s two `Send`-related notes, under
//! the same backend waiver the tkrzw shim carries. Waived safety is not
//! waived correctness: every block below states its invariant, and the
//! shared read and write suites gate this backend like any other.
//!
//! # The four hazards, and where each is answered
//!
//! **Unwinding.** libdb is C, and Rust calls into it rather than the
//! reverse, so no Rust panic ever unwinds through a C frame here. The
//! [`Db`] and [`Cursor`] `Drop` impls close their handles on an unwind
//! just as on a normal return.
//!
//! **`Send`/`Sync`.** [`Db`] holds a raw pointer, so it is `!Send` and
//! `!Sync` by construction — the compiler derives neither, and this
//! module implements neither. Handles are opened without `DB_THREAD`,
//! exactly as libpinyin opens them (`ngram_bdb.cpp`,
//! `phrase_large_table3_bdb.cpp`), which is what permits the borrowed
//! reads below: `DB_THREAD` would require `DB_DBT_MALLOC`,
//! `DB_DBT_REALLOC` or `DB_DBT_USERMEM` on every `DBT` and so a copy of
//! every record. A handle therefore belongs to one thread for its
//! lifetime. `docs/findings/berkeleydb-backend.md` records what making
//! this type `Send` would cost.
//!
//! **Cursor lifetimes.** [`Cursor::get`] takes `&mut self` and returns a
//! [`Record`] borrowing `&self`, so the borrow checker rejects a second
//! `get` — or a `close`, or dropping the cursor — while a record is
//! still held. The rule that libdb states in prose ("the memory is valid
//! only until the next call on this cursor") is a compile error here, not
//! a comment.
//!
//! **Null returns.** `db_create` reports allocation failure by leaving
//! its out-parameter null with a zero return in some builds, and every
//! method on a `DB` is a struct member function pointer that bindgen
//! types as `Option<unsafe extern "C" fn ...>`. Both are checked: the
//! pointer with an explicit test, the members through [`method`], which
//! turns a null member into an error rather than a call through null.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::ptr;

use crate::StoreError;

/// The generated Berkeley DB declarations.
///
/// Written by `build.rs` from the system `db.h` on every build; see there
/// for why they are not checked in.
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    missing_docs,
    dead_code,
    clippy::all
)]
mod sys {
    include!(concat!(env!("OUT_DIR"), "/bdb_bindings.rs"));
}

pub(crate) use sys::DBTYPE;
/// `DB_BTREE` — bindgen names the `DBTYPE` enumerators with a type prefix.
pub(crate) const DB_BTREE: DBTYPE = sys::DBTYPE_DB_BTREE;
/// `DB_HASH`, the type libpinyin opens `bigram.db` as.
pub(crate) const DB_HASH: DBTYPE = sys::DBTYPE_DB_HASH;

/// The Berkeley DB major.minor this backend's format survey covers.
///
/// 5.3 is what every distro pins (5.3.28 is the last BSD-licensed
/// release) and what libpinyin build-depends on, so it is what wrote the
/// files this backend reads. The check is at run time rather than compile
/// time because `db.h` and the shared library can disagree — a header
/// upgrade without a matching runtime, or the reverse — and it is the
/// runtime that owns the on-disk format.
const SUPPORTED_MAJOR: i32 = 5;
const SUPPORTED_MINOR: i32 = 3;

/// The linked library's `major.minor.patch`.
pub(crate) fn runtime_version() -> (i32, i32, i32) {
    let (mut major, mut minor, mut patch) = (0, 0, 0);
    // SAFETY: `db_version` writes three `int`s through the pointers it is
    // given and returns a static string we ignore. The pointers are to
    // live locals of exactly that type.
    unsafe {
        sys::db_version(&raw mut major, &raw mut minor, &raw mut patch);
    }
    (major, minor, patch)
}

/// Refuses a libdb whose on-disk format this backend has not surveyed.
///
/// A newer Berkeley DB reads a 5.3 file, but this backend also *writes*
/// user profiles that the user's own libpinyin must read back, and it
/// decodes struct fields whose layout is version-specific. Guessing there
/// is how a profile gets corrupted silently, so an unsurveyed version is
/// an error at open rather than a risk taken at write.
pub(crate) fn check_runtime_version() -> Result<(), StoreError> {
    let (major, minor, _) = runtime_version();
    if (major, minor) == (SUPPORTED_MAJOR, SUPPORTED_MINOR) {
        return Ok(());
    }
    Err(StoreError::Backend(
        format!(
            "unsupported Berkeley DB {major}.{minor}: this backend is surveyed against \
             {SUPPORTED_MAJOR}.{SUPPORTED_MINOR} (the release every distro pins and \
             libpinyin build-depends on), and it writes user profiles that libpinyin \
             itself has to read back"
        )
        .into(),
    ))
}

/// Turns a libdb return code into a [`StoreError`], preserving the
/// library's own message.
fn check(code: i32, what: &'static str) -> Result<(), StoreError> {
    if code == 0 {
        return Ok(());
    }
    // SAFETY: `db_strerror` returns a pointer to a NUL-terminated static
    // string for any input, including codes it does not recognise.
    let message = unsafe { CStr::from_ptr(sys::db_strerror(code)) }
        .to_string_lossy()
        .into_owned();
    // Berkeley DB reports out-of-space and other filesystem failures as
    // positive errno values; keep those on the I/O arm so callers can
    // branch on them the way they do for the other backends.
    if code > 0 {
        return Err(StoreError::Io(std::io::Error::from_raw_os_error(code)));
    }
    Err(StoreError::Backend(format!("{what}: {message}").into()))
}

/// Reads one member function pointer, refusing a null instead of calling
/// through it.
macro_rules! method {
    ($handle:expr, $name:ident, $what:literal) => {
        // SAFETY: `$handle` is a non-null pointer to a live `DB`/`DBC`
        // allocated by libdb, checked at construction and kept alive by
        // the owning wrapper for as long as this borrow lasts.
        match unsafe { (*$handle).$name } {
            Some(function) => Ok(function),
            None => Err(StoreError::Backend(
                concat!("libdb provides no ", $what, " entry point").into(),
            )),
        }
    };
}

/// A path as libdb wants it: a NUL-terminated byte string.
fn c_path(path: &Path) -> Result<CString, StoreError> {
    CString::new(path.as_os_str().as_bytes())
        .map_err(|_| StoreError::InvalidInput("store path contains NUL"))
}

/// An owned `DB` handle, closed on drop.
pub(crate) struct Db {
    handle: *mut sys::DB,
    read_only: bool,
}

impl Db {
    /// Opens `path` as `db_type`, exactly as libpinyin opens its own
    /// files: no environment, no transaction, no comparator, mode 0644.
    ///
    /// Passing no comparator is load-bearing for `DB_BTREE`: the default
    /// is a byte-wise `memcmp` with the shorter key first on a shared
    /// prefix, which is the store's key-ordering contract exactly.
    /// Setting one would silently reorder files libpinyin wrote.
    pub(crate) fn open(
        path: &Path,
        db_type: DBTYPE,
        read_only: bool,
        create: bool,
    ) -> Result<Self, StoreError> {
        check_runtime_version()?;
        let path = c_path(path)?;

        let mut handle: *mut sys::DB = ptr::null_mut();
        // SAFETY: `db_create` writes a fresh handle through the pointer
        // it is given. A null environment asks for a standalone database,
        // which is what libpinyin uses everywhere.
        let code = unsafe { sys::db_create(&raw mut handle, ptr::null_mut(), 0) };
        check(code, "db_create")?;
        // Hazard (d): libdb reports an allocation failure here by leaving
        // the out-parameter null. Calling a member on that is a null
        // dereference, so it is refused before the handle is wrapped.
        if handle.is_null() {
            return Err(StoreError::Backend(
                "db_create returned success with a null handle (allocation failure)".into(),
            ));
        }
        // From here on the handle is owned: every early return must close
        // it, which `this` does by construction.
        let this = Self { handle, read_only };

        let mut flags = if read_only {
            sys::DB_RDONLY
        } else if create {
            sys::DB_CREATE
        } else {
            0
        };
        // An existing file must not be silently re-created as a different
        // type; libdb checks that itself when DB_CREATE is absent.
        if read_only {
            flags |= sys::DB_RDONLY;
        }

        let open = method!(this.handle, open, "DB->open")?;
        // SAFETY: `this.handle` is live; `path` outlives the call; both
        // the environment (already bound at create) and the transaction
        // are null, and the sub-database name is null, which is the
        // whole-file form libpinyin uses.
        let code = unsafe {
            open(
                this.handle,
                ptr::null_mut(),
                path.as_ptr(),
                ptr::null(),
                db_type,
                flags,
                0o644,
            )
        };
        check(code, "DB->open")?;
        Ok(this)
    }

    /// Whether this handle was opened read-only.
    pub(crate) const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Point read. `None` when the key is absent.
    ///
    /// Copies the value out: a `get` result outlives the call, so it
    /// cannot borrow libdb's record memory the way [`Cursor::get`] does.
    pub(crate) fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let get = method!(self.handle, get, "DB->get")?;
        let mut key_dbt = dbt_from(key);
        let mut value_dbt = empty_dbt();
        // SAFETY: both `DBT`s are live locals; `key` outlives the call.
        // The value `DBT` carries no flags, so libdb fills it with a
        // pointer into its own memory, valid until the next operation on
        // this handle — the copy below happens before any such call.
        let code = unsafe {
            get(
                self.handle,
                ptr::null_mut(),
                &raw mut key_dbt,
                &raw mut value_dbt,
                0,
            )
        };
        if code == sys::DB_NOTFOUND {
            return Ok(None);
        }
        check(code, "DB->get")?;
        // SAFETY: the read succeeded, and the borrow ends inside this
        // expression — the copy happens before any further call on the
        // handle, which is the window libdb guarantees.
        Ok(Some(unsafe { dbt_slice(&value_dbt) }.to_vec()))
    }

    /// Insert or overwrite.
    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let put = method!(self.handle, put, "DB->put")?;
        let mut key_dbt = dbt_from(key);
        let mut value_dbt = dbt_from(value);
        // SAFETY: both slices outlive the call; libdb copies out of them
        // before returning.
        let code = unsafe {
            put(
                self.handle,
                ptr::null_mut(),
                &raw mut key_dbt,
                &raw mut value_dbt,
                0,
            )
        };
        check(code, "DB->put")
    }

    /// Remove `key`; absent is not an error.
    pub(crate) fn del(&self, key: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let del = method!(self.handle, del, "DB->del")?;
        let mut key_dbt = dbt_from(key);
        // SAFETY: `key` outlives the call; the transaction is null.
        let code = unsafe { del(self.handle, ptr::null_mut(), &raw mut key_dbt, 0) };
        if code == sys::DB_NOTFOUND {
            return Ok(());
        }
        check(code, "DB->del")
    }

    /// Flush to the operating system (`hard` also asks for the disk).
    pub(crate) fn sync(&self) -> Result<(), StoreError> {
        if self.read_only {
            return Ok(());
        }
        let sync = method!(self.handle, sync, "DB->sync")?;
        // SAFETY: the handle is live and the flags argument is the
        // documented zero.
        check(unsafe { sync(self.handle, 0) }, "DB->sync")
    }

    /// Opens a cursor over this database.
    ///
    /// The returned cursor borrows `self`, so the database cannot be
    /// dropped while a cursor is open on it.
    pub(crate) fn cursor(&self) -> Result<Cursor<'_>, StoreError> {
        let cursor = method!(self.handle, cursor, "DB->cursor")?;
        let mut handle: *mut sys::DBC = ptr::null_mut();
        // SAFETY: the database handle is live; the transaction is null;
        // the out-parameter is a live local.
        let code = unsafe { cursor(self.handle, ptr::null_mut(), &raw mut handle, 0) };
        check(code, "DB->cursor")?;
        // Hazard (d) again: a null out-parameter on success.
        if handle.is_null() {
            return Err(StoreError::Backend(
                "DB->cursor returned success with a null cursor (allocation failure)".into(),
            ));
        }
        Ok(Cursor {
            handle,
            _db: PhantomData,
        })
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        // A close failure has nowhere to go in `Drop`, and libdb frees the
        // handle regardless of what it returns, so the code is discarded.
        // Callers who need to know that a flush succeeded call `sync`
        // first, which does report.
        if let Ok(close) = method!(self.handle, close, "DB->close") {
            // SAFETY: the handle is live and owned; this is the last use
            // of it, and libdb frees it whatever the return code.
            unsafe {
                close(self.handle, 0);
            }
        }
    }
}

/// A cursor over one database, closed on drop.
///
/// The `'db` lifetime ties the cursor to its database; `Record`'s
/// lifetime ties a read row to the cursor (see [`Cursor::get`]).
pub(crate) struct Cursor<'db> {
    handle: *mut sys::DBC,
    _db: PhantomData<&'db Db>,
}

/// Where to move the cursor before reading.
#[derive(Clone, Copy)]
pub(crate) enum Seek<'a> {
    /// `DB_FIRST` — the first record in key order.
    First,
    /// `DB_NEXT` — the record after the current position.
    Next,
    /// `DB_SET_RANGE` — the smallest key at or after this one.
    AtOrAfter(&'a [u8]),
}

/// One record borrowed from a positioned cursor.
///
/// Hazard (c): the bytes live in libdb's cursor memory and are valid only
/// while the cursor stays where it is. The `'c` lifetime is the borrow of
/// the cursor itself, so a second [`Cursor::get`] — which needs `&mut` —
/// cannot compile while a `Record` is alive. A caller that wants to keep
/// a row copies exactly what it retains.
pub(crate) struct Record<'c> {
    /// The record's key bytes.
    pub(crate) key: &'c [u8],
    /// The record's value bytes.
    pub(crate) value: &'c [u8],
}

impl Cursor<'_> {
    /// Moves as `seek` says and reads the record there; `None` at the end
    /// of the database.
    pub(crate) fn get(&mut self, seek: Seek<'_>) -> Result<Option<Record<'_>>, StoreError> {
        let get = method!(self.handle, get, "DBC->get")?;
        let (mut key_dbt, flags) = match seek {
            Seek::First => (empty_dbt(), sys::DB_FIRST),
            Seek::Next => (empty_dbt(), sys::DB_NEXT),
            Seek::AtOrAfter(key) => (dbt_from(key), sys::DB_SET_RANGE),
        };
        let mut value_dbt = empty_dbt();
        // SAFETY: the cursor handle is live; both `DBT`s are live locals,
        // and any key slice `seek` carries outlives the call. Neither
        // `DBT` sets a memory flag, so libdb fills them with pointers into
        // its own cursor memory — which is exactly what `Record`'s
        // lifetime below is bounding.
        let code = unsafe { get(self.handle, &raw mut key_dbt, &raw mut value_dbt, flags) };
        if code == sys::DB_NOTFOUND {
            return Ok(None);
        }
        check(code, "DBC->get")?;
        // SAFETY: the two `DBT`s were filled by a successful `DBC->get`,
        // so each points at `size` initialised bytes owned by libdb's
        // cursor memory. That memory stays valid until the next operation
        // on this cursor, and `Record<'_>` borrows `*self` for exactly
        // that window: another `get` needs `&mut self`, and `close` needs
        // ownership, so neither can happen while the record lives. The
        // lifetime is therefore bounded by the caller's borrow, not by
        // the `DBT` locals, which is why the slices cannot be built from
        // references to those locals.
        let (key, value) = unsafe { (dbt_slice(&key_dbt), dbt_slice(&value_dbt)) };
        Ok(Some(Record { key, value }))
    }
}

impl Drop for Cursor<'_> {
    fn drop(&mut self) {
        if let Ok(close) = method!(self.handle, close, "DBC->close") {
            // SAFETY: the cursor handle is live and owned, its database
            // outlives it by the `'db` bound, and this is its last use.
            unsafe {
                close(self.handle);
            }
        }
    }
}

/// A `DBT` describing `bytes` — libdb reads through it and copies before
/// returning, so the slice only has to outlive the call.
fn dbt_from(bytes: &[u8]) -> sys::DBT {
    let mut dbt = empty_dbt();
    dbt.data = bytes.as_ptr().cast::<std::ffi::c_void>().cast_mut();
    dbt.size = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
    dbt
}

/// A zeroed `DBT`, which is how libdb wants an output parameter.
fn empty_dbt() -> sys::DBT {
    // SAFETY: `DBT` is a plain C struct of pointers and integers with no
    // niche and no invalid bit pattern; all-zero is the initialisation
    // libdb's own documentation prescribes (`memset(&dbt, 0, sizeof dbt)`).
    unsafe { std::mem::zeroed() }
}

/// The bytes a filled-in `DBT` points at, at a caller-chosen lifetime.
///
/// The pointer inside a read `DBT` is libdb's memory, not the `DBT`'s, so
/// the slice's lifetime is *not* the borrow of the struct — tying it there
/// would be both wrong and unusable. It is the caller's job to pick a
/// lifetime no longer than the window libdb guarantees.
///
/// # Safety
///
/// `dbt` must have been filled by a successful libdb read, and `'a` must
/// not outlive the memory libdb documents for it: until the next
/// operation on the handle for `DB->get`, until the next operation on the
/// cursor for `DBC->get`. The two callers below satisfy this by copying
/// immediately (`Db::get`) or by bounding `'a` to a `&mut` borrow of the
/// cursor (`Cursor::get`).
unsafe fn dbt_slice<'a>(dbt: &sys::DBT) -> &'a [u8] {
    if dbt.data.is_null() || dbt.size == 0 {
        return &[];
    }
    // SAFETY: the caller guarantees `dbt` was filled by a successful read
    // and that `'a` is within the window libdb keeps that memory valid.
    unsafe { std::slice::from_raw_parts(dbt.data.cast::<u8>(), dbt.size as usize) }
}
