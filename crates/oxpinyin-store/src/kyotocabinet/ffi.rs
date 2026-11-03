//! Safe owners over the Kyoto Cabinet bridge (`bridge.rs`).
//!
//! This is the one module that calls the bridge. Everything above it is
//! safe Rust. The `expect` below is scoped to this module: it carries the
//! `unsafe impl Send`/`Sync` on [`Db`] and the calls to the bridge's five
//! `unsafe fn`s, each with its SAFETY note.
//!
//! # Safety claims (ruling point 1)
//!
//! The bridge declares every function safe to call except the five that
//! take a key region. The claims:
//!
//! - The object is non-null for the whole life of a [`Db`]: the
//!   `new_*_db` constructors return a fresh allocation (`std::make_unique`,
//!   which throws rather than return null), `UniquePtr` keeps it, and only
//!   [`Db::delete_unopened`], which consumes the owner, releases it before `Drop`.
//!   A never-opened object is a valid receiver: every method tests `omode_`
//!   under the object's lock before it touches anything else
//!   (`kchashdb.h:556-566,933-939,969-976`; `kcplantdb.h:1048-1052,
//!   1590-1596,1643-1648`; `kcprotodb.h:804-812`; `kcstashdb.h:706-712`).
//! - `open` and `close` take `Pin<&mut BasicDB>`: they run with exclusive
//!   access, before the object is shared and in `Drop`. The record
//!   operations take `&BasicDB` and mutate the database behind it under its
//!   own lock (see the `Send`/`Sync` note).
//! - Paths cross as `std::string`s copied from bytes in which [`cxx_path`]
//!   has refused NUL, at which the library would truncate them
//!   (`kchashdb.h:799` opens `path.c_str()`). `synchronize`, `iterate` and
//!   the snapshot calls pass no file processor and no progress checker, as
//!   the pin does.
//! - The regions the bridge's visitor hands [`Sink::full`] are borrowed for
//!   that call only (ruling point 4, `adapter.cc`); `Sink` copies them.
//!
//! The five region functions (`check`, `get`, `set`, `remove`, `jump_to`)
//! are `unsafe fn`: Kyoto Cabinet's contract for a key or value region is
//! `size <= MEMMAXSIZ` (`kcutil.h:133`), asserted at entry
//! (`kchashdb.h:557`, `kcplantdb.h:1043`, `kcplantdb.h:223`) and relied on
//! by `TreeDB`, which stores a record's sizes as `uint32_t`
//! (`kcplantdb.h:2294-2297`). [`region`] is the one place that discharges
//! it; a longer region is [`StoreError::InvalidInput`] before any call.
//!
//! # Status
//!
//! A native answer is the library's: a `bool`, a size of -1, a count. The
//! reason is the thread's own `BasicDB::error()`, read on the same thread
//! straight after the call: `sys::error_code` for a decision on the code,
//! which copies nothing, and `sys::last_error` only where a failure's text
//! is reported. A miss and a failure are
//! both a false or -1 answer; the code (`NOREC`) is the only way to tell
//! them apart (`kcdb.h:1862-1885,2020-2043`).
//!
//! **Allocation failure** in a visitor callback aborts the process, as
//! `new[]` failing inside libpinyin's own reads does: the copies use
//! `reserve_exact`, never a fallible reservation turned into an error.
#![expect(
    unsafe_code,
    reason = "Send/Sync for the native object and calls to the bridge's region functions; every block carries a SAFETY comment"
)]

use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use cxx::{UniquePtr, let_cxx_string};

use super::bridge::sys;
use crate::StoreError;

/// The Kyoto Cabinet major.minor this backend's format survey covers.
const SUPPORTED: &str = "1.2";

/// The linked library's version string (`kyotocabinet::VERSION`).
pub(crate) fn runtime_version() -> String {
    sys::version()
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

/// `kyotocabinet::MEMMAXSIZ`, `INT32MAX / 2` (`kcutil.h:133`). A
/// `static_assert` in `adapter.cc` holds this to the library's constant, so
/// [`region`] bounds a slice without a bridge call.
pub(super) const MEM_MAX_SIZE: usize = 0x3fff_ffff;

/// A key or value region the bridge's region functions may receive: at
/// most `MEMMAXSIZ` bytes, Kyoto Cabinet's contract for every region
/// (`kcutil.h:133`, asserted at `kchashdb.h:557` and `kcplantdb.h:1043`).
/// The one place the bridge's `unsafe fn` precondition is discharged.
fn region(bytes: &[u8]) -> Result<&[u8], StoreError> {
    if bytes.len() <= MEM_MAX_SIZE {
        Ok(bytes)
    } else {
        Err(StoreError::InvalidInput(
            "a Kyoto Cabinet key or value is longer than MEMMAXSIZ",
        ))
    }
}

/// A path's bytes, refused when they hold NUL: `open` and the snapshot
/// calls hand the library `path.c_str()`, which would end at the first NUL.
fn cxx_path(path: &Path) -> Result<&[u8], StoreError> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        return Err(StoreError::InvalidInput("store path contains NUL"));
    }
    Ok(bytes)
}

/// Which Kyoto Cabinet class an owner allocates.
///
/// The class comes from the caller, as libpinyin allocates it, so the file
/// name plays no part in the choice (see [`Db::open`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DbType {
    /// `TreeDB`, ordered by the record comparator.
    Tree,
    /// `HashDB`, unordered — used by `bigram.db`.
    Hash,
    /// `StashDB`, in-memory — the container libpinyin's Kyoto Cabinet
    /// *user* bigram lives in between snapshot load and dump
    /// (`ngram_kyotodb.cpp:54-108`).
    Stash,
    /// ProtoTreeDB, the pin's in-memory user index container.
    ProtoTree,
}

impl DbType {
    /// The pin's `new TreeDB` / `new HashDB` / `new StashDB` /
    /// `new ProtoTreeDB`, untuned and not opened.
    fn allocate(self) -> UniquePtr<sys::BasicDB> {
        match self {
            Self::Tree => sys::new_tree_db(),
            Self::Hash => sys::new_hash_db(),
            Self::Stash => sys::new_stash_db(),
            Self::ProtoTree => sys::new_proto_tree_db(),
        }
    }
}

/// `BasicDB::OREADER`, `OWRITER` and `OCREATE`, the bits the pin's
/// `attach` builds (`kyotodb_utils.h:32-45`, `flexible_ngram_kyotodb.h:
/// 131-152`).
pub(crate) const OREADER: u32 = sys::OpenMode::OREADER.repr;
pub(crate) const OWRITER: u32 = sys::OpenMode::OWRITER.repr;
pub(crate) const OCREATE: u32 = sys::OpenMode::OCREATE.repr;

/// The open mode the pin's `attach_options` builds (`kyotodb_utils.h:
/// 32-45`): `OREADER` for read-only, `OREADER | OWRITER` otherwise, and
/// `OCREATE` added when the file may be created. Kyoto Cabinet tests only
/// `OWRITER` when both are present (`kchashdb.h:799-822`,
/// `kcplantdb.h:1491-1520`, `kcprotodb.h:750-763`, `kcstashdb.h:640-660`).
fn attach_mode(read_only: bool, create: bool) -> u32 {
    let mut mode = OREADER;
    if !read_only {
        mode |= OWRITER;
        if create {
            mode |= OCREATE;
        }
    }
    mode
}

/// Where the bridge's visitor copies what it visits: one point read, the
/// record at a cursor, every record, or every key.
pub(super) enum Sink {
    /// A point read into the caller's buffer, which arrives cleared.
    Value { bytes: Vec<u8>, found: bool },
    /// A bounded point read, retaining the complete record length.
    Bounded {
        bytes: Vec<u8>,
        limit: usize,
        size: Option<usize>,
    },
    /// The record at a cursor, key then value, in a buffer the cursor
    /// reuses.
    Record {
        bytes: Vec<u8>,
        key_len: usize,
        with_value: bool,
    },
    /// Every record of a whole-file copy.
    Records(Vec<Record>),
    /// Every key, as the four-byte tokens the user bigram keys by; `None`
    /// for a key of any other length.
    Keys(Vec<Option<[u8; 4]>>),
    /// Every key, byte for byte, as the attached bigram's walk collects it.
    RawKeys(Vec<Vec<u8>>),
}

impl Sink {
    /// One visited record. Never a database call: the visitor runs under
    /// the record's lock (`kcdb.h:268-271`).
    pub(super) fn full(&mut self, key: &[u8], value: &[u8]) {
        match self {
            Self::Value { bytes, found } => {
                bytes.reserve_exact(value.len());
                bytes.extend_from_slice(value);
                *found = true;
            }
            Self::Bounded { bytes, limit, size } => {
                let count = value.len().min(*limit);
                bytes.extend_from_slice(&value[..count]);
                *size = Some(value.len());
            }
            Self::Record {
                bytes,
                key_len,
                with_value,
            } => {
                let value = if *with_value { value } else { &[] };
                bytes.clear();
                bytes.reserve_exact(key.len() + value.len());
                bytes.extend_from_slice(key);
                bytes.extend_from_slice(value);
                *key_len = key.len();
            }
            Self::Records(records) => {
                let mut bytes = Vec::new();
                bytes.reserve_exact(key.len() + value.len());
                bytes.extend_from_slice(key);
                bytes.extend_from_slice(value);
                records.push(Record {
                    bytes: bytes.into_boxed_slice(),
                    key_len: key.len(),
                });
            }
            Self::Keys(keys) => keys.push(<[u8; 4]>::try_from(key).ok()),
            Self::RawKeys(keys) => keys.push(key.to_vec()),
        }
    }
}

/// One record of a whole-file copy: key then value in one allocation,
/// so a consumer that takes the records by value frees each as it goes.
pub(crate) struct Record {
    bytes: Box<[u8]>,
    key_len: usize,
}

impl Record {
    pub(crate) fn key(&self) -> &[u8] {
        &self.bytes[..self.key_len]
    }

    pub(crate) fn value(&self) -> &[u8] {
        &self.bytes[self.key_len..]
    }
}

/// A table's rows, copied into one buffer by the cursor walks of
/// `WriteTxn::scan`.
pub(crate) struct Rows {
    bytes: Vec<u8>,
    /// `(start, key length, value length)` per record.
    spans: Vec<(usize, usize, usize)>,
}

impl Rows {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: Vec::new(),
            spans: Vec::new(),
        }
    }

    /// Appends one record.
    pub(crate) fn push(&mut self, key: &[u8], value: &[u8]) {
        self.spans.push((self.bytes.len(), key.len(), value.len()));
        self.bytes.extend_from_slice(key);
        self.bytes.extend_from_slice(value);
    }

    /// Record `index`'s key and value.
    pub(crate) fn get(&self, index: usize) -> (&[u8], &[u8]) {
        let (start, key_len, value_len) = self.spans[index];
        let key_end = start + key_len;
        (
            &self.bytes[start..key_end],
            &self.bytes[key_end..key_end + value_len],
        )
    }

    pub(crate) fn len(&self) -> usize {
        self.spans.len()
    }
}

/// A native call on one object, as the tests observe it.
#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum NativeCall {
    Open(Vec<u8>, u32),
    Check,
    Set,
    Count,
    Iterate,
    Synchronize(bool),
    Close,
    Delete,
}

#[cfg(test)]
thread_local! {
    // Observe completed native calls, never the wrapper's early returns.
    static NATIVE_CALLS: std::cell::RefCell<Vec<(usize, NativeCall)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
fn record_native_call(object: usize, call: NativeCall) {
    NATIVE_CALLS.with(|calls| calls.borrow_mut().push((object, call)));
}

#[cfg(test)]
pub(super) fn take_native_calls() -> Vec<(usize, NativeCall)> {
    NATIVE_CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
}

/// One native database object, the pin's `BasicDB* m_db`: a concrete class
/// allocated by [`DbType::allocate`], opened — or not — by [`Db::open_mode`],
/// and torn down in `Drop` as its owner's `reset()` tears it down.
///
/// # Teardown (ruling point 5)
///
/// `Drop` makes the pin's `reset()`: `synchronize()` when the owner's pin
/// counterpart makes it, then `close()`, both answers ignored as the pin
/// ignores them, then the destruction. The pin's `reset()` synchronizes
/// before it closes for the system tables and the in-memory containers
/// (`chewing_large_table2_kyotodb.cpp:65-74`,
/// `phrase_large_table3_kyotodb.cpp:76-88`, `punct_table_kyotodb.cpp:37-49`,
/// `ngram_kyotodb.cpp:43-50`); those owners call [`Db::with_reset_sync`].
/// `FlexibleBigram` makes its `synchronize()` through
/// [`crate::WriteStore::flush`] (`flexible_ngram_kyotodb.h:86-92`), and its
/// object is destroyed after the close, which the pin leaves undone. Its
/// failed open without CREATE is [`Db::delete_unopened`]: the object is destroyed
/// unclosed (`flexible_ngram_kyotodb.h:145-150`), and `~HashDB` closes only
/// an open object (`kchashdb.h:532-546`). Kyoto Cabinet's destructors close
/// an open object implicitly (`kchashdb.h:532-546`, `kcplantdb.h:1018-1028`,
/// `kcprotodb.h:352`, `kcstashdb.h:324`); the explicit close keeps the pin's
/// position for it.
pub(crate) struct Db {
    db: UniquePtr<sys::BasicDB>,
    db_type: DbType,
    read_only: bool,
    /// Whether teardown calls `synchronize(false)` before `close()`.
    sync_before_close: bool,
}

// SAFETY: (ruling point 2) every record operation of the four classes takes
// the object's own reader-writer lock before it reads or writes anything
// (`RWLock mlock_`: `kchashdb.h:3753`, `kcplantdb.h:3879`, `kcprotodb.h:1255`,
// `kcstashdb.h:1438`; taken at `kchashdb.h:558`, `kcplantdb.h:1044-1048`,
// `kcprotodb.h:376`, `kcstashdb.h:348`, and in every cursor method, e.g.
// `kcplantdb.h:224`), and the class documents each operation as atomic with
// other threads blocked (`kcdb.h:268-271`). `begin_transaction` takes the
// writer lock and spins while another transaction is live
// (`kchashdb.h:1010-1032`). The error slot is per thread (`TSD<Error>`,
// `kchashdb.h:3761`, `kcprotodb.h:1257`, `kcstashdb.h:1444`; `TreeDB`
// forwards to its inner `HashDB`, `kcplantdb.h:1455-1458`), so the error
// this module reads after a call is that call's, whatever other threads do.
// `open` and `close` run with exclusive access (`&mut self`, `Drop`), and
// destruction follows the close. `BasicDB`'s own note forbids two objects
// on one file in one process and sharing an object with a child process,
// not sharing one between threads (`kcdb.h:466-479`). The owner holds no
// Rust-side state that the native calls touch.
unsafe impl Send for Db {}
// SAFETY: as `Send` — every operation reachable through `&Db` takes the
// object's own lock, and the error state each thread reads is its own.
unsafe impl Sync for Db {}

impl Db {
    /// The object's address, which the test recorder keys on.
    #[cfg(test)]
    pub(super) fn address(&self) -> usize {
        self.db
            .as_ref()
            .map_or(0, |db| std::ptr::from_ref(db) as usize)
    }

    /// Opens `path` as `db_type` with the pin's mode for `read_only` and
    /// `create`, and destroys the object if the open fails.
    ///
    /// # The class comes from the caller, not the file name
    ///
    /// libpinyin's files are named `bigram.db` and `user_bigram.db`
    /// (`src/pinyin_internal.h:56-58`) whatever DBM backend it was built
    /// against, and it constructs the class itself before opening the
    /// path (`ngram_kyotodb.cpp:58,117` at 074a2219). So does this.
    pub(crate) fn open(
        path: &Path,
        db_type: DbType,
        read_only: bool,
        create: bool,
    ) -> Result<Self, StoreError> {
        let (db, opened) = Self::open_retained(path, db_type, read_only, create)?;
        if !opened {
            return Err(db.error("kcdbopen"));
        }
        Ok(db)
    }

    /// Retains the concrete object when open fails, as the pin's system
    /// attach does (chewing_large_table2_kyotodb.cpp:87-89 and
    /// ngram_kyotodb.cpp:117-119 at 074a2219). KC's omode_ stays zero
    /// (kcplantdb.h:1516; kchashdb.h:823-832); later calls are valid
    /// object operations returning INVALID, not calls through a null DB.
    pub(crate) fn open_retained(
        path: &Path,
        db_type: DbType,
        read_only: bool,
        create: bool,
    ) -> Result<(Self, bool), StoreError> {
        // Refused before the allocation, as the pin returns before its
        // `new` for a null path. `new_unopened` checks the version.
        cxx_path(path)?;
        let mut db = Self::new_unopened(db_type, read_only)?;
        let opened = db.open_mode(path, attach_mode(read_only, create))?;
        Ok((db, opened))
    }

    /// A never-opened object of `db_type`: the pin's `new HashDB`
    /// (`flexible_ngram_kyotodb.h:143`). [`Self::open_mode`] opens it.
    ///
    /// # Errors
    ///
    /// An unsupported Kyoto Cabinet.
    pub(crate) fn new_unopened(db_type: DbType, read_only: bool) -> Result<Self, StoreError> {
        check_runtime_version()?;
        Ok(Self {
            db: db_type.allocate(),
            db_type,
            read_only,
            sync_before_close: false,
        })
    }

    /// `m_db->open(path, mode)` on this object, with the pin's own mode
    /// bits. The pin's CREATE retry calls it twice on one object
    /// (`flexible_ngram_kyotodb.h:145-155`); the second `open` reads the
    /// state the first left, such as `FFATAL` from `set_error`. On failure
    /// the object stays as the library left it, for the caller to keep or
    /// drop.
    ///
    /// # Errors
    ///
    /// A path that contains NUL, before any native call.
    pub(crate) fn open_mode(&mut self, path: &Path, mode: u32) -> Result<bool, StoreError> {
        let bytes = cxx_path(path)?;
        let_cxx_string!(native_path = bytes);
        let opened = self.db.pin_mut().open(&native_path, mode);
        #[cfg(test)]
        record_native_call(self.address(), NativeCall::Open(bytes.to_vec(), mode));
        Ok(opened)
    }

    /// Marks this owner as one whose pin `reset()` calls `synchronize()`
    /// before `close()`.
    pub(crate) const fn with_reset_sync(mut self) -> Self {
        self.sync_before_close = true;
        self
    }

    /// `delete m_db` with no `close`, as the pin deletes a `HashDB` whose
    /// open failed (`flexible_ngram_kyotodb.h:145-150`). Call it only after
    /// a failed open: `~HashDB` closes only an open object
    /// (`kchashdb.h:532-534`), so nothing else reaches the library. The
    /// virtual destructor runs now, and `Drop` finds nothing left.
    pub(crate) fn delete_unopened(mut self) {
        #[cfg(test)]
        let object = self.address();
        self.db = UniquePtr::null();
        #[cfg(test)]
        record_native_call(object, NativeCall::Delete);
    }

    /// The library's own account of the last failure on this handle.
    fn error(&self, what: &str) -> StoreError {
        let error = sys::last_error(&self.db);
        // Kyoto Cabinet reports filesystem trouble through its own codes
        // rather than errno, so everything lands on the backend arm; the
        // library's name for the code is preserved verbatim.
        StoreError::Backend(format!("{what}: {}: {}", error.name, error.message).into())
    }

    /// Whether the last operation failed because the key was absent
    /// rather than because something went wrong.
    ///
    /// Both are a false or -1 answer, so the code is the only way to tell
    /// them apart — and treating an error as a miss would turn a damaged
    /// file into silently empty data.
    fn last_was_no_record(&self) -> bool {
        sys::error_code(&self.db) == sys::ErrorCode::NOREC
    }

    /// Whether this handle was opened read-only.
    pub(crate) const fn is_read_only(&self) -> bool {
        self.read_only
    }

    /// Whether a cursor walks this class in key order (`TreeDB`,
    /// `ProtoTreeDB`) rather than in bucket order (`HashDB`, `StashDB`).
    pub(crate) const fn is_ordered(&self) -> bool {
        matches!(self.db_type, DbType::Tree | DbType::ProtoTree)
    }

    /// Point read into `value`, which is cleared first. `false` when the
    /// key is absent.
    ///
    /// One read-only `accept`: the visitor copies the value straight into
    /// the caller's buffer. libpinyin reads the same record with `check`
    /// and then `get(kbuf, ksiz, vbuf, vsiz)` into the chunk it reuses
    /// (`ngram_kyotodb.cpp:131-139` at 074a2219), two `accept`s whose
    /// results agree for a caller that does not write in between.
    pub(crate) fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        let key = region(key)?;
        value.clear();
        let mut sink = Sink::Value {
            bytes: std::mem::take(value),
            found: false,
        };
        // SAFETY: `region` bounds the key by MEMMAXSIZ.
        let ok = unsafe { sys::get(&self.db, key, &mut sink) };
        let Sink::Value { bytes, found } = sink else {
            unreachable!("the visitor keeps the sink's kind");
        };
        *value = bytes;
        if !ok {
            // The text `kcdbget` reported, so a failure reads as before.
            return Err(self.error("kcdbget"));
        }
        Ok(found)
    }

    /// Bounded point read using the same read-only accept visitor as
    /// BasicDB::get's bounded overload (KC 1.2.80 kcdb.h:1985-2012).
    pub(crate) fn get_bounded(
        &self,
        key: &[u8],
        value: &mut [u8],
    ) -> Result<Option<usize>, StoreError> {
        let key = region(key)?;
        let mut sink = Sink::Bounded {
            bytes: Vec::with_capacity(value.len()),
            limit: value.len(),
            size: None,
        };
        // SAFETY: `region` bounds the key by MEMMAXSIZ.
        let ok = unsafe { sys::get(&self.db, key, &mut sink) };
        let Sink::Bounded { bytes, size, .. } = sink else {
            unreachable!("the visitor keeps the sink's kind");
        };
        if !ok {
            return Err(self.error("kcdbget"));
        }
        value[..bytes.len()].copy_from_slice(&bytes);
        Ok(size)
    }

    /// Whether `key` exists, an empty value included, copying nothing:
    /// `BasicDB::check`, whose read-only visitor records the value's size
    /// and sets `NOREC` on a miss (`kcdb.h:2020-2043`). libpinyin's
    /// suggestion walk opens with the same call
    /// (`phrase_large_table3_kyotodb.cpp:206` at 074a2219).
    pub(crate) fn check(&self, key: &[u8]) -> Result<bool, StoreError> {
        self.check_size(key).map(|size| size.is_some())
    }

    /// `check`'s value size, or `None` for a missing record.
    fn check_size(&self, key: &[u8]) -> Result<Option<usize>, StoreError> {
        let key = region(key)?;
        // SAFETY: `region` bounds the key by MEMMAXSIZ.
        let size = unsafe { sys::check(&self.db, key) };
        #[cfg(test)]
        record_native_call(self.address(), NativeCall::Check);
        if size >= 0 {
            return Ok(usize::try_from(size).ok());
        }
        if self.last_was_no_record() {
            return Ok(None);
        }
        Err(self.error("kcdbcheck"))
    }

    /// `Bigram::load`'s checked read: the size check, then the body read
    /// into exactly that size. A check that finds no record (or fails) is
    /// a silent miss, as `Bigram::load` treats a failed first read
    /// (`ngram_kyotodb.cpp:131-134`); a body read that fails or returns a
    /// different size after the check succeeded is
    /// [`StoreError::ReadAfterCheck`], the pin's
    /// `check_result(vsiz == get(...))` assert (`ngram_kyotodb.cpp:131-139`).
    pub(crate) fn get_bigram(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        let Some(size) = self.check_size(key).unwrap_or(None) else {
            return Ok(false);
        };
        match self.get(key, value) {
            Ok(true) if value.len() == size => Ok(true),
            _ => Err(StoreError::ReadAfterCheck),
        }
    }

    /// Every record, copied out by one read-only `iterate`, in the class's
    /// own walk order, into a vector reserved for `count` records.
    ///
    /// The iteration holds the database's lock throughout; the copy is all
    /// that runs under it.
    pub(crate) fn records(&self) -> Result<Vec<Record>, StoreError> {
        let mut records = Vec::new();
        records.reserve_exact(usize::try_from(self.count()?).unwrap_or(0));
        let Sink::Records(records) = self.iterate(Sink::Records(records))? else {
            unreachable!("the visitor keeps the sink's kind");
        };
        Ok(records)
    }

    /// Every key, in the class's walk order: `Bigram::get_all_items`'
    /// one native call, `iterate(&visitor, false)`, with no `count` before
    /// it (`ngram_kyotodb.cpp:186-196`). The keys grow one vector, as the
    /// pin appends them to one `GArray`.
    pub(crate) fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError> {
        let Sink::Keys(keys) = self.iterate(Sink::Keys(Vec::new()))? else {
            unreachable!("the visitor keeps the sink's kind");
        };
        Ok(keys)
    }

    /// [`Self::keys`] with each key's own bytes, whatever its length: the
    /// walk of an attached bigram, whose caller judges the key length.
    pub(crate) fn raw_keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        let Sink::RawKeys(keys) = self.iterate(Sink::RawKeys(Vec::new()))? else {
            unreachable!("the visitor keeps the sink's kind");
        };
        Ok(keys)
    }

    fn iterate(&self, mut sink: Sink) -> Result<Sink, StoreError> {
        let ok = sys::iterate(&self.db, &mut sink);
        #[cfg(test)]
        record_native_call(self.address(), NativeCall::Iterate);
        if !ok {
            return Err(self.error("kcdbiterate"));
        }
        Ok(sink)
    }

    /// Insert or overwrite.
    pub(crate) fn set(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let (key, value) = (region(key)?, region(value)?);
        // SAFETY: `region` bounds the key and the value by MEMMAXSIZ.
        let ok = unsafe { sys::set(&self.db, key, value) };
        #[cfg(test)]
        record_native_call(self.address(), NativeCall::Set);
        if !ok {
            return Err(self.error("kcdbset"));
        }
        Ok(())
    }

    /// Remove `key`. `Ok(false)` when the record was absent (`NOREC`), the
    /// answer the pin's `remove` returns (`flexible_ngram_kyotodb.h:
    /// 246-252`, `kcdb.h:1862-1885`).
    pub(crate) fn remove(&self, key: &[u8]) -> Result<bool, StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let key = region(key)?;
        // SAFETY: `region` bounds the key by MEMMAXSIZ.
        if unsafe { sys::remove(&self.db, key) } {
            return Ok(true);
        }
        if self.last_was_no_record() {
            return Ok(false);
        }
        Err(self.error("kcdbremove"))
    }

    /// `synchronize(hard)`, the native call whatever the open mode: on a
    /// reader it takes the locks and returns true (`kchashdb.h:969-983`,
    /// `kcplantdb.h:1641-1700`).
    pub(crate) fn sync(&self, hard: bool) -> Result<(), StoreError> {
        if !self.synchronize(hard) {
            return Err(self.error("kcdbsync"));
        }
        Ok(())
    }

    fn synchronize(&self, hard: bool) -> bool {
        let ok = sys::synchronize(&self.db, hard);
        #[cfg(test)]
        record_native_call(self.address(), NativeCall::Synchronize(hard));
        ok
    }

    /// Reads a snapshot stream (`KCSS`) into this database —
    /// `load_snapshot(src)`.
    ///
    /// libpinyin's Kyoto Cabinet *user* bigram is this and nothing else:
    /// `Bigram::load_db` opens an in-memory `StashDB` and calls
    /// `load_snapshot(dbfile)` on it (`ngram_kyotodb.cpp:54-64`), so the
    /// file on disk is a snapshot of records, not a `HashDB` — its magic
    /// is `KCSS`, and opening it as a hash database fails with "missing
    /// magic data of the file". The *system* bigram is a genuine `HashDB`
    /// file (`attach`, `ngram_kyotodb.cpp:110-119`); the two are different
    /// containers and must not share one open path.
    pub(crate) fn load_snapshot(&self, path: &Path) -> Result<(), StoreError> {
        let_cxx_string!(native_path = cxx_path(path)?);
        if !sys::load_snapshot(&self.db, &native_path) {
            return Err(self.error("kcdbloadsnap"));
        }
        Ok(())
    }

    /// Writes this database's records out as a snapshot stream (`KCSS`) —
    /// `dump_snapshot(dest)`, the inverse of [`Self::load_snapshot`] and
    /// the format `Bigram::save_db` leaves behind
    /// (`ngram_kyotodb.cpp:82-101`: `unlink` then `dump_snapshot`).
    pub(crate) fn dump_snapshot(&self, path: &Path) -> Result<(), StoreError> {
        let_cxx_string!(native_path = cxx_path(path)?);
        if !sys::dump_snapshot(&self.db, &native_path) {
            return Err(self.error("kcdbdumpsnap"));
        }
        Ok(())
    }

    /// Marks this handle read-only, in place, after a load-time fill.
    ///
    /// The user-bigram open loads a snapshot into a writable in-memory
    /// stash (the stash class opens writer, and `load_snapshot` needs
    /// that); the seam itself is read-only, so the fill ends by sealing
    /// the handle — a later `write` on it answers [`StoreError::ReadOnly`]
    /// instead of mutating memory that nothing would ever dump.
    pub(crate) fn into_read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// An in-memory `StashDB` opened as `"-"` with the pin's mode, the
    /// container libpinyin's KC user bigram lives in between its snapshot
    /// load and dump (`ngram_kyotodb.cpp:58-60`).
    pub(crate) fn open_stash() -> Result<Self, StoreError> {
        Self::open(Path::new("-"), DbType::Stash, false, true)
    }

    /// An in-memory `ProtoTreeDB` opened as `"-"` with the pin's mode, the
    /// container of both user indexes (`chewing_large_table2_kyotodb.cpp:
    /// 100-102`, `phrase_large_table3_kyotodb.cpp:115-117`).
    pub(crate) fn open_proto_tree() -> Result<Self, StoreError> {
        Self::open(Path::new("-"), DbType::ProtoTree, false, true)
    }

    /// The number of records — `count()`, -1 on failure.
    pub(crate) fn count(&self) -> Result<u64, StoreError> {
        let count = sys::count(&self.db);
        #[cfg(test)]
        record_native_call(self.address(), NativeCall::Count);
        u64::try_from(count).map_err(|_| self.error("kcdbcount"))
    }

    /// Begins a transaction.
    ///
    /// Unlike the Berkeley DB backend — where libpinyin's environment-less
    /// opens leave no transaction to use — Kyoto Cabinet gives a
    /// standalone handle real transactions, so this backend's atomic
    /// writes are the library's own rather than a buffered imitation.
    pub(crate) fn begin_transaction(&self) -> Result<(), StoreError> {
        // `hard = false` asks for a transaction durable against process
        // crash but not against power loss, which matches what the
        // commit's `sync(false)` then provides.
        if !sys::begin_transaction(&self.db, false) {
            return Err(self.error("kcdbbegintran"));
        }
        Ok(())
    }

    /// Ends a transaction, committing or rolling back.
    pub(crate) fn end_transaction(&self, commit: bool) -> Result<(), StoreError> {
        if !sys::end_transaction(&self.db, commit) {
            return Err(self.error("kcdbendtran"));
        }
        Ok(())
    }

    /// Opens a cursor, which borrows this database. `cursor()` allocates
    /// with `new` (`kchashdb.h:1242`, `kcplantdb.h:1952`, `kcprotodb.h:1052`,
    /// `kcstashdb.h:951`), so it is never null.
    pub(crate) fn cursor(&self) -> Cursor<'_> {
        Cursor {
            cursor: sys::make_cursor(&self.db),
            db: &self.db,
            sink: Sink::Record {
                bytes: Vec::new(),
                key_len: 0,
                with_value: true,
            },
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        if self.db.is_null() {
            // `delete_unopened` destroyed the object already.
            return;
        }
        // The owner's `reset()`: every answer is ignored, as the pin
        // ignores it, and the object is destroyed either way.
        if self.sync_before_close {
            let _ = self.synchronize(false);
        }
        let _ = self.db.pin_mut().close();
        #[cfg(test)]
        let object = self.address();
        #[cfg(test)]
        record_native_call(object, NativeCall::Close);
        self.db = UniquePtr::null();
        #[cfg(test)]
        record_native_call(object, NativeCall::Delete);
    }
}

/// A cursor over one database, destroyed on drop.
///
/// `UniquePtr<DbCursor<'db>>` carries the database's lifetime (ruling point
/// 3): every cursor method dereferences its database (`kcplantdb.h:
/// 222-228`), and its destructor takes the database's lock
/// (`kchashdb.h:146-151`, `kcplantdb.h:134-140`), so the borrow checker
/// refuses a cursor that outlives the [`Db`] it came from. The cursor is
/// neither `Send` nor `Sync`.
///
/// Holds the last record it read, in a buffer it reuses from step to step.
pub(crate) struct Cursor<'db> {
    cursor: UniquePtr<sys::DbCursor<'db>>,
    db: &'db sys::BasicDB,
    sink: Sink,
}

impl Cursor<'_> {
    /// Positions at the smallest key at or after `key`; `Ok(false)` when
    /// there is none.
    pub(crate) fn jump_to(&mut self, key: &[u8]) -> Result<bool, StoreError> {
        let key = region(key)?;
        // SAFETY: `region` bounds the key by MEMMAXSIZ.
        if unsafe { sys::jump_to(self.cursor.pin_mut(), key) } {
            return Ok(true);
        }
        self.positioning_outcome("kccurjumpkey")
    }

    /// Positions at the first record — `jump()`, the no-key form.
    ///
    /// A key jump with the empty string is **not** equivalent on the hash
    /// containers: `jump("")` answers "no record" on a HashDB that has
    /// records, so an unbounded raw walk must take this form. `Ok(false)`
    /// when the database is empty.
    pub(crate) fn jump_first(&mut self) -> Result<bool, StoreError> {
        if self.cursor.pin_mut().jump() {
            return Ok(true);
        }
        self.positioning_outcome("kccurjump")
    }

    /// A cursor call returned false: `Ok(false)` when the code is "no
    /// record" — an empty database or a key past the end — otherwise the
    /// backend error the code names. A cursor's error is its database's
    /// (`kcdb.h:908-911`).
    fn positioning_outcome(&self, call: &str) -> Result<bool, StoreError> {
        if sys::error_code(self.db) == sys::ErrorCode::NOREC {
            return Ok(false);
        }
        let error = sys::last_error(self.db);
        Err(StoreError::Backend(
            format!("{call}: {}", error.name).into(),
        ))
    }

    /// Reads the record at the cursor into the cursor's buffer, then steps
    /// past it.
    ///
    /// `Ok(false)` at the end of the database. The record stays readable
    /// through [`Cursor::key`] and [`Cursor::value`] until the next read.
    pub(crate) fn next(&mut self) -> Result<bool, StoreError> {
        self.read(true, true)
    }

    /// [`Cursor::next`] without the value: [`Cursor::value`] is empty after
    /// it.
    #[cfg(test)]
    pub(crate) fn next_key(&mut self) -> Result<bool, StoreError> {
        self.read(false, true)
    }

    /// Reads the key at the cursor without moving it.
    pub(crate) fn peek_key(&mut self) -> Result<bool, StoreError> {
        self.read(false, false)
    }

    /// The key of the last record read.
    pub(crate) fn key(&self) -> &[u8] {
        let Sink::Record { bytes, key_len, .. } = &self.sink else {
            unreachable!("a cursor's sink is a record");
        };
        &bytes[..*key_len]
    }

    /// The value of the last record read.
    pub(crate) fn value(&self) -> &[u8] {
        let Sink::Record { bytes, key_len, .. } = &self.sink else {
            unreachable!("a cursor's sink is a record");
        };
        &bytes[*key_len..]
    }

    fn read(&mut self, with_value: bool, step: bool) -> Result<bool, StoreError> {
        if let Sink::Record {
            with_value: wanted, ..
        } = &mut self.sink
        {
            *wanted = with_value;
        }
        if sys::cursor_accept(self.cursor.pin_mut(), &mut self.sink, step) {
            return Ok(true);
        }
        // A false return is the end of the database as well as a failure;
        // the code separates them. The text is what `kccurget` reported.
        self.positioning_outcome("kccurget")
    }
}
