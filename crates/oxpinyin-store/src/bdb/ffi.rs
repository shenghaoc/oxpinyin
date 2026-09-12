//! Safe RAII wrappers over the generated Berkeley DB declarations.
//!
//! This is the crate's only Berkeley DB FFI surface. Everything above it
//! — [`super::BdbStore`] — is safe Rust.
//!
//! The generated declarations are `unsafe extern "C"`, which the
//! workspace's `unsafe_code = "deny"` would otherwise reject; the allow
//! is scoped to this module, under the same backend waiver the tkrzw
//! and Kyoto Cabinet shims carry. Waived safety is not waived
//! correctness: every block below states its invariant, and the shared
//! read and write suites gate this backend like any other.
//!
//! # The four hazards, and where each is answered
//!
//! **Unwinding.** libdb is C, and Rust calls into it rather than the
//! reverse, so no Rust panic ever unwinds through a C frame here. The
//! [`Db`] and [`Cursor`] `Drop` impls close their handles on an unwind
//! just as on a normal return.
//!
//! **`Send`/`Sync`.** [`Db`] holds a raw pointer, so the compiler
//! derives neither; both are implemented, because the store's consumers
//! need them — `oxpinyin-user`'s registry keeps `DefaultStore` behind a
//! `Mutex` in a `static` (`Send`), and `oxpinyin-data`'s readers share
//! one store across threads (`Sync`). Both are sound under the
//! configuration this module enforces: every handle is opened with
//! `DB_THREAD` and every `DBT` libdb writes into carries
//! `DB_DBT_USERMEM` over caller-owned memory. That pair is libdb's own
//! documented contract for a handle used from multiple threads — with
//! it, no operation returns memory owned by the library, and
//! concurrent calls are serialized by the handle's own locking.
//! Cursors never escape a single call on `&self`, so the "one cursor,
//! one thread at a time" rule holds by construction. Opening without
//! `DB_THREAD`, as libpinyin does, would permit zero-copy borrowed
//! reads — and cost a copy of every record, which the `ReadStore`
//! trait (`Vec<u8>` returns) charges anyway.
//!
//! **Cursor lifetimes.** [`Cursor::get`] takes `&mut self` and returns
//! a [`Row`] borrowing `&self` (the cursor's own buffers), so a second
//! `get` while a row is held is a **compile error**. libdb's prose rule
//! — cursor memory is valid until the next operation on that cursor —
//! is enforced by the borrow checker, not by a comment.
//!
//! **Null returns.** `db_create` reports allocation failure by leaving
//! its out-parameter null with a zero return in some builds, and every
//! method on a `DB`/`DBC` is a struct member function pointer that
//! bindgen types as `Option<unsafe extern "C" fn ...>`. Both are
//! checked: the pointer with an explicit test, the members through
//! [`method`], which turns a null member into an error rather than a
//! call through null.
#![allow(unsafe_code)]

use std::ffi::{CStr, CString};
use std::marker::PhantomData;
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
    // db.h's DBT is a bitfield struct, and bindgen's bitfield accessors
    // transmute integer types rustc 1.97's `unnecessary_transmutes`
    // (warn-by-default) flags inside generated code we cannot edit.
    unnecessary_transmutes,
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
/// 5.3 is what every target distro pins (5.3.28 is the last
/// Sleepycat-licensed release, which is why both Debian and Fedora
/// freeze there) and what libpinyin's `--with-dbm=BerkeleyDB` builds
/// against, so it is what wrote the files this backend reads. The check
/// is at run time rather than compile time because `db.h` and the
/// shared library can disagree — a header upgrade without a matching
/// runtime, or the reverse — and it is the runtime that owns the
/// on-disk format. The Homebrew formula's 18.1 is refused here.
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
/// user profiles that the user's own libpinyin must read back, and the
/// generated `DB`/`DBT` layouts come from the build machine's header
/// while the linked library is the run machine's. Guessing at either is
/// how a profile gets corrupted silently, so an unsurveyed version is
/// an error at open rather than a risk taken at write.
pub(crate) fn check_runtime_version() -> Result<(), StoreError> {
    let (major, minor, _) = runtime_version();
    if (major, minor) == (SUPPORTED_MAJOR, SUPPORTED_MINOR) {
        return Ok(());
    }
    Err(StoreError::Backend(
        format!(
            "unsupported Berkeley DB {major}.{minor}: this backend is surveyed against \
             {SUPPORTED_MAJOR}.{SUPPORTED_MINOR} (the release every target distro pins and \
             libpinyin's --with-dbm=BerkeleyDB builds against), and it writes user profiles \
             that libpinyin itself has to read back"
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

/// A path as libdb wants it: a NUL-terminated byte string, taken from
/// the platform encoding the same way the LMDB backend takes it.
fn c_path(path: &Path) -> Result<CString, StoreError> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| StoreError::InvalidInput("path contains NUL"))
}

/// An owned `DB` handle, closed on drop.
///
/// Opened with `DB_THREAD`, every output `DBT` under `DB_DBT_USERMEM`:
/// the documented configuration for a handle used from more than one
/// thread, and the reason both `unsafe impl Send` and `unsafe impl
/// Sync` below are sound — see the module notes.
pub(crate) struct Db {
    handle: *mut sys::DB,
    read_only: bool,
    /// Whether this handle is a `DB_HASH` (unordered) rather than a
    /// `DB_BTREE` — the store's raw-walk strategy branches on it.
    hash: bool,
}

impl Db {
    /// Opens `path` as `db_type`, exactly as libpinyin opens its own
    /// files — no environment, no transaction, no comparator, mode
    /// 0644 — plus the `DB_THREAD` flag this module's threading
    /// contract needs.
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
        let this = Self {
            handle,
            read_only,
            hash: db_type == DB_HASH,
        };

        let mut flags = sys::DB_THREAD;
        if read_only {
            flags |= sys::DB_RDONLY;
        } else if create {
            flags |= sys::DB_CREATE;
        }
        // An existing file must not be silently re-created as a different
        // type; libdb checks that itself when DB_CREATE is absent.

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

    /// Whether this handle is a `DB_HASH` database (no key order), as
    /// opposed to a `DB_BTREE` (the store's byte order).
    pub(crate) const fn is_hash(&self) -> bool {
        self.hash
    }

    /// Point read. `None` when the key is absent.
    ///
    /// The value lands in a caller-owned buffer (`DB_DBT_USERMEM`),
    /// grown once on `DB_BUFFER_SMALL` and returned as an owned `Vec` —
    /// the `DB_THREAD` contract, and the trait's copy-owning shape,
    /// both satisfied by one allocation.
    pub(crate) fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let get = method!(self.handle, get, "DB->get")?;
        let mut key_dbt = dbt_from(key)?;
        let mut buf = vec![0_u8; INITIAL_BUFFER];
        loop {
            let mut value_dbt = usermem_dbt(&mut buf);
            // SAFETY: both `DBT`s are live locals; `key` and `buf`
            // outlive the call; the transaction is null, as in every
            // libpinyin call.
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
            if code == sys::DB_BUFFER_SMALL {
                grow(&mut buf, value_dbt.size);
                continue;
            }
            check(code, "DB->get")?;
            let size = value_dbt.size as usize;
            if size > buf.len() {
                return Err(StoreError::Backend(
                    "DB->get reported more bytes than the buffer it filled".into(),
                ));
            }
            buf.truncate(size);
            return Ok(Some(buf));
        }
    }

    /// Insert or overwrite.
    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let put = method!(self.handle, put, "DB->put")?;
        let mut key_dbt = dbt_from(key)?;
        let mut value_dbt = dbt_from(value)?;
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
        let mut key_dbt = dbt_from(key)?;
        // SAFETY: `key` outlives the call; the transaction is null.
        let code = unsafe { del(self.handle, ptr::null_mut(), &raw mut key_dbt, 0) };
        if code == sys::DB_NOTFOUND {
            return Ok(());
        }
        check(code, "DB->del")
    }

    /// Flush to the operating system. This is the commit-visible sync
    /// `write` ends with and the device-visible sync `compact` calls.
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
        // the out-parameter is a live local. `DB_THREAD` on the database
        // makes a cursor from one thread usable in these single-threaded
        // patterns; the cursor itself never crosses threads (it is not
        // `Send`, and only lives inside one call on this `&self`).
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
            key_buf: vec![0_u8; INITIAL_BUFFER],
            value_buf: vec![0_u8; INITIAL_BUFFER],
            _db: PhantomData,
        })
    }
}

// SAFETY: `Db` moves between threads (`oxpinyin-user`'s registry holds a
// `DefaultStore` behind a `Mutex` in a `static`, which requires `Send`)
// and is shared across them (`oxpinyin-data`'s chewing reader holds one
// behind `Box<dyn ChewingDbm + Send + Sync>`, which requires `Sync`).
// Both claims are sound under exactly the configuration `Db::open`
// builds, and that configuration is libdb's own documented contract for
// multi-threaded handles: the handle carries `DB_THREAD` — the flag
// whose entire meaning is "this handle may be used by multiple threads
// simultaneously" — and every `DBT` libdb writes into (the value of a
// `get`, the key and value of a cursor read) is `DB_DBT_USERMEM` over
// memory this side owns, so no operation hands back library-owned
// memory tied to the calling thread. Cursors are created, walked and
// closed inside a single call on `&self`, so the "one cursor, one
// thread at a time" rule holds by construction even when two threads
// run concurrent reads. libpinyin opens the same files without
// `DB_THREAD` because it is single-threaded per handle; the cost of the
// flag here is one caller-buffer copy per record read, which the
// `ReadStore` trait (`Vec<u8>` returns) charges anyway.
unsafe impl Send for Db {}
unsafe impl Sync for Db {}

impl Drop for Db {
    fn drop(&mut self) {
        // libpinyin's `reset()` is `sync` then `close`, and a `close`
        // failure has nowhere to go in `Drop` — libdb frees the handle
        // regardless of what `close` returns, so the codes are
        // discarded. Callers who need to know that a flush succeeded
        // call `sync` first, which does report.
        if !self.read_only
            && let Ok(sync) = method!(self.handle, sync, "DB->sync")
        {
            // SAFETY: the handle is live and owned.
            unsafe {
                sync(self.handle, 0);
            }
        }
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
/// The `'db` lifetime ties the cursor to its database. Key and value
/// land in buffers the cursor owns (`DB_DBT_USERMEM`, the `DB_THREAD`
/// contract), and [`Cursor::get`] hands them out as a [`Row`] borrowing
/// the cursor — hazard (c) below.
pub(crate) struct Cursor<'db> {
    handle: *mut sys::DBC,
    /// Reusable key buffer: grows to the largest key seen, never shrinks.
    key_buf: Vec<u8>,
    /// Reusable value buffer: same policy.
    value_buf: Vec<u8>,
    _db: PhantomData<&'db Db>,
}

/// Where to move the cursor before reading.
#[derive(Clone, Copy)]
pub(crate) enum Seek<'a> {
    /// `DB_FIRST` — the first record in key order (a `DB_BTREE` walks in
    /// key order; a `DB_HASH` walks in bucket order, which callers that
    /// need an order must not rely on).
    First,
    /// `DB_NEXT` — the record after the current position.
    Next,
    /// `DB_SET_RANGE` — the smallest key at or after this one.
    AtOrAfter(&'a [u8]),
}

/// One row borrowed from a positioned cursor.
///
/// Hazard (c): the bytes live in the cursor's own buffers and stay valid
/// only until the cursor moves. The `'c` lifetime is the borrow of the
/// cursor itself, so a second [`Cursor::get`] — which needs `&mut` —
/// cannot compile while a `Row` is alive. A caller that wants to keep a
/// row copies exactly what it retains.
pub(crate) struct Row<'c> {
    /// The row's key bytes.
    pub(crate) key: &'c [u8],
    /// The row's value bytes.
    pub(crate) value: &'c [u8],
}

impl Cursor<'_> {
    /// Moves as `seek` says and reads the row there; `None` at the end
    /// of the database.
    pub(crate) fn get(&mut self, seek: Seek<'_>) -> Result<Option<Row<'_>>, StoreError> {
        let get = method!(self.handle, get, "DBC->get")?;
        let flags = match seek {
            Seek::First => sys::DB_FIRST,
            Seek::Next => sys::DB_NEXT,
            Seek::AtOrAfter(_) => sys::DB_SET_RANGE,
        };
        // For `DB_SET_RANGE` the key `DBT` is both input (the seek
        // target) and output (the found key); every other mode reads it
        // as pure output. Under `DB_DBT_USERMEM` both directions run
        // through the same buffer — which is why the seek target is
        // re-copied into that buffer at the top of EVERY iteration: a
        // `DB_BUFFER_SMALL` attempt may have overwritten it with the
        // found key, and the retry must re-seek from the caller's bytes.
        loop {
            let input_key_len = match seek {
                Seek::AtOrAfter(key) => {
                    let len = key.len();
                    if len > self.key_buf.len() {
                        self.key_buf.resize(len, 0);
                    }
                    self.key_buf[..len].copy_from_slice(key);
                    len
                }
                _ => 0,
            };
            let mut key_dbt = usermem_dbt_with_size(&mut self.key_buf, input_key_len);
            let mut value_dbt = usermem_dbt(&mut self.value_buf);
            // SAFETY: the cursor handle is live; both `DBT`s point at
            // this cursor's own buffers, which outlive the call, and any
            // seek slice `AtOrAfter` carries was re-copied into
            // `key_buf` at the top of this iteration.
            let code = unsafe { get(self.handle, &raw mut key_dbt, &raw mut value_dbt, flags) };
            if code == sys::DB_NOTFOUND {
                return Ok(None);
            }
            if code == sys::DB_BUFFER_SMALL {
                // libdb reports, per `DBT`, the size it needed; grow the
                // buffers to those sizes and retry the same operation.
                // The position does not advance on `DB_BUFFER_SMALL`, so
                // the retry reads the same row. If neither buffer grew,
                // the library is contradicting itself and that must not
                // loop forever.
                let key_was = self.key_buf.len();
                let value_was = self.value_buf.len();
                grow(&mut self.key_buf, key_dbt.size);
                grow(&mut self.value_buf, value_dbt.size);
                if self.key_buf.len() == key_was && self.value_buf.len() == value_was {
                    return Err(StoreError::Backend(
                        "DBC->get keeps returning DB_BUFFER_SMALL with buffers it says fit".into(),
                    ));
                }
                continue;
            }
            check(code, "DBC->get")?;
            let key_len = key_dbt.size as usize;
            let value_len = value_dbt.size as usize;
            if key_len > self.key_buf.len() || value_len > self.value_buf.len() {
                return Err(StoreError::Backend(
                    "DBC->get reported more bytes than the buffers it filled".into(),
                ));
            }
            let row = Row {
                key: &self.key_buf[..key_len],
                value: &self.value_buf[..value_len],
            };
            return Ok(Some(row));
        }
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

/// The starting size of every `DB_DBT_USERMEM` buffer: one BDB page
/// covers almost every record this store reads, so growth is the
/// exception rather than the rule.
const INITIAL_BUFFER: usize = 4096;

/// A `DBT` describing `bytes` — libdb reads through it and copies before
/// returning, so the slice only has to outlive the call.
fn dbt_from(bytes: &[u8]) -> Result<sys::DBT, StoreError> {
    let size = u32::try_from(bytes.len()).map_err(|_| {
        StoreError::InvalidInput("key or value longer than Berkeley DB's 4 GiB DBT size")
    })?;
    let mut dbt = empty_dbt();
    dbt.data = bytes.as_ptr().cast::<std::ffi::c_void>().cast_mut();
    dbt.size = size;
    Ok(dbt)
}

/// A `DBT` over `buf` as a `DB_DBT_USERMEM` output: libdb may write at
/// most `buf.len()` bytes into it and reports what it wrote in `size`.
fn usermem_dbt(buf: &mut [u8]) -> sys::DBT {
    usermem_dbt_with_size(buf, 0)
}

/// [`usermem_dbt`] with an input `size` — the `DB_SET_RANGE` key form,
/// where the same `DBT` names the seek target (`size`) and the
/// copy-back buffer (`ulen`).
fn usermem_dbt_with_size(buf: &mut [u8], size: usize) -> sys::DBT {
    let mut dbt = empty_dbt();
    dbt.flags = sys::DB_DBT_USERMEM;
    dbt.data = buf.as_mut_ptr().cast::<std::ffi::c_void>();
    dbt.ulen = u32::try_from(buf.len()).unwrap_or(u32::MAX);
    dbt.size = u32::try_from(size).unwrap_or(u32::MAX);
    dbt
}

/// Grows a `DB_DBT_USERMEM` buffer to the size a `DB_BUFFER_SMALL`
/// return reported it needed. Only ever grows: the reported size is the
/// needed length, so a buffer already that large was not the small one.
fn grow(buf: &mut Vec<u8>, reported: u32) {
    let needed = reported as usize;
    if needed > buf.len() {
        buf.resize(needed, 0);
    }
}

/// A zeroed `DBT`, which is how libdb wants an output parameter.
fn empty_dbt() -> sys::DBT {
    // SAFETY: `DBT` is a plain C struct of pointers and integers with no
    // niche and no invalid bit pattern; all-zero is the initialisation
    // libdb's own documentation prescribes (`memset(&dbt, 0, sizeof dbt)`).
    unsafe { std::mem::zeroed() }
}
