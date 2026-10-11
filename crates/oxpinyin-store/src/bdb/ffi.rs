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
//! **`Send`/`Sync`.** [`Db`] is `Send`, but not `Sync`: its standalone
//! handle is used by one thread at a time. [`SharedDb`] owns it behind
//! a Rust `Mutex`, including every cursor operation and cursor close.
//! Shared readers therefore get `Sync` without native threaded opens.
//! Cursors borrow that owner and never cross threads. The lock is
//! released before a row reaches a visitor, so visitors can read the
//! same store. `DB_DBT_USERMEM` keeps point-read and cursor output in
//! caller-owned buffers that remain valid across those intervening reads.
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
use std::sync::{Mutex, MutexGuard};

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
///
/// `OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION`, when set to a non-empty
/// value, covers this comparison and nothing else: the open continues,
/// and the process emits one warning naming the linked version and the
/// surveyed one. Unset or blank, the refusal above is unchanged. It
/// exists so the Windows CI lane can link vcpkg's 4.8.30; a developer
/// build does not set it. See `docs/runbooks/backends.md`.
pub(crate) fn check_runtime_version() -> Result<(), StoreError> {
    let (major, minor, _) = runtime_version();
    if (major, minor) == (SUPPORTED_MAJOR, SUPPORTED_MINOR) {
        return Ok(());
    }
    if allow_unsurveyed_version() {
        // One line per process, not per open: a suite that opens a store
        // on every test would otherwise bury the warning in repetition,
        // and a single line is enough that the log cannot look clean.
        static WARNED: std::sync::Once = std::sync::Once::new();
        WARNED.call_once(|| {
            eprintln!(
                "warning: OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION is set: linked Berkeley DB \
                 {major}.{minor}, surveyed version is {SUPPORTED_MAJOR}.{SUPPORTED_MINOR}"
            );
        });
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

/// The version-survey opt-in. Any other `OXPINYIN_BDB_*` name is a path
/// or a library name; this one is not, and it is read only here.
fn allow_unsurveyed_version() -> bool {
    matches!(
        std::env::var("OXPINYIN_BDB_ALLOW_UNSURVEYED_VERSION"),
        Ok(value) if !value.trim().is_empty()
    )
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

/// A shareable owner that serializes all use of its standalone handle.
pub(crate) struct SharedDb {
    inner: Mutex<Db>,
    read_only: bool,
    hash: bool,
}

impl SharedDb {
    fn new(db: Db) -> Self {
        Self {
            read_only: db.is_read_only(),
            hash: db.is_hash(),
            inner: Mutex::new(db),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Db> {
        // No user callback runs under this lock. A Rust unwind cannot
        // leave a native call in flight, so poisoning does not prevent
        // the next exclusive use or cursor cleanup.
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// Opens a whole-file database with the pin's flags and file mode.
    pub(crate) fn open(
        path: &Path,
        db_type: DBTYPE,
        read_only: bool,
        create: bool,
        mode: c_int,
    ) -> Result<Self, StoreError> {
        Db::open(path, db_type, read_only, create, mode).map(Self::new)
    }

    /// Opens the user bigram's unnamed hash database.
    pub(crate) fn open_in_memory_hash() -> Result<Self, StoreError> {
        Db::open_in_memory_hash().map(Self::new)
    }

    /// Opens the session's unnamed ordered database.
    pub(crate) fn open_in_memory_tree() -> Result<Self, StoreError> {
        Db::open_in_memory_tree().map(Self::new)
    }

    pub(crate) const fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub(crate) const fn is_hash(&self) -> bool {
        self.hash
    }

    pub(crate) fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        self.lock().get(key)
    }

    pub(crate) fn put(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.lock().put(key, value)
    }

    pub(crate) fn del(&self, key: &[u8]) -> Result<(), StoreError> {
        self.lock().del(key)
    }

    pub(crate) fn sync(&self) -> Result<(), StoreError> {
        self.lock().sync()
    }

    /// The cursor borrows the owner, rather than holding its lock across
    /// visitor callbacks. Each native cursor call acquires the same lock.
    pub(crate) fn cursor(&self) -> Result<Cursor<'_>, StoreError> {
        let handle = self.lock().cursor_handle()?;
        Ok(Cursor {
            handle,
            key_buf: vec![0_u8; INITIAL_BUFFER],
            value_buf: vec![0_u8; INITIAL_BUFFER],
            db: self,
        })
    }
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
    /// (`ngram_bdb.cpp:51-56`: `DB_HASH, DB_CREATE, 0600`).
    pub(crate) fn open_in_memory_hash() -> Result<Self, StoreError> {
        Self::open_at(None, DB_HASH, false, true, USER_FILE_MODE)
    }

    /// An in-memory `DB_BTREE` — the same null-file-name form as
    /// [`Self::open_in_memory_hash`], ordered: the session store a
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
        check_runtime_version()?;

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
        let mut this = Self {
            handle,
            read_only,
            hash: db_type == DB_HASH,
            opened: false,
        };

        let mut flags = 0;
        if read_only {
            flags |= sys::DB_RDONLY;
        } else if create {
            flags |= sys::DB_CREATE;
        }
        // An existing file must not be silently re-created as a different
        // type; libdb checks that itself when DB_CREATE is absent.

        let open = method!(this.handle, open, "DB->open")?;
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
        check(code, "DB->open")?;
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

    /// Point read. `None` when the key is absent.
    ///
    /// The value lands in a caller-owned buffer (`DB_DBT_USERMEM`),
    /// grown once on `DB_BUFFER_SMALL` and returned as an owned `Vec`.
    /// This buffer remains valid after another operation uses the handle.
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
                // The same self-contradiction guard `Cursor::get` carries:
                // libdb reports the size it needed, so the buffer grows and
                // the retry fits. A `DB_BUFFER_SMALL` whose reported size
                // the buffer already covers would otherwise spin forever.
                let was = buf.len();
                grow(&mut buf, value_dbt.size);
                if buf.len() == was {
                    return Err(StoreError::Backend(
                        "DB->get keeps returning DB_BUFFER_SMALL with a buffer it says fits".into(),
                    ));
                }
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

    /// Flush to stable storage. `DB->sync` is libdb's only flush for a
    /// handle opened with no environment — which syscall it issues is
    /// the linked libdb's build-time choice (`__os_fsync` picks one per
    /// its configure result), not this binding's to claim — so this one
    /// call serves both roles: the commit-visible sync `write` ends
    /// with and the device-visible sync `compact` owes. There is no
    /// softer tier to select.
    pub(crate) fn sync(&self) -> Result<(), StoreError> {
        if self.read_only {
            return Ok(());
        }
        let sync = method!(self.handle, sync, "DB->sync")?;
        // SAFETY: the handle is live and the flags argument is the
        // documented zero.
        check(unsafe { sync(self.handle, 0) }, "DB->sync")
    }

    /// Opens a native cursor while the shareable owner's lock is held.
    fn cursor_handle(&self) -> Result<*mut sys::DBC, StoreError> {
        let cursor = method!(self.handle, cursor, "DB->cursor")?;
        let mut handle: *mut sys::DBC = ptr::null_mut();
        // SAFETY: the database handle is live; the transaction is null;
        // the out-parameter is a live local, and the owner serializes
        // this call with every other use of the database and its cursors.
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

// SAFETY: ownership may move between threads, but native use is exclusive.
// SharedDb's Mutex covers every DB/DBC call; its release/acquire establishes
// happens-before for the handle and its implicit private environment. Db is
// not Sync. The following audit is of Berkeley DB 5.3.28 at libdb commit
// e66a5e66d5a8db89d13532012f28d17d375e8564 (paths relative to that tree):
//
// - src/db/db.c:472-498 opens only DB_INIT_MPOOL | DB_PRIVATE (plus CREATE),
//   not locking/transactions; src/env/env_open.c:1182-1191 gates their opens.
//   Thus src/db/db_am.c:106-125,237-282 cannot create thread-affine lockers.
//   db_create's zero flags also exclude XA (src/db/db_method.c:94-134).
// - Without thread tracking, src/env/env_failchk.c:214-222 leaves thr_hashtab
//   null and src/dbinc/db_int.in:615-639 makes ENV_ENTER/GET_THREAD_INFO yield
//   null, so no caller entry is created or retained by a cursor. The cached
//   PID is process-wide on these platforms, not a thread ID
//   (src/os/os_pid.c:29-55); the DB/ENV have no current-caller pointer
//   (src/dbinc/db.in:1433-1517; src/dbinc/db_int.in:772-886).
// - DB_CONFIG can enable tracking, as in the pin. Every ENV_ENTER starts
//   with a null local and looks up the current caller afresh: the callback,
//   PID/TID comparison and returned entry are in src/env/env_failchk.c:
//   404-440,458-511. Neither DB nor ENV retains that local for another call;
//   environment close also resolves the current caller
//   (src/env/env_open.c:858-860).
// - Only DBC retains a caller pointer on these paths. Cursor checkout,
//   including reuse from the free queue, resets it before use
//   (src/db/db_am.c:87-103,395-409). Rust Cursor's raw pointer makes it
//   neither Send nor Sync; get and Drop stay on its creating thread, and its
//   owner borrow prevents database close while it is live. Native internal
//   get/put/del cursors close in their current call (src/db/db_iface.c:
//   783-813; src/db/db_am.c:465,585-589,622,781-785). Duplicates inherit that
//   same pointer (src/db/db_cam.c:613-616,720-722). DB close destroys closed
//   cached cursors without using their old thread_info (src/db/db.c:897-901;
//   src/db/db_cam.c:180-216; src/btree/bt_cursor.c:662-688;
//   src/hash/hash.c:255-265).
// - Output DBTs always use DB_DBT_USERMEM: src/db/db_ret.c:141-158 copies
//   into caller-owned buffers, so no native return buffer escapes the lock
//   or becomes invalid after an intervening call. Cursor rows borrow those
//   buffers, rather than BDB pages. Close/cleanup is serialized as well.
unsafe impl Send for Db {}

impl Drop for Db {
    fn drop(&mut self) {
        // libpinyin's `reset()` is `sync` then `close`, and a `close`
        // failure has nowhere to go in `Drop` — libdb frees the handle
        // regardless of what `close` returns, so the codes are
        // discarded. Callers who need to know that a flush succeeded
        // call `sync` first, which does report.
        if self.opened
            && !self.read_only
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
    /// Reusable key buffer: grows to the largest key seen, never shrinks.
    key_buf: Vec<u8>,
    /// Reusable value buffer: same policy.
    value_buf: Vec<u8>,
    db: &'db SharedDb,
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
        let _guard = self.db.lock();
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
            // `key_buf` at the top of this iteration. The owner lock
            // excludes every other database or cursor call until return.
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
        let _guard = self.db.lock();
        if let Ok(close) = method!(self.handle, close, "DBC->close") {
            // SAFETY: the cursor handle is live and owned, its database
            // outlives it by the `'db` bound, and this is its last use.
            // The owner lock also serializes close with all other calls.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    #[test]
    fn unnamed_opens_use_the_pins_unthreaded_create_flags() {
        for db in [
            SharedDb::open_in_memory_tree().unwrap(),
            SharedDb::open_in_memory_hash().unwrap(),
        ] {
            let guard = db.lock();
            let get_flags = method!(guard.handle, get_open_flags, "DB->get_open_flags").unwrap();
            let mut flags = 0;
            // SAFETY: the handle and output local are live, and the
            // guard excludes every other use of this database.
            check(
                unsafe { get_flags(guard.handle, &raw mut flags) },
                "get flags",
            )
            .unwrap();
            assert_eq!(flags, sys::DB_CREATE);
        }
    }

    #[test]
    fn shared_point_reads_wait_for_exclusive_native_access() {
        let db = Arc::new(SharedDb::open_in_memory_tree().unwrap());
        db.put(b"key", b"value").unwrap();
        let guard = db.lock();
        let (started_tx, started_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let reader = Arc::clone(&db);
        let thread = std::thread::spawn(move || {
            assert!(matches!(
                reader.inner.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            started_tx.send(()).unwrap();
            finished_tx.send(reader.get(b"key").unwrap()).unwrap();
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            finished_rx.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        drop(guard);
        assert_eq!(
            finished_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            Some(b"value".to_vec())
        );
        thread.join().unwrap();
    }
}
