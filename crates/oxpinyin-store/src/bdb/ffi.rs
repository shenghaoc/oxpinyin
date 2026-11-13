//! Safe RAII wrappers over the generated Berkeley DB declarations.
//!
//! This is the crate's only Berkeley DB FFI surface. Everything above it
//! — [`super::BdbStore`] — is safe Rust.
//!
//! The generated declarations are `unsafe extern "C"`, which the
//! workspace's `unsafe_code = "deny"` would otherwise reject; the
//! `expect` is scoped to this module alone. Every `unsafe` in the
//! backend lives below, so `super` stays under the workspace `deny` —
//! the shape the Kyoto Cabinet shim shares, and one the tkrzw backend
//! cannot have. Because the waiver is an `expect` rather than
//! an `allow`, an unfulfilled one draws
//! `unfulfilled_lint_expectations` — warn-by-default, an error only
//! where warnings are denied, and every supported CI check denies them
//! (`RUSTFLAGS: -D warnings`), so a stale waiver cannot land unnoticed.
//! Waived safety is not waived correctness: every block below states
//! its invariant, and the shared read and write suites gate this
//! backend like any other.
//!
//! # The four hazards, and where each is answered
//!
//! **Unwinding.** libdb is C, and Rust calls into it rather than the
//! reverse, so no Rust panic ever unwinds through a C frame here. The
//! [`Db`] and [`Cursor`] `Drop` impls close their handles on an unwind
//! just as on a normal return.
//!
//! **One caller at a time.** [`Db`] is neither `Send` nor `Sync`: its raw
//! handle keeps it on one thread, and the C contract serializes the calls
//! that reach it. Cursors borrow the database. Nothing of the handle's is
//! borrowed while a visitor reads a row, so visitors can read the same
//! store. `DB_DBT_USERMEM` keeps cursor output in caller-owned buffers that
//! remain valid across those intervening reads. A point read uses the
//! handle's own return buffer and copies the value out before it returns.
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
//! `method!`, which turns a null member into an error rather than a
//! call through null.
#![expect(
    unsafe_code,
    reason = "FFI over the system libdb; every block carries a SAFETY comment"
)]

use std::ffi::{CStr, CString, c_int};
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
    // `clippy::all` does not reach the restriction group, and bindgen's
    // generated bitfield accessors carry bare `unsafe` blocks with no
    // safety comments — a lint this crate cannot satisfy in a file it
    // does not write. Scoped to the generated module only.
    clippy::undocumented_unsafe_blocks,
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
/// the platform encoding.
fn c_path(path: &Path) -> Result<CString, StoreError> {
    CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| StoreError::InvalidInput("path contains NUL"))
}

/// An owned, exclusively used `DB` handle, closed on drop.
pub(crate) struct Db {
    handle: *mut sys::DB,
    read_only: bool,
    /// Whether this handle is a `DB_HASH` (unordered) rather than a
    /// `DB_BTREE` — the store's raw-walk strategy branches on it.
    hash: bool,
    /// Whether `DB->open` succeeded. libdb refuses `DB->sync` on a handle
    /// that was never opened and says so on stderr (`BDB1565`), so a handle
    /// whose open failed is closed without the flush.
    opened: bool,
    /// Whether dropping a writable handle syncs it before `DB->close`.
    /// [`Db::close_unsynced`] clears it for a `save_db` copy, which the pin
    /// closes with no sync of its own handle.
    sync_on_drop: bool,
}

/// The mode libpinyin's `attach` passes to `DB->open` — the system
/// tables (`chewing_large_table2_bdb.cpp:90-91`,
/// `phrase_large_table3_bdb.cpp:105-106`, `ngram_bdb.cpp:119-120`).
pub(crate) const SYSTEM_FILE_MODE: c_int = 0o644;

/// The mode libpinyin's `save_db` passes to `DB->open` when it creates a
/// user table — `user_pinyin_index.bin`, `user_phrase_index.bin`,
/// `user_bigram.db` (`chewing_large_table2_bdb.cpp:148-149`,
/// `phrase_large_table3_bdb.cpp:163-164`, `ngram_bdb.cpp:94-95`).
pub(crate) const USER_FILE_MODE: c_int = 0o600;

impl Db {
    /// Opens `path` as `db_type`, as libpinyin opens its own files — no
    /// explicit environment, no transaction, no comparator, and only
    /// `DB_RDONLY` or `DB_CREATE` as applicable.
    ///
    /// `mode` is what `DB->open` creates a missing file with, before the
    /// process umask applies: [`SYSTEM_FILE_MODE`] where libpinyin
    /// attaches, [`USER_FILE_MODE`] where it saves a user table. libdb
    /// ignores it for a file that already exists.
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
        mode: c_int,
    ) -> Result<Self, StoreError> {
        let path = c_path(path)?;
        Self::open_at(Some(&path), db_type, read_only, create, mode)
    }

    /// An in-memory `DB_HASH` — `DB->open` with a null file name, as
    /// libpinyin's `Bigram::load_db` creates the user bigram it works on
    /// (`ngram_bdb.cpp:51-56`: `DB_HASH, DB_CREATE, 0600`), with the pin's
    /// answers (`:51-57`): `Ok(None)` when the native `DB->open`
    /// failed, after its handle was closed (Berkeley DB requires
    /// `DB->close` then, `dbopen.html`, 5.3.28; compatibility register row
    /// 113).
    ///
    /// # Errors
    ///
    /// A linked libdb this backend does not support.
    ///
    /// A failed `db_create` aborts the process, where the pin asserts
    /// (`ngram_bdb.cpp:51-52` at 074a2219; [`attach_outcome`]).
    pub(crate) fn open_in_memory_hash_attach() -> Result<Option<Self>, StoreError> {
        attach_outcome(Self::open_steps(None, DB_HASH, false, true, USER_FILE_MODE))
    }

    /// An in-memory `DB_BTREE` — the same null-file-name form as
    /// [`Self::open_in_memory_hash_attach`], ordered: the session store a
    /// libpinyin profile is loaded into.
    pub(crate) fn open_in_memory_tree() -> Result<Self, StoreError> {
        Self::open_at(None, DB_BTREE, false, true, USER_FILE_MODE)
    }

    /// [`Self::open`]'s body over an optional file name (`None` is
    /// libdb's in-memory database).
    fn open_at(
        path: Option<&std::ffi::CStr>,
        db_type: DBTYPE,
        read_only: bool,
        create: bool,
        mode: c_int,
    ) -> Result<Self, StoreError> {
        Self::open_steps(path, db_type, read_only, create, mode).map_err(OpenFailure::into_error)
    }

    /// The pin's `attach` open: `Ok(None)` when the native `DB->open`
    /// failed, after its handle was closed (Berkeley DB requires
    /// `DB->close` then, `dbopen.html`, 5.3.28).
    ///
    /// # Errors
    ///
    /// A path that contains NUL, or a linked libdb this backend does not
    /// support: neither is a failed attach.
    ///
    /// A failed `db_create` aborts the process, where every pin `attach`
    /// asserts or aborts (`chewing_large_table2_bdb.cpp:87-88`,
    /// `phrase_large_table3_bdb.cpp:102-103`, `punct_table_bdb.cpp:64-65`,
    /// `ngram_bdb.cpp:116-117`, `flexible_ngram_bdb.h:106-108` at 074a2219;
    /// [`attach_outcome`]).
    pub(crate) fn open_attach(
        path: &Path,
        db_type: DBTYPE,
        read_only: bool,
        create: bool,
        mode: c_int,
    ) -> Result<Option<Self>, StoreError> {
        let path = c_path(path)?;
        attach_outcome(Self::open_steps(
            Some(&path),
            db_type,
            read_only,
            create,
            mode,
        ))
    }

    /// The open sequence, with the step that failed.
    fn open_steps(
        path: Option<&std::ffi::CStr>,
        db_type: DBTYPE,
        read_only: bool,
        create: bool,
        mode: c_int,
    ) -> Result<Self, OpenFailure> {
        check_runtime_version().map_err(OpenFailure::Before)?;

        let mut handle: *mut sys::DB = ptr::null_mut();
        // SAFETY: `db_create` writes a fresh handle through the pointer
        // it is given. A null environment asks for a standalone database,
        // which is what libpinyin uses everywhere.
        let code = unsafe { sys::db_create(&raw mut handle, ptr::null_mut(), 0) };
        check(code, "db_create").map_err(OpenFailure::Create)?;
        // Hazard (d): libdb reports an allocation failure here by leaving
        // the out-parameter null. Calling a member on that is a null
        // dereference, so it is refused before the handle is wrapped.
        if handle.is_null() {
            return Err(OpenFailure::Create(StoreError::Backend(
                "db_create returned success with a null handle (allocation failure)".into(),
            )));
        }
        // From here on the handle is owned: every early return must close
        // it, which `this` does by construction.
        let mut this = Self {
            handle,
            read_only,
            hash: db_type == DB_HASH,
            opened: false,
            sync_on_drop: true,
        };

        let mut flags = 0;
        if read_only {
            flags |= sys::DB_RDONLY;
        } else if create {
            flags |= sys::DB_CREATE;
        }
        // An existing file must not be silently re-created as a different
        // type; libdb checks that itself when DB_CREATE is absent.

        let open = method!(this.handle, open, "DB->open").map_err(OpenFailure::Before)?;
        // SAFETY: `this.handle` is live; `path`, when present, outlives
        // the call, and a null file name is libdb's documented in-memory
        // database; both the environment (already bound at create) and
        // the transaction are null, and the sub-database name is null,
        // which is the whole-file form libpinyin uses.
        let code = unsafe {
            open(
                this.handle,
                ptr::null_mut(),
                path.map_or(ptr::null(), std::ffi::CStr::as_ptr),
                ptr::null(),
                db_type,
                flags,
                mode,
            )
        };
        // On failure `this` drops here: `DB->close`, with no sync.
        check(code, "DB->open").map_err(OpenFailure::Open)?;
        this.opened = true;
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

    /// Point read into `buf`, which is cleared first. `false` when the
    /// key is absent.
    ///
    /// The value DBT is zeroed, as in every libpinyin point read
    /// (074a2219 `ngram_bdb.cpp:138-140`, `phrase_large_table3_bdb.cpp:194-196`,
    /// `chewing_large_table2_bdb.cpp:180-182`), so libdb copies the record
    /// into this handle's own return buffer, which it grows only when a
    /// record is larger than any read before (`src/db/db_ret.c:144-158`,
    /// owned by the handle through `SET_RET_MEM`, `src/db/db_iface.c:800`).
    /// That buffer is valid until the next call on the handle, so the value
    /// is copied into `buf` before this returns, growing `buf` only past its
    /// capacity: a fresh buffer gets the value's exact size, and a buffer
    /// passed back for the next read allocates nothing. One lookup serves
    /// every record size: there is no `DB_BUFFER_SMALL` retry.
    pub(crate) fn get(&self, key: &[u8], buf: &mut Vec<u8>) -> Result<bool, StoreError> {
        buf.clear();
        let get = method!(self.handle, get, "DB->get")?;
        let mut key_dbt = dbt_from(key)?;
        let mut value_dbt = empty_dbt();
        // SAFETY: both `DBT`s are live locals and `key` outlives the call;
        // the transaction is null, as in every libpinyin call. A zeroed
        // output DBT is refused only on a `DB_THREAD` handle
        // (`src/db/db_iface.c:2921-2923`), and this one is opened without it.
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
            return Ok(false);
        }
        check(code, "DB->get")?;
        let size = value_dbt.size as usize;
        // An empty record copies nothing, and libdb then leaves `data` at
        // the handle's buffer, which is null until a non-empty read.
        if size == 0 {
            return Ok(true);
        }
        if value_dbt.data.is_null() {
            return Err(StoreError::Backend(
                "DB->get reported bytes but returned no buffer".into(),
            ));
        }
        // SAFETY: a successful DB->get with a zeroed DBT points `data` at
        // `size` initialized bytes of the handle's return buffer. Nothing
        // else can use the handle before the copy: `Db` is not `Sync`, and
        // this call returns only after the copy.
        let value = unsafe { std::slice::from_raw_parts(value_dbt.data.cast::<u8>(), size) };
        buf.reserve_exact(size);
        buf.extend_from_slice(value);
        Ok(true)
    }

    /// Whether `key` exists, an empty value included, copying nothing:
    /// `DB->exists`, which is `DB->get` with a `DB_DBT_PARTIAL |
    /// DB_DBT_USERMEM` data `DBT` of length 0 ("returns no bytes so
    /// there's no copy of the data", `src/db/db_iface.c:578-607`, Berkeley
    /// DB 5.3.28).
    pub(crate) fn exists(&self, key: &[u8]) -> Result<bool, StoreError> {
        let exists = method!(self.handle, exists, "DB->exists")?;
        let mut key_dbt = dbt_from(key)?;
        // SAFETY: the `DBT` is a live local over `key`, which outlives the
        // call; libdb only reads it. The transaction is null, as in every
        // libpinyin call, and the flags are the documented zero.
        let code = unsafe { exists(self.handle, ptr::null_mut(), &raw mut key_dbt, 0) };
        if code == sys::DB_NOTFOUND {
            return Ok(false);
        }
        check(code, "DB->exists")?;
        Ok(true)
    }

    /// `DB->get` with `DB_DBT_PARTIAL`: copy at most `length` bytes at
    /// `offset` into `buf`, the pin's header read
    /// (`flexible_ngram_bdb.h:309-326`, `:379-389`).
    pub(crate) fn get_partial(
        &self,
        key: &[u8],
        offset: u32,
        length: u32,
        buf: &mut Vec<u8>,
    ) -> Result<bool, StoreError> {
        let get = method!(self.handle, get, "DB->get")?;
        let mut key_dbt = dbt_from(key)?;
        let mut value_dbt = empty_dbt();
        value_dbt.flags = sys::DB_DBT_PARTIAL | sys::DB_DBT_USERMEM;
        value_dbt.doff = offset;
        value_dbt.dlen = length;
        buf.clear();
        buf.resize(length as usize, 0);
        value_dbt.data = buf.as_mut_ptr().cast();
        value_dbt.ulen = length;
        // SAFETY: both `DBT`s are live locals, `key` and `buf` outlive the
        // call; libdb writes at most `length` bytes into `buf`.
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
            return Ok(false);
        }
        check(code, "DB->get")?;
        buf.truncate(value_dbt.size as usize);
        Ok(true)
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
    /// `DB->del`: `Ok(false)` for `DB_NOTFOUND`, the answer the pin's
    /// `remove` returns for a missing record (`ret == 0`,
    /// `flexible_ngram_bdb.h:233-234`).
    pub(crate) fn del(&self, key: &[u8]) -> Result<bool, StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let del = method!(self.handle, del, "DB->del")?;
        let mut key_dbt = dbt_from(key)?;
        // SAFETY: `key` outlives the call; the transaction is null.
        let code = unsafe { del(self.handle, ptr::null_mut(), &raw mut key_dbt, 0) };
        if code == sys::DB_NOTFOUND {
            return Ok(false);
        }
        check(code, "DB->del").map(|()| true)
    }

    /// Flush to stable storage. `DB->sync` is libdb's only flush for a
    /// handle opened with no environment — which syscall it issues is
    /// the linked libdb's build-time choice (`__os_fsync` picks one per
    /// its configure result), not this binding's to claim — so this one
    /// call serves both roles: the commit-visible sync `write` ends
    /// with and the device-visible sync `compact` owes. There is no
    /// softer tier to select.
    pub(crate) fn sync(&self) -> Result<(), StoreError> {
        if self.read_only && self.opened {
            return Ok(());
        }
        let sync = method!(self.handle, sync, "DB->sync")?;
        // SAFETY: the handle is live and the flags argument is the
        // documented zero.
        check(unsafe { sync(self.handle, 0) }, "DB->sync")
    }

    /// Closes the handle with `DB->close(db, 0)` alone, as the pin's
    /// `save_db` closes the file it copied into
    /// (`chewing_large_table2_bdb.cpp:135-162`,
    /// `phrase_large_table3_bdb.cpp:150-177`, `ngram_bdb.cpp:81-108` at
    /// 074a2219): its `tmp_db->sync(m_db, 0)` syncs the in-memory source
    /// handle, not the new file, so the file's only flush is the one
    /// `DB->close` makes. Close's return code is discarded, as the pin
    /// discards it.
    pub(crate) fn close_unsynced(mut self) {
        self.sync_on_drop = false;
    }

    /// [`Self::open_in_memory_hash_attach`]'s answer when the open fails at
    /// `step`, for the user bigram's injected-failure tests: the outcome
    /// goes through [`attach_outcome`], as a real failure does.
    #[cfg(test)]
    pub(crate) fn failed_in_memory_hash_attach(
        step: FailedStep,
    ) -> Result<Option<Self>, StoreError> {
        let error = StoreError::Backend("injected".into());
        attach_outcome::<Self>(Err(match step {
            FailedStep::Create => OpenFailure::Create(error),
            FailedStep::Open => OpenFailure::Open(error),
        }))
    }

    /// Opens a cursor that borrows this database. Each cursor owns its
    /// own buffer pair for its walk, as each pin walk owns its `DBT` pair
    /// (`ngram_bdb.cpp:194-205` at `074a2219`), so a visitor can open
    /// another cursor without aliasing.
    pub(crate) fn cursor(&self) -> Result<Cursor<'_>, StoreError> {
        let handle = self.cursor_handle()?;
        Ok(Cursor {
            handle,
            key_buf: vec![0_u8; INITIAL_BUFFER],
            value_buf: vec![0_u8; INITIAL_BUFFER],
            db: std::marker::PhantomData,
        })
    }

    /// Opens a native cursor.
    fn cursor_handle(&self) -> Result<*mut sys::DBC, StoreError> {
        let cursor = method!(self.handle, cursor, "DB->cursor")?;
        let mut handle: *mut sys::DBC = ptr::null_mut();
        // SAFETY: the database handle is live; the transaction is null;
        // the out-parameter is a live local, and `Db` is not `Sync`, so no
        // other call uses the database or its cursors during this one.
        let code = unsafe { cursor(self.handle, ptr::null_mut(), &raw mut handle, 0) };
        check(code, "DB->cursor")?;
        // Hazard (d) again: a null out-parameter on success.
        if handle.is_null() {
            return Err(StoreError::Backend(
                "DB->cursor returned success with a null cursor (allocation failure)".into(),
            ));
        }
        Ok(handle)
    }
}

/// The open step an injected failure stops at.
#[cfg(test)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum FailedStep {
    /// `db_create`.
    Create,
    /// The native `DB->open`.
    Open,
}

/// The step at which [`Db::open_steps`] stopped.
#[derive(Debug)]
enum OpenFailure {
    /// Before `db_create`: an unsupported libdb, or no `DB->open` method.
    Before(StoreError),
    /// `db_create` failed or left a null handle.
    Create(StoreError),
    /// The native `DB->open` failed; its handle is closed.
    Open(StoreError),
}

impl OpenFailure {
    fn into_error(self) -> StoreError {
        match self {
            Self::Before(error) | Self::Create(error) | Self::Open(error) => error,
        }
    }
}

/// [`Db::open_attach`]'s answer for one open: only a failed native
/// `DB->open` is a failed attach.
///
/// A failed `db_create` ends the process with `std::process::abort()`, the
/// register's abort rule. Every pin attach dies there: `assert(0 == ret)`
/// in the tables' and the bigram's `attach` and the user bigram's `load_db`
/// (`chewing_large_table2_bdb.cpp:87-88`, `phrase_large_table3_bdb.cpp:
/// 102-103`, `punct_table_bdb.cpp:64-65`, `ngram_bdb.cpp:51-52`, `:116-117`)
/// and `abort()` in `FlexibleBigram::attach` (`flexible_ngram_bdb.h:106-108`
/// at 074a2219). Nothing is written first: an `abort()` writes nothing, and
/// glibc's `Assertion … failed.` line is the register's policy-accepted
/// missing line.
fn attach_outcome<T>(opened: Result<T, OpenFailure>) -> Result<Option<T>, StoreError> {
    match opened {
        Ok(db) => Ok(Some(db)),
        Err(OpenFailure::Open(_)) => Ok(None),
        Err(OpenFailure::Before(error)) => Err(error),
        Err(OpenFailure::Create(_)) => std::process::abort(),
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        // libpinyin's `reset()` is `sync` then `close`, and a `close`
        // failure has nowhere to go in `Drop` — libdb frees the handle
        // regardless of what `close` returns, so the codes are
        // discarded. Callers who need to know that a flush succeeded
        // call `sync` first, which does report.
        if self.opened
            && !self.read_only
            && self.sync_on_drop
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
/// land in buffers the cursor owns (`DB_DBT_USERMEM`), and
/// [`Cursor::get`] hands them out as a [`Row`] borrowing
/// the cursor — hazard (c) below.
pub(crate) struct Cursor<'db> {
    handle: *mut sys::DBC,
    /// This walk's key buffer: grows to the largest key seen, never shrinks.
    key_buf: Vec<u8>,
    /// This walk's value buffer: same policy.
    value_buf: Vec<u8>,
    /// The cursor must not outlive its database.
    db: std::marker::PhantomData<&'db Db>,
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
        self.read(seek, true)
    }

    /// [`Self::get`] for the key alone. The value `DBT` asks for zero
    /// bytes at offset zero (`DB_DBT_PARTIAL`), so libdb copies none of it
    /// (`src/db/db_ret.c:106-114`) and reads no overflow page for it
    /// (`src/db/db_overflow.c:120-123`). The cursor moves exactly as
    /// [`Self::get`] would. The pin's key walk, `Bigram::get_all_items`
    /// (074a2219 `ngram_bdb.cpp:194-206`), copies every value it never
    /// reads.
    pub(crate) fn key(&mut self, seek: Seek<'_>) -> Result<Option<&[u8]>, StoreError> {
        Ok(self.read(seek, false)?.map(|row| row.key))
    }

    /// The body of [`Self::get`] and [`Self::key`]: `value` says whether
    /// the value is copied.
    fn read(&mut self, seek: Seek<'_>, value: bool) -> Result<Option<Row<'_>>, StoreError> {
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
            let input_key_len = if let Seek::AtOrAfter(key) = seek {
                let len = key.len();
                if len > self.key_buf.len() {
                    self.key_buf.resize(len, 0);
                }
                self.key_buf[..len].copy_from_slice(key);
                len
            } else {
                0
            };
            let mut key_dbt = usermem_dbt_with_size(&mut self.key_buf, input_key_len);
            let mut value_dbt = if value {
                usermem_dbt(&mut self.value_buf)
            } else {
                no_value_dbt()
            };
            // SAFETY: the cursor handle is live; both `DBT`s point at
            // this cursor's own buffers, which outlive the call, and any
            // seek slice `AtOrAfter` carries was re-copied into
            // `key_buf` at the top of this iteration. `Db` is not
            // `Sync`, so no other database or cursor call runs until return.
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

/// An output `DBT` that takes none of the record: a zero-length partial
/// read into an empty caller buffer, which libdb allows to be null when it
/// copies nothing (`src/db/db_ret.c:125-130`).
fn no_value_dbt() -> sys::DBT {
    let mut dbt = empty_dbt();
    dbt.flags = sys::DB_DBT_USERMEM | sys::DB_DBT_PARTIAL;
    dbt
}

/// A zeroed `DBT`, which is how libdb wants an output parameter.
fn empty_dbt() -> sys::DBT {
    // SAFETY: `DBT` is a plain C struct of pointers and integers with no
    // niche and no invalid bit pattern; all-zero is the initialisation
    // libdb's own documentation prescribes (`memset(&dbt, 0, sizeof dbt)`).
    unsafe { std::mem::zeroed() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_failed_native_open_is_a_failed_attach() {
        let backend = || StoreError::Backend("x".into());
        assert_eq!(attach_outcome(Ok(7)).unwrap(), Some(7));
        assert_eq!(
            attach_outcome::<u8>(Err(OpenFailure::Open(backend()))).unwrap(),
            None
        );
        // An unsupported libdb has no pin counterpart: an ordinary error.
        assert!(attach_outcome::<u8>(Err(OpenFailure::Before(backend()))).is_err());
    }

    /// Every pin attach dies where `db_create` fails, and so does this:
    /// SIGABRT, with no Rust panic text or other line before it.
    #[test]
    fn a_failed_db_create_aborts_where_the_pin_asserts() {
        let stderr = crate::death_test::assert_aborts(
            "bdb::ffi::tests::a_failed_db_create_aborts_where_the_pin_asserts",
            || {
                let _ =
                    attach_outcome::<u8>(Err(OpenFailure::Create(StoreError::Backend("x".into()))));
            },
        );
        assert!(stderr.is_empty(), "{}", String::from_utf8_lossy(&stderr));
    }

    #[test]
    fn point_reads_own_exact_values_across_buffer_growth_and_misses() {
        struct TempPath(std::path::PathBuf);
        impl Drop for TempPath {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        // Keep large records file-backed: an unnamed BDB's tiny cache can
        // spill into its compiled-in /var/tmp instead of the test's temp dir.
        let path = TempPath(
            std::env::temp_dir().join(format!("oxpinyin-point-read-{}.db", std::process::id())),
        );
        let db = Db::open(&path.0, DB_BTREE, false, true, USER_FILE_MODE).unwrap();
        // One buffer for every read, as a decode loop passes it.
        let mut value = Vec::new();
        for len in [0, 1, INITIAL_BUFFER, INITIAL_BUFFER + 1, 397_899] {
            let expected: Vec<u8> = (0..len).map(|index| (index % 251) as u8).collect();
            db.put(b"key", &expected).unwrap();
            let mut retained = Vec::new();
            assert!(db.get(b"key", &mut retained).unwrap());
            assert_eq!(retained, expected);
            // Exact size: a fresh buffer holds no spare capacity.
            assert_eq!(retained.capacity(), len);
            assert!(db.get(b"key", &mut value).unwrap());
            assert_eq!(value, expected);
            assert!(!db.get(b"absent", &mut value).unwrap());
            assert!(value.is_empty());
            db.put(b"key", b"replacement").unwrap();
            assert!(db.get(b"key", &mut value).unwrap());
            assert_eq!(value, b"replacement");
            assert_eq!(retained, expected);
        }
    }

    #[test]
    fn cursor_rows_stay_owned_across_another_cursor_and_a_put() {
        let db = Db::open_in_memory_tree().unwrap();
        db.put(b"a", b"original").unwrap();
        db.put(b"b", b"second").unwrap();
        let mut cursor = db.cursor().unwrap();
        let row = cursor.get(Seek::First).unwrap().unwrap();
        {
            let mut other = db.cursor().unwrap();
            assert_eq!(
                other.get(Seek::AtOrAfter(b"b")).unwrap().unwrap().value,
                b"second"
            );
            db.put(b"a", b"changed").unwrap();
        }
        assert_eq!(
            (row.key, row.value),
            (b"a".as_slice(), b"original".as_slice())
        );
        assert_eq!(cursor.get(Seek::First).unwrap().unwrap().value, b"changed");
    }

    #[test]
    fn cursor_values_survive_growth_and_misses() {
        struct TempPath(std::path::PathBuf);
        impl Drop for TempPath {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        // File-backed, as in the point-read test, to keep large-record
        // spill files inside the test's chosen scratch directory.
        let path = TempPath(
            std::env::temp_dir().join(format!("oxpinyin-cursor-values-{}.db", std::process::id())),
        );
        let db = Db::open(&path.0, DB_BTREE, false, true, USER_FILE_MODE).unwrap();
        db.put(b"a", b"first").unwrap();
        db.put(b"z", b"last").unwrap();
        for len in [0, 1, INITIAL_BUFFER, INITIAL_BUFFER + 1, 397_899] {
            let expected: Vec<u8> = (0..len).map(|index| (index % 251) as u8).collect();
            db.put(b"m", &expected).unwrap();
            let mut cursor = db.cursor().unwrap();
            // Missing seek target between rows, exact seek, and DB_NEXT
            // all reach the same value, including DB_BUFFER_SMALL.
            for seek in [Seek::AtOrAfter(b"l"), Seek::AtOrAfter(b"m")] {
                let row = cursor.get(seek).unwrap().unwrap();
                assert_eq!((row.key, row.value), (b"m".as_slice(), expected.as_slice()));
            }
            assert_eq!(cursor.get(Seek::First).unwrap().unwrap().value, b"first");
            assert_eq!(cursor.get(Seek::Next).unwrap().unwrap().value, expected);
            assert_eq!(cursor.get(Seek::Next).unwrap().unwrap().value, b"last");
            assert!(cursor.get(Seek::Next).unwrap().is_none());
            assert!(cursor.get(Seek::AtOrAfter(b"zz")).unwrap().is_none());
        }
    }

    #[test]
    fn key_walks_visit_the_row_walk_order_without_reading_values() {
        struct TempPath(std::path::PathBuf);
        impl Drop for TempPath {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        // File-backed, as in the tests above, for the overflow record.
        let path = TempPath(
            std::env::temp_dir().join(format!("oxpinyin-key-walk-{}.db", std::process::id())),
        );
        let db = Db::open(&path.0, DB_HASH, false, true, USER_FILE_MODE).unwrap();
        for (index, len) in [0, 1, INITIAL_BUFFER, INITIAL_BUFFER + 1, 397_899]
            .into_iter()
            .enumerate()
        {
            let value: Vec<u8> = (0..len).map(|byte| (byte % 251) as u8).collect();
            db.put(&u32::try_from(index).unwrap().to_le_bytes(), &value)
                .unwrap();
        }
        let mut rows = Vec::new();
        let mut cursor = db.cursor().unwrap();
        let mut seek = Seek::First;
        while let Some(row) = cursor.get(seek).unwrap() {
            rows.push(row.key.to_vec());
            seek = Seek::Next;
        }
        drop(cursor);
        let mut keys = Vec::new();
        let mut cursor = db.cursor().unwrap();
        let mut seek = Seek::First;
        while let Some(key) = cursor.key(seek).unwrap() {
            keys.push(key.to_vec());
            seek = Seek::Next;
        }
        assert_eq!(keys, rows);
        assert_eq!(keys.len(), 5);
        // No value was copied, so the value buffer never grew past one page.
        assert!(cursor.value_buf.len() <= INITIAL_BUFFER);
    }

    #[test]
    fn cursor_seek_keys_survive_growth_and_retries() {
        // Test key-buffer growth independently from large-value disk I/O
        // in the file-backed test above. Both DB_SET_RANGE input/output
        // and DB_NEXT output must survive key and value buffer retries.
        let db = Db::open_in_memory_tree().unwrap();
        let long_key = vec![b'm'; INITIAL_BUFFER + 1];
        db.put(b"a", b"first").unwrap();
        db.put(b"z", b"last").unwrap();
        for len in [0, 1, INITIAL_BUFFER, INITIAL_BUFFER + 1] {
            let expected: Vec<u8> = (0..len).map(|index| (index % 251) as u8).collect();
            db.put(&long_key, &expected).unwrap();
            {
                let mut cursor = db.cursor().unwrap();
                for seek in [Seek::AtOrAfter(b"m"), Seek::AtOrAfter(&long_key)] {
                    let row = cursor.get(seek).unwrap().unwrap();
                    assert_eq!(
                        (row.key, row.value),
                        (long_key.as_slice(), expected.as_slice())
                    );
                }
                assert_eq!(cursor.get(Seek::First).unwrap().unwrap().value, b"first");
                let row = cursor.get(Seek::Next).unwrap().unwrap();
                assert_eq!(
                    (row.key, row.value),
                    (long_key.as_slice(), expected.as_slice())
                );
            }
        }
    }

    #[test]
    fn empty_and_missing_ranges_stay_empty() {
        use crate::{BdbStore, RawReadStore, WriteStore, WriteTxn};
        use std::ops::Bound::{Excluded, Included, Unbounded};
        let store = BdbStore::create_in_memory().unwrap();
        assert_eq!(store.count_raw().unwrap(), 0);
        store.write(|txn| txn.put_raw(b"b", b"value")).unwrap();
        for (lo, hi) in [
            (Unbounded, Excluded(b"b".as_slice())),
            (Included(b"b".as_slice()), Excluded(b"b".as_slice())),
            (Included(b"c".as_slice()), Unbounded),
        ] {
            let mut count = 0;
            store
                .range_raw(lo, hi, |_, _| {
                    count += 1;
                    Ok(())
                })
                .unwrap();
            assert_eq!(count, 0);
        }
        let mut seen = Vec::new();
        store
            .range_raw(Included(b"a"), Unbounded, |key, value| {
                seen.push((key.to_vec(), value.to_vec()));
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, vec![(b"b".to_vec(), b"value".to_vec())]);
    }

    #[test]
    fn a_read_only_handle_refuses_put_and_del() {
        struct TempPath(std::path::PathBuf);
        impl Drop for TempPath {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        // `Bigram::store` and `remove`, and the `FlexibleBigram` twins, are a
        // bare `DB->put` / `DB->del` (ngram_bdb.cpp:161, :174;
        // flexible_ngram_bdb.h:212, :233 at 074a2219); on a handle opened
        // `DB_RDONLY` (bdb_utils.h:32-33) libdb refuses them and the method
        // answers false. The wrapper's answer is `ReadOnly`, and the record
        // is unchanged.
        let path = TempPath(
            std::env::temp_dir().join(format!("oxpinyin-read-only-{}.db", std::process::id())),
        );
        let db = Db::open(&path.0, DB_HASH, false, true, USER_FILE_MODE).unwrap();
        db.put(b"key", b"value").unwrap();
        drop(db);
        let db = Db::open(&path.0, DB_HASH, true, false, USER_FILE_MODE).unwrap();
        assert!(matches!(
            db.put(b"key", b"other"),
            Err(StoreError::ReadOnly)
        ));
        assert!(matches!(db.del(b"key"), Err(StoreError::ReadOnly)));
        let mut value = Vec::new();
        assert!(db.get(b"key", &mut value).unwrap());
        assert_eq!(value, b"value");
    }

    #[test]
    fn unnamed_opens_use_the_pins_unthreaded_create_flags() {
        for db in [
            Db::open_in_memory_tree().unwrap(),
            Db::open_in_memory_hash_attach().unwrap().unwrap(),
        ] {
            let get_flags = method!(db.handle, get_open_flags, "DB->get_open_flags").unwrap();
            let mut flags = 0;
            // SAFETY: the handle and output local are live, and nothing
            // else uses this database.
            check(unsafe { get_flags(db.handle, &raw mut flags) }, "get flags").unwrap();
            assert_eq!(flags, sys::DB_CREATE);
        }
    }
}
