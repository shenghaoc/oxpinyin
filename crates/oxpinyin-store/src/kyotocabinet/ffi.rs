//! Safe RAII wrappers over the Kyoto Cabinet bridge.
//!
//! This is the crate's only Kyoto Cabinet FFI surface. Everything above
//! it is safe Rust.
//!
//! The bridge is a `cxx` declaration of a narrow C++ shim (`shim.h`,
//! `shim.cc`) over Kyoto Cabinet's own classes. The shim constructs the
//! concrete class libpinyin constructs for each container — `TreeDB`,
//! `HashDB`, `StashDB`, `ProtoTreeDB` (`chewing_large_table2_kyotodb.cpp`,
//! `phrase_large_table3_kyotodb.cpp`, `ngram_kyotodb.cpp` at 074a2219) —
//! with no `PolyDB` between them and no `#type=` string. Kyoto Cabinet's
//! database engine is header code, so those classes are compiled into this
//! crate from the build-time headers, exactly as they are into libpinyin;
//! the system libkyotocabinet supplies files, threads, locks and the other
//! utilities it exports.
//!
//! The bridge's declarations are `unsafe extern "C++"`, which the
//! workspace's `unsafe_code = "deny"` would otherwise reject; the
//! `expect` is scoped to this module alone, as it was for the C binding.
//!
//! # The hazards
//!
//! **Exceptions and unwinding.** No bridge function returns `Result`, so a
//! C++ exception (Kyoto Cabinet's `new` throwing `std::bad_alloc`) reaches
//! cxx's `noexcept` boundary and ends the process with `std::terminate` —
//! the end the same exception met escaping the C API, defined here rather
//! than relied on. A Rust panic in a collector aborts at the
//! `extern "Rust"` boundary. Kyoto Cabinet reports every other failure
//! through its error code, which the wrappers below read and map
//! explicitly; no error is ever classified by an exception's message.
//!
//! **`Send`/`Sync`.** [`Db`] holds a `UniquePtr` to an opaque C++ type, so
//! the compiler derives neither — the `unsafe impl`s near the bottom of
//! the module carry the same argument the C binding did, for the concrete
//! classes `PolyDB` used to wrap.
//!
//! **Buffer ownership.** Reads go through visitors: `accept` on a record,
//! `Cursor::accept` and `iterate` hand the shim's visitor the record's key
//! and value as pointers into Kyoto Cabinet's own storage, valid only for
//! the call. The visitor passes them to a Rust collector as `&[u8]`, the
//! collector copies them into a Rust-owned buffer, and the visitor returns
//! `NOP`. No record view outlives the callback, and no Kyoto Cabinet
//! allocation reaches Rust.
//!
//! **Callbacks under the record lock.** A visitor runs while Kyoto Cabinet
//! holds the record's (or, for `iterate`, the database's) lock, and must
//! not perform a database operation (`kcdb.h`, `DB::accept`, `iterate`).
//! The collectors below only copy bytes. A store visitor that may call back
//! into the database runs after the call has returned, over the copy.
//!
//! **Allocation failure** in a collector aborts the process, as `new[]`
//! failing inside libpinyin's own reads does: the copies use
//! `reserve_exact`, never a fallible reservation turned into an error.
#![expect(
    unsafe_code,
    reason = "cxx bridge to Kyoto Cabinet's C++ classes; every block carries a SAFETY comment"
)]

use std::marker::PhantomData;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

use cxx::UniquePtr;

use crate::StoreError;

#[cxx::bridge(namespace = "oxpinyin::kc")]
mod bridge {
    extern "Rust" {
        type PointRead;
        type RecordSink;
        type Records;
        type KeySink;

        fn visit_point(sink: &mut PointRead, key: &[u8], value: &[u8]);
        fn visit_record(sink: &mut RecordSink, key: &[u8], value: &[u8]);
        fn visit_records(sink: &mut Records, key: &[u8], value: &[u8]);
        fn visit_keys(sink: &mut KeySink, key: &[u8], value: &[u8]);
    }

    unsafe extern "C++" {
        include!("oxpinyin-store/src/kyotocabinet/shim.h");

        type Db;
        type Cursor;

        fn db_new(which: u8) -> UniquePtr<Db>;
        fn codename(code: i32) -> &'static [u8];
        fn version() -> &'static [u8];

        fn open(self: Pin<&mut Db>, path: &[u8], mode: u32) -> bool;
        fn tune_buckets(self: Pin<&mut Db>, bnum: i64) -> bool;
        fn ecode(self: &Db) -> i32;
        fn emsg(self: &Db) -> &[u8];
        fn accept_get(self: &Db, key: &[u8], out: &mut PointRead) -> bool;
        fn set(self: &Db, key: &[u8], value: &[u8]) -> bool;
        fn remove(self: &Db, key: &[u8]) -> bool;
        fn iterate_records(self: &Db, out: &mut Records) -> bool;
        fn iterate_keys(self: &Db, out: &mut KeySink) -> bool;
        fn count(self: &Db) -> i64;
        fn sync(self: &Db, hard: bool) -> bool;
        fn begin_transaction(self: &Db, hard: bool) -> bool;
        fn end_transaction(self: &Db, commit: bool) -> bool;
        fn load_snapshot(self: &Db, path: &[u8]) -> bool;
        fn dump_snapshot(self: &Db, path: &[u8]) -> bool;
        fn cursor(self: &Db) -> UniquePtr<Cursor>;

        fn jump(self: Pin<&mut Cursor>) -> bool;
        fn jump_key(self: Pin<&mut Cursor>, key: &[u8]) -> bool;
        fn accept_record(self: Pin<&mut Cursor>, out: &mut RecordSink, step: bool) -> bool;
        fn ecode(self: &Cursor) -> i32;
    }
}

/// The Kyoto Cabinet major.minor this backend's format survey covers.
const SUPPORTED: &str = "1.2";

/// `BasicDB::Error::NOREC`.
const NO_RECORD: i32 = 7;

/// `BasicDB::OREADER`, `OWRITER`, `OCREATE` (`kcdb.h`; `KCOREADER` and
/// the rest in `kclangc.h` carry the same values).
const OREADER: u32 = 1 << 0;
const OWRITER: u32 = 1 << 1;
const OCREATE: u32 = 1 << 2;

/// The linked library's version string (`kyotocabinet::VERSION`).
pub(crate) fn runtime_version() -> String {
    String::from_utf8_lossy(bridge::version()).into_owned()
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

/// Which Kyoto Cabinet class a database is.
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
    /// The shim's `db_new` selector.
    const fn which(self) -> u8 {
        match self {
            Self::Tree => 0,
            Self::Hash => 1,
            Self::Stash => 2,
            Self::ProtoTree => 3,
        }
    }
}

/// A path's bytes, refusing an interior NUL as the C binding did: the
/// shim hands them to Kyoto Cabinet as a `std::string`, which then opens
/// them as a C filename.
fn path_bytes(path: &Path) -> Result<&[u8], StoreError> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&0) {
        return Err(StoreError::InvalidInput("store path contains NUL"));
    }
    Ok(bytes)
}

// ── collectors ─────────────────────────────────────────────────────

/// A point read's result: the value, copied into its final allocation.
pub(crate) struct PointRead(Option<Vec<u8>>);

fn visit_point(sink: &mut PointRead, _key: &[u8], value: &[u8]) {
    let mut out = Vec::new();
    out.reserve_exact(value.len());
    out.extend_from_slice(value);
    sink.0 = Some(out);
}

/// The record at a cursor, key then value, in a buffer the cursor reuses.
pub(crate) struct RecordSink {
    bytes: Vec<u8>,
    key_len: usize,
    with_value: bool,
}

fn visit_record(sink: &mut RecordSink, key: &[u8], value: &[u8]) {
    let value = if sink.with_value { value } else { &[] };
    sink.bytes.clear();
    sink.bytes.reserve_exact(key.len() + value.len());
    sink.bytes.extend_from_slice(key);
    sink.bytes.extend_from_slice(value);
    sink.key_len = key.len();
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

/// Every record of a database, collected by `iterate`.
pub(crate) struct Records(Vec<Record>);

fn visit_records(sink: &mut Records, key: &[u8], value: &[u8]) {
    let mut bytes = Vec::new();
    bytes.reserve_exact(key.len() + value.len());
    bytes.extend_from_slice(key);
    bytes.extend_from_slice(value);
    sink.0.push(Record {
        bytes: bytes.into_boxed_slice(),
        key_len: key.len(),
    });
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

/// Every key of a database, in its walk order.
pub(crate) struct KeySink(Vec<Vec<u8>>);

fn visit_keys(sink: &mut KeySink, key: &[u8], _value: &[u8]) {
    sink.0.push(key.to_vec());
}

// ── the database ───────────────────────────────────────────────────

/// An owned Kyoto Cabinet database object.
pub(crate) struct Db {
    handle: UniquePtr<bridge::Db>,
    db_type: DbType,
    read_only: bool,
}

// SAFETY: the argument the C binding's handle carried, verified against
// the Kyoto Cabinet 1.2.80 headers for the four concrete classes this
// wraps — the classes `PolyDB` forwarded every call to:
//
// * Every record operation this backend issues — `accept`, `set`,
//   `remove` (all `accept` underneath), `iterate`, `count`,
//   `synchronize`, and the cursor calls — is documented as "performed
//   atomically and other threads accessing the same record are blocked"
//   (`iterate`: the whole iteration, other threads blocked) and
//   implemented by taking the database object's own reader-writer lock
//   (`mlock_`: `kchashdb.h`, `kcplantdb.h`, `kcstashdb.h`, `kcprotodb.h`;
//   a scoped lock at the top of every method body).
// * `begin_transaction(false)` takes the writer lock and spins while
//   another transaction is live, holding it until `end_transaction` — so
//   concurrent `write` transactions serialize inside the library.
// * The error accessors are NOT shared state: each class holds its error
//   as thread-specific data (`TSD<Error> error_`; `TSD` in `kcthread.h`
//   gives each thread its own instance), and `TreeDB` forwards to its
//   inner `HashDB`. A thread reading `error()` observes its own thread's
//   last failure; this wrapper reads it immediately after the failing call
//   on the same thread.
// * The class-level contract (`kcdb.h`, `BasicDB`'s @note) forbids two
//   database objects in one process opening the same file, and sharing a
//   database object with child processes — cross-thread sharing of one
//   object is inside the supported model. Open completes before the
//   handle exists to share, and the close and delete run in `Drop`, which
//   holds the handle exclusively.
unsafe impl Send for Db {}
// SAFETY: same argument as Send — every record operation holds the
// database's own reader-writer lock, and error accessors are thread-local.
unsafe impl Sync for Db {}

impl Db {
    /// Opens `path` as `db_type`.
    ///
    /// libpinyin's files are named `bigram.db`, `pinyin_index.bin` and so
    /// on (`src/pinyin_internal.h:56-58`): the class comes from the open,
    /// never from a suffix. The C binding had to say so with a `#type=`
    /// tuning parameter, so it refused a `#` in the path; this keeps that
    /// refusal, so a path opens here exactly when it opened there.
    pub(crate) fn open(
        path: &Path,
        db_type: DbType,
        read_only: bool,
        create: bool,
    ) -> Result<Self, StoreError> {
        Self::open_with_tuning(path, db_type, read_only, create, "")
    }

    /// Opens `path` as `db_type`, applying `extra_tuning` first.
    ///
    /// `extra_tuning` is zero or more `#bnum=<n>` parameters, the one knob
    /// the `kyotocabinet_bnum` bench drives; an empty string reproduces
    /// [`Db::open`]. Nothing in the shipping path passes a non-empty one.
    pub(crate) fn open_with_tuning(
        path: &Path,
        db_type: DbType,
        read_only: bool,
        create: bool,
        extra_tuning: &str,
    ) -> Result<Self, StoreError> {
        check_runtime_version()?;
        let bytes = path_bytes(path)?;
        if bytes.contains(&b'#') {
            return Err(StoreError::InvalidInput(
                "store path contains '#', which Kyoto Cabinet reads as the start of \
                 tuning parameters",
            ));
        }
        if !extra_tuning.is_empty() && !extra_tuning.starts_with('#') {
            return Err(StoreError::InvalidInput(
                "extra_tuning must start with '#' or be empty",
            ));
        }
        let mut this = Self {
            handle: bridge::db_new(db_type.which()),
            db_type,
            read_only,
        };
        if this.handle.is_null() {
            return Err(StoreError::Backend("db_new returned null".into()));
        }
        for parameter in extra_tuning.split('#').filter(|p| !p.is_empty()) {
            let bnum = parameter
                .strip_prefix("bnum=")
                .and_then(|n| n.parse().ok())
                .ok_or(StoreError::InvalidInput(
                    "extra_tuning supports #bnum=<n> only",
                ))?;
            if !this.handle.pin_mut().tune_buckets(bnum) {
                return Err(this.error("tune_buckets"));
            }
        }
        let mode = if read_only {
            OREADER
        } else if create {
            OWRITER | OCREATE
        } else {
            OWRITER
        };
        if !this.handle.pin_mut().open(bytes, mode) {
            // The text the C binding's `kcdbopen` reported.
            return Err(this.error("kcdbopen"));
        }
        Ok(this)
    }

    /// The library's own account of the last failure on this handle, in
    /// the C binding's words (`<call>: <code name>: <message>`).
    fn error(&self, what: &str) -> StoreError {
        let code = self.handle.ecode();
        let name = String::from_utf8_lossy(bridge::codename(code));
        let message = String::from_utf8_lossy(self.handle.emsg());
        StoreError::Backend(format!("{what}: {name}: {message}").into())
    }

    /// Whether the last operation failed because the key was absent
    /// rather than because something went wrong.
    fn last_was_no_record(&self) -> bool {
        self.handle.ecode() == NO_RECORD
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

    /// Point read. `None` when the key is absent.
    ///
    /// One read-only `accept`: the visitor copies the value straight into
    /// the returned `Vec`. libpinyin reads the same record with `check` and
    /// then `get(kbuf, ksiz, vbuf, vsiz)` into its chunk
    /// (`ngram_kyotodb.cpp:131-139` at 074a2219), two `accept`s whose
    /// results agree for a caller that does not write in between.
    pub(crate) fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let mut found = PointRead(None);
        if !self.handle.accept_get(key, &mut found) {
            return Err(self.error("kcdbget"));
        }
        Ok(found.0)
    }

    /// Every record, copied out by one read-only `iterate`, in the
    /// class's own walk order, into a vector reserved for `count`
    /// records.
    pub(crate) fn records(&self) -> Result<Vec<Record>, StoreError> {
        let mut records = Records(Vec::new());
        records
            .0
            .reserve_exact(usize::try_from(self.count()?).unwrap_or(0));
        if !self.handle.iterate_records(&mut records) {
            return Err(self.error("kcdbiterate"));
        }
        Ok(records.0)
    }

    /// Every key, in the class's walk order — `Bigram::get_all_items`'
    /// `iterate(&visitor, false)` (`ngram_kyotodb.cpp:186-196`).
    pub(crate) fn keys(&self) -> Result<Vec<Vec<u8>>, StoreError> {
        let mut keys = KeySink(Vec::new());
        if !self.handle.iterate_keys(&mut keys) {
            return Err(self.error("kcdbiterate"));
        }
        Ok(keys.0)
    }

    /// Insert or overwrite.
    pub(crate) fn set(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        if !self.handle.set(key, value) {
            return Err(self.error("kcdbset"));
        }
        Ok(())
    }

    /// Remove `key`; absent is not an error.
    pub(crate) fn remove(&self, key: &[u8]) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        if !self.handle.remove(key) && !self.last_was_no_record() {
            return Err(self.error("kcdbremove"));
        }
        Ok(())
    }

    /// Flush. `hard` also asks for the physical device.
    pub(crate) fn sync(&self, hard: bool) -> Result<(), StoreError> {
        if self.read_only {
            return Ok(());
        }
        if !self.handle.sync(hard) {
            return Err(self.error("kcdbsync"));
        }
        Ok(())
    }

    /// Reads a snapshot stream (`KCSS`) into this database — what
    /// libpinyin's `load_db` does for its in-memory containers
    /// (`ngram_kyotodb.cpp:54-64`).
    pub(crate) fn load_snapshot(&self, path: &Path) -> Result<(), StoreError> {
        if !self.handle.load_snapshot(path_bytes(path)?) {
            return Err(self.error("kcdbloadsnap"));
        }
        Ok(())
    }

    /// Writes this database's records out as a snapshot stream (`KCSS`),
    /// the format `Bigram::save_db` leaves behind (`ngram_kyotodb.cpp:82-101`).
    pub(crate) fn dump_snapshot(&self, path: &Path) -> Result<(), StoreError> {
        if !self.handle.dump_snapshot(path_bytes(path)?) {
            return Err(self.error("kcdbdumpsnap"));
        }
        Ok(())
    }

    /// Marks this handle read-only, in place, after a load-time fill.
    pub(crate) fn into_read_only(mut self) -> Self {
        self.read_only = true;
        self
    }

    /// An in-memory `StashDB`, opened as the C binding opened it (`"-"`).
    pub(crate) fn open_stash() -> Result<Self, StoreError> {
        Self::open(Path::new("-"), DbType::Stash, false, true)
    }

    /// The pin's in-memory `ProtoTreeDB` for both user indexes, opened as
    /// the C binding opened it (`"+"`).
    pub(crate) fn open_proto_tree() -> Result<Self, StoreError> {
        Self::open(Path::new("+"), DbType::ProtoTree, false, true)
    }

    /// The number of records.
    pub(crate) fn count(&self) -> Result<u64, StoreError> {
        u64::try_from(self.handle.count()).map_err(|_| self.error("kcdbcount"))
    }

    /// Begins a transaction, durable against process crash but not against
    /// power loss (`hard = false`), matching the commit's `sync(false)`.
    pub(crate) fn begin_transaction(&self) -> Result<(), StoreError> {
        if !self.handle.begin_transaction(false) {
            return Err(self.error("kcdbbegintran"));
        }
        Ok(())
    }

    /// Ends a transaction, committing or rolling back.
    pub(crate) fn end_transaction(&self, commit: bool) -> Result<(), StoreError> {
        if !self.handle.end_transaction(commit) {
            return Err(self.error("kcdbendtran"));
        }
        Ok(())
    }

    /// Opens a cursor, which borrows this database.
    pub(crate) fn cursor(&self) -> Result<Cursor<'_>, StoreError> {
        let handle = self.handle.cursor();
        if handle.is_null() {
            return Err(self.error("kcdbcursor"));
        }
        Ok(Cursor {
            handle,
            sink: RecordSink {
                bytes: Vec::new(),
                key_len: 0,
                with_value: true,
            },
            _db: PhantomData,
        })
    }
}

/// A cursor over one database, deleted on drop.
///
/// Holds the last record it read, in a buffer it reuses from step to step.
pub(crate) struct Cursor<'db> {
    handle: UniquePtr<bridge::Cursor>,
    sink: RecordSink,
    _db: PhantomData<&'db Db>,
}

impl Cursor<'_> {
    /// Positions at the smallest key at or after `key`; `Ok(false)` when
    /// there is none.
    pub(crate) fn jump_to(&mut self, key: &[u8]) -> Result<bool, StoreError> {
        if self.handle.pin_mut().jump_key(key) {
            return Ok(true);
        }
        self.positioning_outcome("kccurjumpkey")
    }

    /// Positions at the first record — the no-key jump. A key jump with the
    /// empty string is **not** equivalent on the hash containers.
    pub(crate) fn jump_first(&mut self) -> Result<bool, StoreError> {
        if self.handle.pin_mut().jump() {
            return Ok(true);
        }
        self.positioning_outcome("kccurjump")
    }

    /// A cursor call returned false: `Ok(false)` when the cursor's code is
    /// "no record" — an empty database or the end of the walk — otherwise
    /// the backend error the code names.
    fn positioning_outcome(&self, call: &str) -> Result<bool, StoreError> {
        let code = self.handle.ecode();
        if code == NO_RECORD {
            return Ok(false);
        }
        let name = String::from_utf8_lossy(bridge::codename(code));
        Err(StoreError::Backend(format!("{call}: {name}").into()))
    }

    /// Reads the record at the cursor into the cursor's buffer, then steps
    /// past it. `Ok(false)` at the end of the database.
    pub(crate) fn next(&mut self) -> Result<bool, StoreError> {
        self.read(true, true)
    }

    /// [`Cursor::next`] without the value.
    #[cfg(any(test, feature = "bench-internal"))]
    pub(crate) fn next_key(&mut self) -> Result<bool, StoreError> {
        self.read(false, true)
    }

    /// Reads the key at the cursor without moving it.
    pub(crate) fn peek_key(&mut self) -> Result<bool, StoreError> {
        self.read(false, false)
    }

    /// The key of the last record read.
    pub(crate) fn key(&self) -> &[u8] {
        &self.sink.bytes[..self.sink.key_len]
    }

    /// The value of the last record read.
    pub(crate) fn value(&self) -> &[u8] {
        &self.sink.bytes[self.sink.key_len..]
    }

    fn read(&mut self, with_value: bool, step: bool) -> Result<bool, StoreError> {
        self.sink.with_value = with_value;
        if self.handle.pin_mut().accept_record(&mut self.sink, step) {
            return Ok(true);
        }
        self.positioning_outcome("kccurget")
    }
}
