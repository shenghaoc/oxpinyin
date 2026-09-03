//! Backend-agnostic ordered key–value store for oxpinyin tables.
//!
//! This crate defines an ordered byte-KV interface split into two
//! capability tiers — [`ReadStore`] (point get, ranged scan, full scan,
//! emptiness check) and [`WriteStore`] (creation, atomic multi-table
//! writes, compaction) — and provides four peer implementations behind
//! it: [`KcStore`] on Kyoto Cabinet, [`RedbStore`] on redb, [`LmdbStore`]
//! on LMDB, and [`TkrzwStore`] on tkrzw. All four are first-class and
//! interchangeable: any oxpinyin binary picks exactly one at compile time
//! via the cargo features and calls it through the same trait surface,
//! and a table produced by any of them satisfies the same logical
//! contract as the others. Kyoto Cabinet is the default *selection* (the
//! feature enabled when no other is named), not a privileged
//! implementation. Consumers depend on the narrowest tier they need; the
//! concrete backend the current build resolves to is the [`DefaultStore`]
//! alias.
//!
//! # Key ordering
//!
//! Keys are ordered by ascending **byte** order (`memcmp` on the raw stored
//! key bytes) and nothing else — the store never decodes a key, so it has no
//! notion of integer order.  All four backends satisfy exactly this: redb's
//! `Key for &[u8]` is a byte compare; the LMDB backend sets no integer or
//! custom comparator (so LMDB's default lexicographic one applies); and the
//! tkrzw backend installs no comparator, so `TreeDBM` uses its default
//! `LexicalKeyComparator` (plain unsigned byte order); and the Kyoto Cabinet
//! backend opens `TreeDB` with no `rcomp` tuning parameter, exactly as
//! libpinyin does, so Kyoto Cabinet's default `LEXICALCOMP` applies — again
//! byte order, shorter key first on a shared prefix.  Any further backend
//! must match that default lexicographic comparator.  The encodings each
//! layer chooses on top of this rule (data little-endian = byte order,
//! intentionally not integer order; user big-endian = integer order) are
//! documented in one place in `docs/findings/store-key-ordering.md`.

// Constitution §4, mechanically: library builds may not unwrap, expect,
// or panic. Inline #[cfg(test)] modules are exempt (see the allow below
// their declaration); tests/, benches/ and examples/ are separate crates.
#![cfg_attr(not(test), deny(clippy::unwrap_used))]
#![cfg_attr(not(test), deny(clippy::expect_used))]
#![cfg_attr(not(test), deny(clippy::panic))]
#![cfg_attr(not(test), deny(clippy::panic_in_result_fn))]

// ── Exactly-one-backend invariant, enforced at compile time ────────────
//
// The four store backends (kyotocabinet, redb, lmdb, tkrzw) are peer
// implementations behind the store's trait surface, and every oxpinyin
// build has exactly one of them. Cargo features are additive under
// unification, so a plausible-looking `cargo build --features redb`
// silently combines redb with the default KC feature — precisely the
// slide these guards refuse. Every consumer crate forwards its own
// `{kyotocabinet, redb, lmdb, tkrzw}` features onto this crate, so this
// one guard suffices for the whole workspace.
//
// The zero-backend case is refused too — a build with no backend has no
// `DefaultStore` type to assemble the runtime around, and the resulting
// "unresolved type" error a downstream consumer would hit is a worse
// diagnostic than saying "select a backend" here.

#[cfg(not(any(
    feature = "kyotocabinet",
    feature = "redb",
    feature = "lmdb",
    feature = "tkrzw",
)))]
compile_error!(
    "oxpinyin-store: no store backend selected. Enable exactly one of \
     `kyotocabinet` (the default), `redb`, `lmdb`, or `tkrzw`. On the \
     command line: `cargo build` for the default (KC), or \
     `cargo build --no-default-features --features {redb|lmdb|tkrzw}` \
     for a peer."
);

#[cfg(any(
    all(feature = "kyotocabinet", feature = "redb"),
    all(feature = "kyotocabinet", feature = "lmdb"),
    all(feature = "kyotocabinet", feature = "tkrzw"),
    all(feature = "redb", feature = "lmdb"),
    all(feature = "redb", feature = "tkrzw"),
    all(feature = "lmdb", feature = "tkrzw"),
))]
compile_error!(
    "oxpinyin-store: more than one store backend selected. Exactly one \
     of `kyotocabinet`, `redb`, `lmdb`, `tkrzw` may be enabled per \
     build. A build that names an alternate peer must also disable the \
     workspace's default feature set: \
     `cargo build --no-default-features --features {redb|lmdb|tkrzw}`."
);

use std::fmt;
use std::ops::Bound;
use std::path::Path;

#[cfg(feature = "redb")]
use redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableHandle};

/// Errors that can occur when opening or scanning a store.
///
/// The variants preserve the I/O-vs-other distinction without naming any
/// backend type, so callers can branch on I/O failures independently of
/// which backend produced them.
#[derive(Debug)]
#[non_exhaustive]
pub enum StoreError {
    /// An I/O error from the underlying storage layer.
    Io(std::io::Error),
    /// A non-I/O error from the storage backend (type-erased).
    Backend(Box<dyn std::error::Error + Send + Sync>),
    /// A mutation was requested through a read-only store.
    ReadOnly,
    /// A public input cannot be represented by the selected backend.
    InvalidInput(&'static str),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "store I/O error: {e}"),
            Self::Backend(e) => write!(f, "store error: {e}"),
            Self::ReadOnly => f.write_str("store is read-only"),
            Self::InvalidInput(message) => write!(f, "invalid store input: {message}"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Backend(e) => Some(e.as_ref()),
            Self::ReadOnly | Self::InvalidInput(_) => None,
        }
    }
}

/// Row visitor passed to store scan methods.
pub type Visitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), StoreError> + 'a;

// ── traits ─────────────────────────────────────────────────────────

/// An atomic write transaction over one or more tables.
///
/// Operations see their own writes (read-your-writes within the closure).
/// All modifications land atomically on commit or are fully rolled back.
/// Used as `&mut dyn WriteTxn`.
///
/// Opening a table inside a write transaction creates it when absent, so
/// read-side methods ([`WriteTxn::get`], [`WriteTxn::range`],
/// [`WriteTxn::for_each`], [`WriteTxn::is_empty`]) never observe a
/// missing-table condition: an untouched table reads as empty.
pub trait WriteTxn {
    /// Read a single key from `table`.  Returns `None` if absent.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend read fails.
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Insert or overwrite `key` → `value` in `table`.
    ///
    /// Backends may bound key length by their storage format; the LMDB
    /// backend rejects keys outside 1..=511 bytes with
    /// [`StoreError::InvalidInput`], while the redb backend has no such
    /// limit.
    ///
    /// Backends may also bound the number of distinct tables: the LMDB
    /// backend caps a store at 32 named tables and rejects writes to a 33rd
    /// with [`StoreError::InvalidInput`], while the redb backend has no such
    /// limit.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend write fails.
    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError>;

    /// Insert or overwrite a raw (unframed) `key` → `value` — the write
    /// half of [`RawReadStore::get_raw`]. Rows written here are what
    /// `get_raw` / [`RawReadStore::range_raw`] read back on the same
    /// backend, with no table-name framing.
    ///
    /// Backends without a flat keyspace (redb, LMDB) delegate to the
    /// well-known [`RAW_TABLE`], the same delegation the raw reads use,
    /// so the round trip holds on every backend. KC and Tkrzw override
    /// this to write the file's bare keyspace — what libpinyin's own
    /// DBMs store and what datagen's libpinyin-format writers emit.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend write fails.
    fn put_raw(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.put(RAW_TABLE, key, value)
    }

    /// Remove `key` from `table` (no-op if absent).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend write fails.
    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError>;

    /// Visit rows of `table` whose keys fall in the `[lo, hi]` range,
    /// ascending key-byte order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend scan fails.
    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError>;

    /// Visit every row of `table` in ascending key-byte order.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend scan fails.
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError>;

    /// Whether `table` has no rows (an absent table counts as empty).
    ///
    /// Implementations must stop at the first row instead of scanning
    /// the whole table.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend read fails.
    fn is_empty(&self, table: &str) -> Result<bool, StoreError>;
}

/// The read tier of an ordered byte-KV store.
///
/// This is the base capability every backend provides: open an existing
/// file read-only, point get, ranged scan, full scan, and an emptiness
/// check.  A backend that can only read — no writer at all —
/// implements this and nothing else, and the system-table loader binds
/// to exactly this tier.
pub trait ReadStore {
    /// Open an existing store file in read-only mode.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be opened read-only.
    fn open_read_only(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Read a single key from `table`.  Returns `None` if absent or the
    /// table does not exist.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend read fails.
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Visit rows of `table` whose keys fall in the `[lo, hi]` range,
    /// ascending.  An absent table is treated as empty.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend scan fails.
    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError>;

    /// Visit every `(key, value)` of `table` in ascending key-byte order,
    /// borrowing each row (no per-row clone).  Stops early on visitor
    /// `Err`.  An absent table is treated as empty.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend scan fails.
    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError>;

    /// Whether `table` has no rows (an absent table counts as empty).
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend read fails.
    fn is_empty(&self, table: &str) -> Result<bool, StoreError>;
}

/// The write tier: creation, atomic multi-table writes, and compaction.
///
/// Adds mutation on top of [`ReadStore`], whose reads a writable backend
/// necessarily also provides.
pub trait WriteStore: ReadStore {
    /// Open or create a store file in read-write mode.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be created.
    fn create(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized;

    /// Open or create a **hash** store file in read-write mode — the
    /// write half of [`RawReadStore::open_hash_read_only`].
    ///
    /// libpinyin's `bigram.db` is a KC **HashDB** / Tkrzw **HashDBM**
    /// while its other DBMs are tree containers; datagen writes the
    /// bigram through this constructor so the reader's hash open finds a
    /// hash file. The default implementation delegates to
    /// [`WriteStore::create`] (correct for redb and LMDB, which have no
    /// hash/tree distinction). KC and Tkrzw override this to select the
    /// hash container class.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the store cannot be created.
    fn create_hash(path: &Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::create(path)
    }

    /// Run `f` inside an atomic write transaction.  All puts/removes in
    /// `f` land together on `Ok`, or none land on `Err` (full rollback).
    /// The closure sees its own writes.
    ///
    /// The closure must not call [`WriteStore::write`] again. Backends may
    /// serialize write transactions, so a nested call can block forever.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the transaction or its callback fails.
    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError>;

    /// Perform backend-dependent compaction work.
    ///
    /// redb rewrites the file and reclaims free pages. LMDB reuses freed pages
    /// in place, so its successful implementation does not shrink the file.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when compaction fails.
    fn compact(&mut self) -> Result<(), StoreError>;
}

// ── Raw (unframed) read tier ──────────────────────────────────────
//
// libpinyin's DBM files (pinyin_index.bin, phrase_index.bin, bigram.db,
// punct.bin) use raw byte keys — no table-name framing. This trait
// exposes the same `open_read_only` handle with key lookups that bypass
// the `table || 0x00 || key` framing the multi-table [`ReadStore`]
// uses.
//
// Needed so oxpinyin can directly consume a libpinyin-generated data
// directory without importing or converting its files.

/// Read-only access to a store file with raw (unframed) keys.
///
/// Extends [`ReadStore`] with methods that bypass table-name framing,
/// matching libpinyin's single-keyspace DBM layout. KC and Tkrzw
/// backends implement this by calling the underlying library with the
/// caller's key verbatim; redb and LMDB delegate to a well-known table
/// name since those backends do not have a flat-keyspace concept.
pub trait RawReadStore: ReadStore {
    /// Read a single raw key. Returns `None` if absent.
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError>;

    /// Visit raw (unframed) rows whose keys fall in `[lo, hi]`, ascending
    /// key-byte order — the ordered-walk half of the raw keyspace that
    /// [`Self::get_raw`] point-reads. libpinyin's phrase table relies on
    /// exactly this for its `search_suggestion` continuation walk
    /// (`phrase_large_table3_tkrzwdb.cpp:155-190`).
    ///
    /// Backends without a flat keyspace (redb, LMDB) delegate to the
    /// well-known [`RAW_TABLE`], the same delegation [`Self::get_raw`]
    /// uses; KC and Tkrzw walk the file's real keyspace.
    fn range_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        self.range(RAW_TABLE, lo, hi, visit)
    }

    /// The number of raw (unframed) rows — the count half of the raw
    /// keyspace [`RawReadStore::range_raw`] walks. The hash containers
    /// (`bigram.db`) are unordered on some backends, so a completeness
    /// check that cannot walk them (a KC HashDB cursor has no ordered
    /// first position) compares per-key values through [`Self::get_raw`]
    /// and closes the reverse direction through this count.
    ///
    /// The default implementation counts by walking the raw keyspace;
    /// KC and Tkrzw override it with the library's own record count.
    ///
    /// # Errors
    ///
    /// Returns [`StoreError`] when the backend count fails.
    fn count_raw(&self) -> Result<u64, StoreError> {
        let mut count: u64 = 0;
        self.range_raw(Bound::Unbounded, Bound::Unbounded, &mut |_key, _value| {
            count += 1;
            Ok(())
        })?;
        Ok(count)
    }

    /// Opens a hash-DB file in read-only mode (for `bigram.db`).
    ///
    /// libpinyin's `bigram.db` uses KC **HashDB** / Tkrzw **HashDBM**,
    /// while the other DBM files use TreeDB/TreeDBM. The default
    /// implementation delegates to [`ReadStore::open_read_only`] (correct
    /// for redb and LMDB, which have no hash/tree distinction). KC and
    /// Tkrzw override this to select the hash container class.
    fn open_hash_read_only(path: &std::path::Path) -> Result<Self, StoreError>
    where
        Self: Sized,
    {
        Self::open_read_only(path)
    }
}

// ── Shared: table-name validation ──────────────────────────────────
//
// Backend-independent: every peer's frame/prefix machinery needs to
// refuse an empty or NUL-bearing table name up front.

pub(crate) fn validate_table_name(table: &str) -> Result<(), StoreError> {
    if table.is_empty() {
        return Err(StoreError::InvalidInput("empty table name"));
    }
    if table.contains('\0') {
        return Err(StoreError::InvalidInput("table name contains NUL"));
    }
    Ok(())
}

// ── redb backend ───────────────────────────────────────────────────

#[cfg(feature = "redb")]
enum RedbInner {
    ReadOnly(redb::ReadOnlyDatabase),
    ReadWrite(redb::Database),
}

/// A redb-backed store implementing both capability tiers.
#[cfg(feature = "redb")]
pub struct RedbStore {
    inner: RedbInner,
}

#[cfg(feature = "redb")]
fn table_def<'a>(
    table: &'a str,
) -> Result<redb::TableDefinition<'a, &'static [u8], &'static [u8]>, StoreError> {
    validate_table_name(table)?;
    Ok(redb::TableDefinition::new(table))
}

#[cfg(feature = "redb")]
impl RedbStore {
    fn begin_read(&self) -> Result<redb::ReadTransaction, StoreError> {
        match &self.inner {
            RedbInner::ReadOnly(db) => db.begin_read().map_err(map_transaction_error),
            RedbInner::ReadWrite(db) => db.begin_read().map_err(map_transaction_error),
        }
    }
}

// ── redb shared read helpers ───────────────────────────────────────

/// An absent table is treated as empty (`None`).
#[cfg(feature = "redb")]
fn read_get(
    txn: &redb::ReadTransaction,
    table: &str,
    key: &[u8],
) -> Result<Option<Vec<u8>>, StoreError> {
    let def = table_def(table)?;
    match txn.open_table(def) {
        Ok(tbl) => Ok(tbl
            .get(key)
            .map_err(map_storage_error)?
            .map(|g| g.value().to_vec())),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(None),
        Err(e) => Err(map_table_error(e)),
    }
}

/// An absent table is treated as empty (no visits).
#[cfg(feature = "redb")]
fn read_range(
    txn: &redb::ReadTransaction,
    table: &str,
    lo: Bound<&[u8]>,
    hi: Bound<&[u8]>,
    visit: &mut Visitor<'_>,
) -> Result<(), StoreError> {
    let def = table_def(table)?;
    match txn.open_table(def) {
        Ok(tbl) => {
            for item in tbl.range::<&[u8]>((lo, hi)).map_err(map_storage_error)? {
                let (key, value) = item.map_err(map_storage_error)?;
                visit(key.value(), value.value())?;
            }
            Ok(())
        }
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(()),
        Err(e) => Err(map_table_error(e)),
    }
}

/// An absent table is treated as empty (no visits).
#[cfg(feature = "redb")]
fn read_for_each(
    txn: &redb::ReadTransaction,
    table: &str,
    visit: &mut Visitor<'_>,
) -> Result<(), StoreError> {
    let def = table_def(table)?;
    match txn.open_table(def) {
        Ok(tbl) => {
            for item in tbl.iter().map_err(map_storage_error)? {
                let (key, value) = item.map_err(map_storage_error)?;
                visit(key.value(), value.value())?;
            }
            Ok(())
        }
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(()),
        Err(e) => Err(map_table_error(e)),
    }
}

/// An absent table counts as empty.
#[cfg(feature = "redb")]
fn read_is_empty(txn: &redb::ReadTransaction, table: &str) -> Result<bool, StoreError> {
    let def = table_def(table)?;
    match txn.open_table(def) {
        Ok(tbl) => tbl.is_empty().map_err(map_storage_error),
        Err(redb::TableError::TableDoesNotExist(_)) => Ok(true),
        Err(e) => Err(map_table_error(e)),
    }
}

#[cfg(feature = "redb")]
impl ReadStore for RedbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let db = redb::Builder::new()
            .open_read_only(path)
            .map_err(map_database_error)?;
        Ok(Self {
            inner: RedbInner::ReadOnly(db),
        })
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let txn = self.begin_read()?;
        read_get(&txn, table, key)
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        let txn = self.begin_read()?;
        read_for_each(&txn, table, visit)
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let txn = self.begin_read()?;
        read_range(&txn, table, lo, hi, visit)
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        let txn = self.begin_read()?;
        read_is_empty(&txn, table)
    }
}

/// The well-known table name raw reads use on table-oriented backends.
///
/// redb and LMDB have no flat keyspace: [`RawReadStore::get_raw`]
/// delegates to this table name so test fixtures written through
/// `WriteStore::write(|txn| txn.put(RAW_TABLE, key, value))` are
/// readable by the raw path.
pub const RAW_TABLE: &str = "data";

#[cfg(feature = "redb")]
impl RawReadStore for RedbStore {
    fn get_raw(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let txn = self.begin_read()?;
        read_get(&txn, RAW_TABLE, key)
    }
}

#[cfg(feature = "redb")]
impl WriteStore for RedbStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        let db = redb::Database::create(path).map_err(map_database_error)?;
        Ok(Self {
            inner: RedbInner::ReadWrite(db),
        })
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let txn = match &self.inner {
            RedbInner::ReadWrite(db) => db.begin_write().map_err(map_transaction_error)?,
            RedbInner::ReadOnly(_) => return Err(StoreError::ReadOnly),
        };
        let result = {
            let mut wtxn = RedbWriteTxn { txn: &txn };
            f(&mut wtxn)
        };
        match result {
            Ok(result) => {
                txn.commit().map_err(map_commit_error)?;
                Ok(result)
            }
            Err(error) => {
                let _ = txn.abort();
                Err(error)
            }
        }
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        match &mut self.inner {
            RedbInner::ReadWrite(db) => {
                let _ = db.compact().map_err(map_compaction_error)?;
                Ok(())
            }
            RedbInner::ReadOnly(_) => Err(StoreError::ReadOnly),
        }
    }
}

// ── redb write-transaction wrapper ─────────────────────────────────

#[cfg(feature = "redb")]
struct RedbWriteTxn<'txn> {
    txn: &'txn redb::WriteTransaction,
}

#[cfg(feature = "redb")]
impl WriteTxn for RedbWriteTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        let def = table_def(table)?;
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        Ok(tbl
            .get(key)
            .map_err(map_storage_error)?
            .map(|g| g.value().to_vec()))
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        let def = table_def(table)?;
        let mut tbl = self.txn.open_table(def).map_err(map_table_error)?;
        tbl.insert(key, value).map_err(map_storage_error)?;
        Ok(())
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        let def = table_def(table)?;
        let mut tbl = self.txn.open_table(def).map_err(map_table_error)?;
        tbl.remove(key).map_err(map_storage_error)?;
        Ok(())
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        let def = table_def(table)?;
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        for item in tbl.range::<&[u8]>((lo, hi)).map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        // Write-transaction table opens create the table when absent, so
        // `TableDoesNotExist` cannot occur here (see the trait docs).
        let def = table_def(table)?;
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        for item in tbl.iter().map_err(map_storage_error)? {
            let (key, value) = item.map_err(map_storage_error)?;
            visit(key.value(), value.value())?;
        }
        Ok(())
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        // `open_table` creates an absent table; probe existence first so an
        // emptiness check never changes the schema.
        let exists = self
            .txn
            .list_tables()
            .map_err(map_storage_error)?
            .any(|handle| handle.name() == table);
        let def = table_def(table)?;
        if !exists {
            return Ok(true);
        }
        let tbl = self.txn.open_table(def).map_err(map_table_error)?;
        tbl.is_empty().map_err(map_storage_error)
    }
}

// ── The default backend: compile-time selection ───────────────────────
//
// One backend per oxpinyin binary. The four backend implementations
// (Kyoto Cabinet, redb, LMDB, tkrzw) are peers behind the store's trait
// interface, so `DefaultStore` resolves to a single concrete type at
// compile time and everything above it is already generic over
// `ReadStore` / `WriteStore`. The cfg chain below is exactly that
// selection: it picks the enabled backend feature; a multi-feature build
// resolves deterministically along the chain order (kyotocabinet > tkrzw
// > lmdb > redb). The chain order is a tie-break for the additive
// unification, not a hierarchy — Kyoto Cabinet is only the enabled
// feature that the workspace's default set carries, and any single
// `--features <backend>` on `--no-default-features` selects that
// backend's peer implementation instead.

// Each `cfg(feature = ...)` block below is exclusive — the exactly-one-
// backend guards at the top of this file refuse builds where more than
// one feature is enabled — so a build compiles at most one `DefaultStore`
// alias and one `DEFAULT_STORE_EXT` constant.

/// The default store backend — Kyoto Cabinet, the feature enabled in the
/// workspace's default set.
#[cfg(feature = "kyotocabinet")]
pub type DefaultStore = KcStore;

/// The default store backend — tkrzw, on
/// `--no-default-features --features tkrzw`.
#[cfg(feature = "tkrzw")]
pub type DefaultStore = TkrzwStore;

/// The default store backend — LMDB, on
/// `--no-default-features --features lmdb`.
#[cfg(feature = "lmdb")]
pub type DefaultStore = LmdbStore;

/// The default store backend — redb, on
/// `--no-default-features --features redb`.
#[cfg(feature = "redb")]
pub type DefaultStore = RedbStore;

/// File extension for [`DefaultStore`]'s native tables — one per peer;
/// the store forces its database type through open parameters, so the
/// extension is naming, not detection.
#[cfg(feature = "kyotocabinet")]
pub const DEFAULT_STORE_EXT: &str = "kct";
/// File extension for [`DefaultStore`]'s native tables (tkrzw TreeDBM).
#[cfg(feature = "tkrzw")]
pub const DEFAULT_STORE_EXT: &str = "tkt";
/// File extension for [`DefaultStore`]'s native tables (LMDB).
#[cfg(feature = "lmdb")]
pub const DEFAULT_STORE_EXT: &str = "lmdb";
/// File extension for [`DefaultStore`]'s native tables (redb).
#[cfg(feature = "redb")]
pub const DEFAULT_STORE_EXT: &str = "redb";

/// `<stem>.<DEFAULT_STORE_EXT>` — the on-disk name of a native table for
/// the compiled-in backend.
#[must_use]
pub fn default_store_file(stem: &str) -> String {
    format!("{stem}.{DEFAULT_STORE_EXT}")
}

/// Whether [`DefaultStore`] is one of the two DBM libraries libpinyin
/// itself builds against (`--with-dbm=KyotoCabinet` / `--with-dbm=Tkrzw`).
///
/// For these two, a libpinyin install's data directory *is* this
/// backend's file set — same container library, same records, same
/// file names (`pinyin_index.bin`, `bigram.db`, …) — so the runtime opens
/// it unchanged, and `oxpinyin-datagen` writes the same names. redb and
/// LMDB hold the same records in their own containers under their own
/// extensions; no libpinyin build can open those, and none needs to.
#[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
pub const DEFAULT_STORE_IS_LIBPINYIN_DBM: bool = true;
/// See the Kyoto Cabinet / tkrzw definition: redb and LMDB are
/// oxpinyin-only containers.
#[cfg(any(feature = "lmdb", feature = "redb"))]
pub const DEFAULT_STORE_IS_LIBPINYIN_DBM: bool = false;

#[cfg(feature = "lmdb")]
mod lmdb;
#[cfg(feature = "lmdb")]
pub use lmdb::LmdbStore;

#[cfg(feature = "tkrzw")]
mod tkrzw;
#[cfg(feature = "tkrzw")]
pub use tkrzw::TkrzwStore;

#[cfg(feature = "kyotocabinet")]
pub mod kyotocabinet;
#[cfg(feature = "kyotocabinet")]
pub use kyotocabinet::KcStore;

// ── redb error mapping ─────────────────────────────────────────────

#[cfg(feature = "redb")]
fn map_database_error(e: redb::DatabaseError) -> StoreError {
    match e {
        redb::DatabaseError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

#[cfg(feature = "redb")]
fn map_transaction_error(e: redb::TransactionError) -> StoreError {
    match e {
        redb::TransactionError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

#[cfg(feature = "redb")]
fn map_table_error(e: redb::TableError) -> StoreError {
    match e {
        redb::TableError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

#[cfg(feature = "redb")]
fn map_storage_error(e: redb::StorageError) -> StoreError {
    match e {
        redb::StorageError::Io(io) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

#[cfg(feature = "redb")]
fn map_commit_error(e: redb::CommitError) -> StoreError {
    match e {
        redb::CommitError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

#[cfg(feature = "redb")]
fn map_compaction_error(e: redb::CompactionError) -> StoreError {
    match e {
        redb::CompactionError::Storage(redb::StorageError::Io(io)) => StoreError::Io(io),
        other => StoreError::Backend(Box::new(other)),
    }
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    // Each of the four peer backends can produce its own use-line tests
    // — the imports below are gated to whichever peer is compiled. Under
    // the exactly-one-backend invariant, at most one peer is enabled per
    // build, so at most one branch of each `cfg` fires.
    #[cfg(feature = "tkrzw")]
    use std::ops::Bound;

    #[cfg(feature = "lmdb")]
    use super::LmdbStore;
    #[cfg(any(feature = "lmdb", feature = "tkrzw"))]
    use super::ReadStore;
    #[cfg(any(feature = "lmdb", feature = "tkrzw"))]
    use super::StoreError;
    #[cfg(feature = "tkrzw")]
    use super::TkrzwStore;
    #[cfg(any(feature = "redb", feature = "lmdb", feature = "tkrzw"))]
    use super::WriteStore;

    /// Emits the temp-path plumbing every tier group needs.
    ///
    /// Invoked from inside each group's generated module, so the three
    /// groups do not have to triplicate it.
    macro_rules! store_test_paths {
        ($mod:ident, $ext:literal) => {
            /// Owns a temporary store path and removes both the data
            /// file and its `-lock` sidecar when it drops — including
            /// when a test panics, so a failed assertion leaves no
            /// state behind in `std::env::temp_dir()`. Derefs to
            /// `Path`, so `&guard` passes anywhere a `&Path` is wanted.
            struct TempPath(std::path::PathBuf);

            impl std::ops::Deref for TempPath {
                type Target = std::path::Path;
                fn deref(&self) -> &std::path::Path {
                    &self.0
                }
            }

            impl Drop for TempPath {
                fn drop(&mut self) {
                    cleanup(&self.0);
                }
            }

            fn temp_path(tag: &str) -> TempPath {
                let path = std::env::temp_dir().join(format!(
                    "oxpinyin-store-{}-{tag}-{}.{}",
                    stringify!($mod),
                    std::process::id(),
                    $ext,
                ));
                cleanup(&path);
                TempPath(path)
            }

            fn cleanup(path: &std::path::Path) {
                let _ = std::fs::remove_file(path);
                let lock = format!("{}-lock", path.display());
                let _ = std::fs::remove_file(&lock);
            }
        };
    }

    /// The read tier: everything a bare [`ReadStore`] must answer.
    ///
    /// `$store` is exercised through [`ReadStore`] alone. `$writer` is a
    /// [`WriteStore`] used only to lay down the fixture file that `$store`
    /// then opens read-only — a read-only backend supplies whichever
    /// writer produces its file format, and runs this group unchanged
    /// without gaining a write capability.
    macro_rules! store_read_tests {
        ($mod:ident, $store:ty, $writer:ty, $ext:literal) => {
            mod $mod {
                use super::super::*;

                store_test_paths!($mod, $ext);

                /// Lays down a fixture with `$writer`, then returns a
                /// read-only `$store` handle over the same file.
                fn seeded(
                    path: &std::path::Path,
                    build: impl FnOnce(&mut dyn WriteTxn) -> Result<(), StoreError>,
                ) -> $store {
                    let writer = <$writer>::create(path).unwrap();
                    writer.write(build).unwrap();
                    drop(writer);
                    <$store>::open_read_only(path).unwrap()
                }

                #[test]
                fn multi_table_get() {
                    let path = temp_path("multi-table-get");
                    let store = seeded(&path, |txn| {
                        txn.put("alpha", b"k1", b"v1")?;
                        txn.put("beta", b"k2", b"v2")?;
                        Ok(())
                    });
                    assert_eq!(store.get("alpha", b"k1").unwrap(), Some(b"v1".to_vec()));
                    assert_eq!(store.get("beta", b"k2").unwrap(), Some(b"v2".to_vec()));
                    assert_eq!(store.get("alpha", b"k2").unwrap(), None);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn range_included_included() {
                    let path = temp_path("range-ii");
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        txn.put("t", b"c", b"3")?;
                        txn.put("t", b"d", b"4")?;
                        Ok(())
                    });
                    let mut rows = Vec::new();
                    store
                        .range(
                            "t",
                            Bound::Included(b"b".as_slice()),
                            Bound::Included(b"c".as_slice()),
                            &mut |k, v| {
                                rows.push((k.to_vec(), v.to_vec()));
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(rows.len(), 2);
                    assert_eq!(rows[0].0, b"b");
                    assert_eq!(rows[0].1, b"2");
                    assert_eq!(rows[1].0, b"c");
                    assert_eq!(rows[1].1, b"3");
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn range_included_unbounded() {
                    let path = temp_path("range-iu");
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        txn.put("t", b"c", b"3")?;
                        Ok(())
                    });
                    let mut keys = Vec::new();
                    store
                        .range(
                            "t",
                            Bound::Included(b"b".as_slice()),
                            Bound::Unbounded,
                            &mut |k, _v| {
                                keys.push(k.to_vec());
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(keys, vec![b"b".to_vec(), b"c".to_vec()]);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn is_empty_lifecycle() {
                    let path = temp_path("empty");
                    let store = seeded(&path, |_| Ok(()));
                    assert!(store.is_empty("t").unwrap());
                    drop(store);

                    let store = seeded(&path, |txn| {
                        txn.put("t", b"k", b"v")?;
                        Ok(())
                    });
                    assert!(!store.is_empty("t").unwrap());
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn missing_table_scans_are_empty_and_excluded_empty_bound_is_safe() {
                    let path = temp_path("missing-scan");
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        Ok(())
                    });
                    let mut rows = Vec::new();
                    store
                        .for_each("missing", &mut |key, value| {
                            rows.push((key.to_vec(), value.to_vec()));
                            Ok(())
                        })
                        .unwrap();
                    assert!(rows.is_empty());

                    store
                        .range(
                            "t",
                            Bound::Excluded(&[]),
                            Bound::Unbounded,
                            &mut |key, _| {
                                rows.push((key.to_vec(), Vec::new()));
                                Ok(())
                            },
                        )
                        .unwrap();
                    assert_eq!(rows, vec![(b"a".to_vec(), Vec::new())]);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn empty_bounds_never_match_or_error() {
                    let path = temp_path("empty-bounds");
                    let store = seeded(&path, |txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        Ok(())
                    });

                    let mut keys = Vec::new();
                    store
                        .range("t", Bound::Included(&[]), Bound::Unbounded, &mut |k, _| {
                            keys.push(k.to_vec());
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(keys, vec![b"a".to_vec(), b"b".to_vec()]);

                    for hi in [Bound::<&[u8]>::Excluded(&[]), Bound::Included(&[])] {
                        let mut upper_empty_keys = Vec::new();
                        store
                            .range("t", Bound::Unbounded, hi, &mut |k, _| {
                                upper_empty_keys.push(k.to_vec());
                                Ok(())
                            })
                            .unwrap();
                        assert!(upper_empty_keys.is_empty());
                    }
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn empty_and_nul_table_names_are_rejected_by_reads() {
                    fn assert_invalid<T>(result: Result<T, StoreError>) {
                        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
                    }

                    let path = temp_path("invalid-table");
                    let store = seeded(&path, |_| Ok(()));
                    for table in ["", "bad\0name"] {
                        assert_invalid(store.get(table, b"key"));
                        assert_invalid(store.for_each(table, &mut |_, _| Ok(())));
                        assert_invalid(store.range(
                            table,
                            Bound::Unbounded,
                            Bound::Unbounded,
                            &mut |_, _| Ok(()),
                        ));
                        assert_invalid(store.is_empty(table));
                    }
                    drop(store);
                    cleanup(&path);
                }
            }
        };
    }

    /// The write tier: creation, atomic multi-table writes, compaction,
    /// and the read-only handle's refusal to mutate.
    macro_rules! store_write_tests {
        ($mod:ident, $store:ty, $ext:literal) => {
            mod $mod {
                use super::super::*;

                store_test_paths!($mod, $ext);

                #[test]
                fn multi_table_write() {
                    let path = temp_path("multi-table");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("alpha", b"k1", b"v1")?;
                            txn.put("beta", b"k2", b"v2")?;
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(store.get("alpha", b"k1").unwrap(), Some(b"v1".to_vec()));
                    assert_eq!(store.get("beta", b"k2").unwrap(), Some(b"v2".to_vec()));
                    assert_eq!(store.get("alpha", b"k2").unwrap(), None);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn atomic_rollback() {
                    let path = temp_path("rollback");
                    let store = <$store>::create(&path).unwrap();
                    let result: Result<(), _> = store.write(|txn| {
                        txn.put("t", b"a", b"1")?;
                        txn.put("t", b"b", b"2")?;
                        txn.put("u", b"c", b"3")?;
                        Err(StoreError::Backend("deliberate rollback".into()))
                    });
                    assert!(result.is_err());
                    assert!(matches!(
                        result,
                        Err(StoreError::Backend(ref e)) if e.to_string() == "deliberate rollback"
                    ));
                    assert_eq!(store.get("t", b"a").unwrap(), None);
                    assert_eq!(store.get("t", b"b").unwrap(), None);
                    assert_eq!(store.get("u", b"c").unwrap(), None);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn read_your_writes() {
                    let path = temp_path("ryw");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"key", b"val")?;
                            assert_eq!(txn.get("t", b"key")?, Some(b"val".to_vec()));
                            Ok(())
                        })
                        .unwrap();
                    drop(store);
                    cleanup(&path);
                }

                /// An empty value is a stored record, not a delete.
                ///
                /// libpinyin's index DBMs are full of zero-length
                /// continuation markers, so the raw writer depends on this
                /// on every backend. The values are built from `Vec::new()`
                /// on purpose: a `Vec<u8>` with no allocation hands its
                /// `as_ptr()` over as the dangling address `1`, which Kyoto
                /// Cabinet reads as its `Visitor::REMOVE` sentinel (see
                /// `kyotocabinet::ffi::c_ptr`); a `b""` literal has a real
                /// address and never trips it.
                #[test]
                fn empty_value_is_a_record() {
                    let path = temp_path("empty-value");
                    let store = <$store>::create(&path).unwrap();
                    let empty: Vec<u8> = Vec::new();
                    store
                        .write(|txn| {
                            txn.put("t", b"marker", &empty)?;
                            txn.put("t", b"full", b"v")?;
                            txn.put_raw(b"raw-marker", &empty)?;
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(store.get("t", b"marker").unwrap(), Some(Vec::new()));
                    assert_eq!(store.get_raw(b"raw-marker").unwrap(), Some(Vec::new()));
                    // Overwriting a full record with an empty value keeps
                    // the record; only `remove` deletes.
                    store
                        .write(|txn| txn.put("t", b"full", &empty))
                        .unwrap();
                    assert_eq!(store.get("t", b"full").unwrap(), Some(Vec::new()));
                    let mut rows = Vec::new();
                    store
                        .for_each("t", &mut |key, value| {
                            rows.push((key.to_vec(), value.to_vec()));
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(
                        rows,
                        vec![(b"full".to_vec(), Vec::new()), (b"marker".to_vec(), Vec::new())]
                    );
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn write_txn_is_empty() {
                    let path = temp_path("wtxn-empty");
                    let store = <$store>::create(&path).unwrap();
                    // An absent table counts as empty and must not be created
                    // by the probe.
                    assert!(store.write(|txn| txn.is_empty("t")).unwrap());
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            assert!(!txn.is_empty("t")?);
                            txn.remove("t", b"k")?;
                            assert!(txn.is_empty("t")?);
                            Ok(())
                        })
                        .unwrap();
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn remove_in_write() {
                    let path = temp_path("remove");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            txn.remove("t", b"k")?;
                            Ok(())
                        })
                        .unwrap();
                    assert_eq!(store.get("t", b"k").unwrap(), None);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn write_txn_empty_bounds_never_match_or_error() {
                    let path = temp_path("wtxn-empty-bounds");
                    let store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"a", b"1")?;
                            txn.put("t", b"b", b"2")?;
                            Ok(())
                        })
                        .unwrap();

                    let mut visited = Vec::new();
                    store
                        .write(|txn| {
                            txn.range(
                                "t",
                                Bound::Included(&[]),
                                Bound::Excluded(&[]),
                                &mut |key, _| {
                                    visited.push(key.to_vec());
                                    Ok(())
                                },
                            )
                        })
                        .unwrap();
                    assert!(
                        visited.is_empty(),
                        "empty bounds matched {visited:?}"
                    );
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn empty_and_nul_table_names_are_rejected_by_writes() {
                    fn assert_invalid<T>(result: Result<T, StoreError>) {
                        assert!(matches!(result, Err(StoreError::InvalidInput(_))));
                    }

                    let path = temp_path("invalid-table");
                    let store = <$store>::create(&path).unwrap();
                    for table in ["", "bad\0name"] {
                        assert_invalid(store.write(|txn| txn.get(table, b"key")));
                        assert_invalid(store.write(|txn| txn.put(table, b"key", b"value")));
                        assert_invalid(store.write(|txn| txn.remove(table, b"key")));
                        assert_invalid(store.write(|txn| {
                            txn.range(
                                table,
                                Bound::Unbounded,
                                Bound::Unbounded,
                                &mut |_, _| Ok(()),
                            )
                        }));
                        assert_invalid(store.write(|txn| txn.for_each(table, &mut |_, _| Ok(()))));
                        assert_invalid(store.write(|txn| txn.is_empty(table)));
                    }
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn read_only_reopen_preserves_reads_and_rejects_mutation() {
                    let path = temp_path("read-only");
                    let store = <$store>::create(&path).unwrap();
                    store.write(|txn| txn.put("t", b"key", b"value")).unwrap();
                    drop(store);

                    let mut readonly = <$store>::open_read_only(&path).unwrap();
                    assert_eq!(readonly.get("t", b"key").unwrap(), Some(b"value".to_vec()));
                    assert!(matches!(
                        readonly.write(|_| Ok(())),
                        Err(StoreError::ReadOnly)
                    ));
                    assert!(matches!(readonly.compact(), Err(StoreError::ReadOnly)));
                    drop(readonly);
                    cleanup(&path);
                }

                #[test]
                fn write_returns_closure_value() {
                    let path = temp_path("write-ret");
                    let store = <$store>::create(&path).unwrap();
                    let val: u32 = store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            Ok(42u32)
                        })
                        .unwrap();
                    assert_eq!(val, 42);
                    drop(store);
                    cleanup(&path);
                }

                #[test]
                fn compact_preserves_data() {
                    let path = temp_path("compact");
                    let mut store = <$store>::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("t", b"k", b"v")?;
                            Ok(())
                        })
                        .unwrap();
                    store.compact().unwrap();
                    assert_eq!(store.get("t", b"k").unwrap(), Some(b"v".to_vec()));
                    drop(store);
                    cleanup(&path);
                }
            }
        };
    }

    // Every backend offers both tiers, so each runs both groups and is
    // its own fixture writer. A read-only backend would invoke only
    // `store_read_tests!`. Each group is gated by the peer's feature —
    // the exactly-one-backend guards refuse combined builds, so at most
    // one of these four groups is ever compiled.
    #[cfg(feature = "redb")]
    store_read_tests!(redb_read, RedbStore, RedbStore, "redb");
    #[cfg(feature = "redb")]
    store_write_tests!(redb_write, RedbStore, "redb");

    #[cfg(feature = "lmdb")]
    store_read_tests!(lmdb_read, LmdbStore, LmdbStore, "lmdb");
    #[cfg(feature = "lmdb")]
    store_write_tests!(lmdb_write, LmdbStore, "lmdb");

    #[cfg(feature = "tkrzw")]
    store_read_tests!(tkrzw_read, TkrzwStore, TkrzwStore, "tkrzw");
    #[cfg(feature = "tkrzw")]
    store_write_tests!(tkrzw_write, TkrzwStore, "tkrzw");

    #[cfg(feature = "kyotocabinet")]
    store_read_tests!(kc_read, KcStore, KcStore, "kc");
    #[cfg(feature = "kyotocabinet")]
    store_write_tests!(kc_write, KcStore, "kc");

    /// Removes the borrowed path on drop, so a panicking test leaves no
    /// file behind in `std::env::temp_dir()`. redb keeps no `-lock`
    /// sidecar, so the single data file is all that needs removing;
    /// LMDB and tkrzw sidecars are cleaned up separately by their own
    /// tests. Used by the redb probe and the LMDB concurrency tests.
    #[cfg(any(feature = "redb", feature = "lmdb"))]
    struct RemoveOnDrop<'a>(&'a std::path::Path);

    #[cfg(any(feature = "redb", feature = "lmdb"))]
    impl Drop for RemoveOnDrop<'_> {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(self.0);
        }
    }

    // ── Default-backend policy: mechanical invariants ──────────────────
    //
    // The workspace policy: the four peer backends (KC, redb, LMDB, tkrzw)
    // are equal implementations behind the store's trait surface; KC is
    // the default *selection* (the feature enabled by the workspace's
    // default set), not a privileged one. These tests catch any accidental
    // slide back to "redb default" (or any other silent reordering) — a
    // plain string check on `DEFAULT_STORE_EXT` pinned to the feature the
    // build is running under, plus a compile-time type-identity check on
    // `DefaultStore`.

    #[test]
    fn default_store_ext_matches_the_compiled_backend() {
        // Each peer selects a distinct extension. The exactly-one-
        // backend guards refuse combined builds, so exactly one of the
        // arms below fires per build — and if none fired, this test
        // would go unfound (`#[cfg]` gates the whole `#[test]` too).
        #[cfg(feature = "kyotocabinet")]
        assert_eq!(super::DEFAULT_STORE_EXT, "kct");
        #[cfg(feature = "tkrzw")]
        assert_eq!(super::DEFAULT_STORE_EXT, "tkt");
        #[cfg(feature = "lmdb")]
        assert_eq!(super::DEFAULT_STORE_EXT, "lmdb");
        #[cfg(feature = "redb")]
        assert_eq!(super::DEFAULT_STORE_EXT, "redb");
    }

    #[test]
    fn default_store_file_composes_stem_and_extension() {
        let native = super::default_store_file("phrase_index");
        assert!(
            native.starts_with("phrase_index."),
            "the stem must be preserved verbatim: got {native:?}"
        );
        let dot = native
            .find('.')
            .expect("the composed name has an extension");
        assert_eq!(&native[dot + 1..], super::DEFAULT_STORE_EXT);
    }

    /// `DefaultStore` resolves to `KcStore` when the Kyoto Cabinet feature
    /// is enabled — the workspace's default selection. This is a
    /// compile-time type identity, so a silent flip in the cfg chain
    /// would fail to build rather than pass silently.
    #[cfg(feature = "kyotocabinet")]
    #[test]
    fn default_store_is_kc_when_kyotocabinet_is_on() {
        fn assert_type_eq<T>()
        where
            T: 'static,
            super::DefaultStore: 'static,
        {
            assert_eq!(
                std::any::TypeId::of::<super::DefaultStore>(),
                std::any::TypeId::of::<T>(),
                "DefaultStore must resolve to the expected concrete backend"
            );
        }
        assert_type_eq::<super::KcStore>();
    }

    /// `--no-default-features --features redb` resolves `DefaultStore`
    /// to `RedbStore` — the pure-Rust peer. The exactly-one-backend
    /// guards at the top of `lib.rs` refuse a build that combines
    /// `redb` with any of the C peers, so the cfg here only names the
    /// redb feature.
    #[cfg(feature = "redb")]
    #[test]
    fn default_store_is_redb_when_only_redb_is_on() {
        fn assert_type_eq<T>()
        where
            T: 'static,
            super::DefaultStore: 'static,
        {
            assert_eq!(
                std::any::TypeId::of::<super::DefaultStore>(),
                std::any::TypeId::of::<T>(),
                "DefaultStore must resolve to the expected concrete backend"
            );
        }
        assert_type_eq::<super::RedbStore>();
    }

    /// `--no-default-features --features tkrzw` resolves `DefaultStore`
    /// to `TkrzwStore` — the tkrzw peer.
    #[cfg(feature = "tkrzw")]
    #[test]
    fn default_store_is_tkrzw_when_only_tkrzw_is_on() {
        fn assert_type_eq<T>()
        where
            T: 'static,
            super::DefaultStore: 'static,
        {
            assert_eq!(
                std::any::TypeId::of::<super::DefaultStore>(),
                std::any::TypeId::of::<T>(),
                "DefaultStore must resolve to the expected concrete backend"
            );
        }
        assert_type_eq::<super::TkrzwStore>();
    }

    /// `--no-default-features --features lmdb` resolves `DefaultStore`
    /// to `LmdbStore` — the LMDB peer.
    #[cfg(feature = "lmdb")]
    #[test]
    fn default_store_is_lmdb_when_only_lmdb_is_on() {
        fn assert_type_eq<T>()
        where
            T: 'static,
            super::DefaultStore: 'static,
        {
            assert_eq!(
                std::any::TypeId::of::<super::DefaultStore>(),
                std::any::TypeId::of::<T>(),
                "DefaultStore must resolve to the expected concrete backend"
            );
        }
        assert_type_eq::<super::LmdbStore>();
    }

    #[cfg(feature = "redb")]
    #[test]
    fn redb_is_empty_probe_does_not_create_tables() {
        use ::redb::ReadableDatabase;
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-wtxn-probe-{}.redb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveOnDrop(&path);
        let store = crate::RedbStore::create(&path).unwrap();
        assert!(store.write(|txn| txn.is_empty("t")).unwrap());
        drop(store);
        // A committed write transaction that only probed emptiness must
        // leave the database with zero tables; assert it against redb
        // directly because the store traits cannot distinguish an absent
        // table from an empty one.  (`::redb` names the crate
        // unambiguously alongside the generated per-tier test modules.)
        let db = ::redb::ReadOnlyDatabase::open(&path).unwrap();
        let txn = db.begin_read().unwrap();
        assert_eq!(txn.list_tables().unwrap().count(), 0);
        drop(txn);
        drop(db);
    }

    /// Removes a tkrzw store file and its `-lock` sidecar on drop, so a
    /// panicking test leaves nothing behind in `std::env::temp_dir()`.
    #[cfg(feature = "tkrzw")]
    struct RemoveTkrzw(std::path::PathBuf);

    #[cfg(feature = "tkrzw")]
    impl Drop for RemoveTkrzw {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
            let mut lock = self.0.clone().into_os_string();
            lock.push("-lock");
            let _ = std::fs::remove_file(std::path::PathBuf::from(lock));
        }
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_orders_keys_as_unsigned_bytes() {
        // The backend installs no custom comparator, so TreeDBM sorts by
        // its default LexicalKeyComparator. That has to be plain
        // *unsigned* byte order for oxpinyin's big-endian key codec to
        // keep the order it has under redb and LMDB — a signed-char
        // comparison would sort 0x80.. before 0x00.., silently reversing
        // every high-token range scan. ASCII fixtures cannot tell the
        // two apart, so probe the high half explicitly.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-order-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();
        let keys: [&[u8]; 5] = [&[0x00], &[0x7f], &[0x80], &[0xfe], &[0xff]];
        store
            .write(|txn| {
                // Inserted out of order, so the walk order is the store's
                // and not the insertion sequence.
                for key in [keys[2], keys[4], keys[0], keys[3], keys[1]] {
                    txn.put("t", key, b"x")?;
                }
                Ok(())
            })
            .unwrap();

        let mut walked = Vec::new();
        store
            .for_each("t", &mut |key, _value| {
                walked.push(key.to_vec());
                Ok(())
            })
            .unwrap();
        assert_eq!(
            walked,
            keys.iter().map(|key| key.to_vec()).collect::<Vec<_>>(),
            "TreeDBM must walk keys in ascending unsigned byte order"
        );

        // The same order must hold through a bounded scan: 0x80..=0xfe
        // is the half a signed comparison would misplace.
        let mut ranged = Vec::new();
        store
            .range(
                "t",
                Bound::Included(&[0x80][..]),
                Bound::Included(&[0xfe][..]),
                &mut |key, _value| {
                    ranged.push(key.to_vec());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(ranged, vec![vec![0x80], vec![0xfe]]);
        drop(store);
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_keeps_tables_apart_when_one_name_prefixes_another() {
        // TreeDBM is one keyspace, so the backend frames records as
        // `table || 0x00 || key`. The framing is prefix-free only
        // because table names are validated NUL-free; pin that, since a
        // regression would silently merge `a` into `ab`'s scans.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-prefix-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("a", b"k", b"in-a")?;
                txn.put("ab", b"k", b"in-ab")?;
                txn.put("a\u{1}b", b"k", b"in-a1b")?;
                Ok(())
            })
            .unwrap();

        for (table, expected) in [("a", "in-a"), ("ab", "in-ab"), ("a\u{1}b", "in-a1b")] {
            assert_eq!(
                store.get(table, b"k").unwrap(),
                Some(expected.as_bytes().to_vec())
            );
            let mut rows = 0_usize;
            store
                .for_each(table, &mut |_key, value| {
                    assert_eq!(value, expected.as_bytes());
                    rows += 1;
                    Ok(())
                })
                .unwrap();
            assert_eq!(rows, 1, "{table} must scan only its own rows");
        }
        drop(store);
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_write_txn_scan_merges_buffer_over_stored_rows() {
        // `merged_visit` lays a transaction's buffer over stored rows and
        // streams the merge in ascending key order: a buffered value
        // overrides its stored twin, a buffered tombstone hides it, and
        // buffered inserts slot in around both. Seed three rows, then
        // buffer one of each mutation — insert before a stored key, update,
        // tombstone, insert after the last stored key — and scan inside the
        // transaction before anything commits.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-merge-scan-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();
        store
            .write(|txn| {
                txn.put("t", b"b", b"stored-b")?;
                txn.put("t", b"d", b"stored-d")?;
                txn.put("t", b"f", b"stored-f")?;
                Ok(())
            })
            .unwrap();

        store
            .write(|txn| {
                txn.put("t", b"a", b"insert-a")?; // before the first stored key
                txn.put("t", b"b", b"update-b")?; // overwrites a stored key
                txn.remove("t", b"d")?; // tombstones a stored key
                txn.put("t", b"g", b"insert-g")?; // after the last stored key

                let mut rows = Vec::new();
                txn.for_each("t", &mut |key, value| {
                    rows.push((key.to_vec(), value.to_vec()));
                    Ok(())
                })?;
                assert_eq!(
                    rows,
                    vec![
                        (b"a".to_vec(), b"insert-a".to_vec()),
                        (b"b".to_vec(), b"update-b".to_vec()),
                        (b"f".to_vec(), b"stored-f".to_vec()),
                        (b"g".to_vec(), b"insert-g".to_vec()),
                    ],
                    "merged scan must be ascending, apply buffered puts, \
                     and hide the tombstoned row"
                );
                Ok(())
            })
            .unwrap();
        drop(store);
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_scan_keeps_every_borrowed_record_intact() {
        // The zero-copy walk hands the visitor a pointer into tkrzw's
        // record buffer that is valid only for that record's callback. A
        // use-after-free of that borrow corrupts LATER records, not the
        // first — the iterator invalidates or reuses the memory a prior
        // callback read. Walk many records and verify each key and value
        // in place as it is visited, so a stale read fails the assertion
        // rather than passing silently.
        fn expected_value(i: u32) -> [u8; 32] {
            let mut value = [0u8; 32];
            for (j, byte) in value.iter_mut().enumerate() {
                *byte = i.wrapping_add(j as u32) as u8;
            }
            value
        }

        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-tkrzw-scan-many-{}.tkrzw",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let _cleanup = RemoveTkrzw(path.clone());
        let store = TkrzwStore::create(&path).unwrap();

        const RECORDS: u32 = 10_000;
        store
            .write(|txn| {
                for i in 0..RECORDS {
                    txn.put("t", &i.to_be_bytes(), &expected_value(i))?;
                }
                Ok(())
            })
            .unwrap();

        let mut seen = 0u32;
        store
            .for_each("t", &mut |key, value| {
                let i = u32::from_be_bytes(key.try_into().unwrap());
                assert_eq!(i, seen, "scan must visit keys in ascending order");
                assert_eq!(
                    value,
                    &expected_value(i),
                    "record {i} came back corrupt — borrowed buffer invalidated early"
                );
                seen += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(seen, RECORDS, "every record must be visited exactly once");
        drop(store);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_key_lengths_lmdb_cannot_store() {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-keylen-{}.mdb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let lock: std::path::PathBuf = {
            let mut l = path.clone().into_os_string();
            l.push("-lock");
            l.into()
        };
        let store = LmdbStore::create(&path).unwrap();
        assert!(matches!(
            store.write(|txn| txn.put("t", b"", b"v")),
            Err(StoreError::InvalidInput("key length must be 1..=511 bytes"))
        ));
        let long = [b'k'; 512];
        assert!(matches!(
            store.write(|txn| txn.put("t", &long, b"v")),
            Err(StoreError::InvalidInput("key length must be 1..=511 bytes"))
        ));
        let boundary = [b'k'; 511];
        store
            .write(|txn| txn.put("t", &boundary, b"v"))
            .expect("511-byte keys are accepted");
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_unaligned_map_size() {
        // Absolute: heed resolves the path before validating map-size
        // alignment, and a relative path fails with NotFound first.
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-unaligned-{}.mdb",
            std::process::id(),
        ));
        // 3 is not a multiple of any real system page size.
        assert!(matches!(
            LmdbStore::create_with_map_size(&path, 3),
            Err(StoreError::InvalidInput(
                "map size must be a multiple of the system page size"
            ))
        ));
        let _ = std::fs::remove_file(&path);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_zero_map_size() {
        let path = std::path::PathBuf::from("oxpinyin-store-zero-map.mdb");
        assert!(matches!(
            LmdbStore::create_with_map_size(&path, 0),
            Err(StoreError::InvalidInput("map size must be nonzero"))
        ));
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_nul_path() {
        let path = std::path::PathBuf::from("oxpinyin-store\0invalid.mdb");
        assert!(matches!(
            LmdbStore::create(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
        assert!(matches!(
            LmdbStore::open_read_only(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
    }

    #[cfg(feature = "tkrzw")]
    #[test]
    fn tkrzw_rejects_nul_path() {
        let path = std::path::PathBuf::from("oxpinyin-store\0invalid.tkrzw");
        assert!(matches!(
            TkrzwStore::create(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
        assert!(matches!(
            TkrzwStore::open_read_only(&path),
            Err(StoreError::InvalidInput("path contains NUL"))
        ));
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_more_than_max_named_tables() {
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-maxdbs-{}.mdb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let lock: std::path::PathBuf = {
            let mut l = path.clone().into_os_string();
            l.push("-lock");
            l.into()
        };
        let store = LmdbStore::create(&path).unwrap();
        // LMDB caps the environment at 32 named tables; writing past that
        // must surface as InvalidInput, not an opaque backend error. The
        // loop runs well beyond 32 so the exact off-by-one does not matter.
        let result = store.write(|txn| {
            for i in 0..40 {
                txn.put(&format!("t{i}"), b"k", b"v")?;
            }
            Ok(())
        });
        assert!(matches!(
            result,
            Err(StoreError::InvalidInput(
                "too many distinct tables (LMDB caps a store at 32)"
            ))
        ));
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_open_read_only_with_map_size_roundtrips() {
        // A store created with a non-default ceiling reopens read-only with
        // the same ceiling — the path a store grown past the 1 GiB default
        // needs, since the trait `open_read_only` hardcodes that default.
        // (Address space is committed sparsely, so the 2 GiB ceiling costs
        // nothing on disk for one tiny record.)
        const BIG_MAP: usize = 2 << 30; // 2 GiB, a multiple of the page size
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-romap-{}.mdb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let lock: std::path::PathBuf = {
            let mut l = path.clone().into_os_string();
            l.push("-lock");
            l.into()
        };
        let store = LmdbStore::create_with_map_size(&path, BIG_MAP).unwrap();
        store.write(|txn| txn.put("t", b"k", b"v")).unwrap();
        drop(store);
        let readonly = LmdbStore::open_read_only_with_map_size(&path, BIG_MAP).unwrap();
        assert_eq!(readonly.get("t", b"k").unwrap(), Some(b"v".to_vec()));
        assert!(matches!(
            readonly.write(|txn| txn.put("t", b"k2", b"v2")),
            Err(StoreError::ReadOnly)
        ));
        drop(readonly);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_concurrent_first_open_of_shared_table_survives() {
        // The whole reason for the DBI-cache: N threads opening the
        // same env for the first time and hitting the same fresh
        // `mdb_dbi_open` window used to race on `env->me_dbxs` writes
        // and trip glibc's tcache check. With the cache in place the
        // first thread to reach the miss path opens+commits the DBI
        // env-wide, and every other thread finds it on the fast path
        // — no double-alloc, no double-free, no crash.
        //
        // Split into two phases so both the "existing DBI, concurrent
        // reads" and the "first-time creation" contracts are exercised:
        //
        // 1. Seed a table via one writer, then hammer it from N reader
        //    threads on N independent stores of the same path — the
        //    DBI already lives in `me_dbxs` from the seed's commit, so
        //    every reader's fast-path lookup should return the same
        //    cached handle and the reads should run concurrently.
        // 2. Hammer a *fresh* env from N writers all creating the same
        //    not-yet-existing table concurrently — the write-side
        //    serializes on heed's one-writer-per-env, but the point
        //    is that no read-side probe crossing that write's
        //    `mdb_txn_end` misreads `me_dbxs`.
        const THREADS: usize = 8;
        const OPS_PER_THREAD: usize = 32;

        // Cleanup on drop — a spawned-thread panic (any assertion
        // inside the closures) unwinds through `h.join().unwrap()`,
        // and the trailing manual removes would never run.
        fn lock_sidecar(path: &std::path::Path) -> std::path::PathBuf {
            let mut lock = path.to_path_buf().into_os_string();
            lock.push("-lock");
            lock.into()
        }

        // Phase 1: existing table, concurrent readers.
        let read_path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-concurrent-read-{}.mdb",
            std::process::id(),
        ));
        let read_lock = lock_sidecar(&read_path);
        let _ = std::fs::remove_file(&read_path);
        let _ = std::fs::remove_file(&read_lock);
        let _read_cleanup = RemoveOnDrop(&read_path);
        let _read_lock_cleanup = RemoveOnDrop(&read_lock);
        let writer = LmdbStore::create(&read_path).unwrap();
        writer
            .write(|txn| {
                txn.put("shared", b"k", b"v")?;
                Ok(())
            })
            .unwrap();
        drop(writer);

        let path_arc = std::sync::Arc::new(read_path.clone());
        let handles: Vec<_> = (0..THREADS)
            .map(|_| {
                let path = std::sync::Arc::clone(&path_arc);
                std::thread::spawn(move || {
                    let store = LmdbStore::open_read_only(&path).unwrap();
                    for _ in 0..OPS_PER_THREAD {
                        assert_eq!(store.get("shared", b"k").unwrap(), Some(b"v".to_vec()));
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }

        // Phase 2: previously-nonexistent table, concurrent first-time
        // creation from N writers. LMDB serializes write txns on one
        // env internally; what we're proving is that the cache never
        // hands out a DBI a write ended up aborting, and that the
        // first-time creates all wind up naming the same slot after
        // they've each committed (`me_dbxs["fresh"]` is single-valued).
        let write_path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-concurrent-write-{}.mdb",
            std::process::id(),
        ));
        let write_lock = lock_sidecar(&write_path);
        let _ = std::fs::remove_file(&write_path);
        let _ = std::fs::remove_file(&write_lock);
        let _write_cleanup = RemoveOnDrop(&write_path);
        let _write_lock_cleanup = RemoveOnDrop(&write_lock);

        let path_arc = std::sync::Arc::new(write_path.clone());
        let handles: Vec<_> = (0..THREADS)
            .map(|tid| {
                let path = std::sync::Arc::clone(&path_arc);
                std::thread::spawn(move || {
                    let store = LmdbStore::create(&path).unwrap();
                    store
                        .write(|txn| {
                            txn.put("fresh", &tid.to_be_bytes(), b"v")?;
                            Ok(())
                        })
                        .unwrap();
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        // Every writer's key should be visible after the phase — the
        // table exists exactly once and all N writes reached it.
        let reader = LmdbStore::open_read_only(&write_path).unwrap();
        for tid in 0..THREADS {
            assert_eq!(
                reader.get("fresh", &tid.to_be_bytes()).unwrap(),
                Some(b"v".to_vec()),
                "writer {tid}'s row did not land",
            );
        }
        drop(reader);
    }

    #[cfg(feature = "lmdb")]
    #[test]
    fn lmdb_rejects_a_conflicting_map_size_while_the_env_is_live() {
        // One env is shared per path, and heed can neither reopen a live
        // environment at a different ceiling nor resize it — so a mismatching
        // request must fail up front with InvalidInput instead of silently
        // handing back the live env's ceiling (writes would then hit
        // MDB_MAP_FULL at runtime against a ceiling the caller never chose).
        // With the mismatching handle gone, the new ceiling applies.
        const BIG_MAP: usize = 2 << 30; // 2 GiB, a multiple of the page size
        const OTHER_MAP: usize = 4 << 30; // 4 GiB, likewise
        let path = std::env::temp_dir().join(format!(
            "oxpinyin-store-lmdb-mapconflict-{}.mdb",
            std::process::id(),
        ));
        let _ = std::fs::remove_file(&path);
        let lock: std::path::PathBuf = {
            let mut l = path.clone().into_os_string();
            l.push("-lock");
            l.into()
        };
        let store = LmdbStore::create_with_map_size(&path, BIG_MAP).unwrap();
        assert!(matches!(
            LmdbStore::create_with_map_size(&path, OTHER_MAP),
            Err(StoreError::InvalidInput(
                "this LMDB file is already open in this process with a different map size; \
                 close those handles before opening it with this ceiling"
            ))
        ));
        // A matching ceiling still shares the live env.
        assert!(LmdbStore::open_read_only_with_map_size(&path, BIG_MAP).is_ok());
        drop(store);
        let reopened = LmdbStore::create_with_map_size(&path, OTHER_MAP).unwrap();
        drop(reopened);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }

    // ── per-peer key-ordering conformance ─────────────────────────
    //
    // The byte-order contract must hold *identically* across every
    // peer backend, and — the property a small-id fixture cannot see —
    // byte order must diverge from integer order across the 256
    // boundary. Under the exactly-one-backend invariant the tests
    // cannot cross-compare two peers in one process, so each build
    // runs them against its own `DefaultStore`. Running all four peer
    // builds (KC / redb / LMDB / Tkrzw) through CI gives the same
    // coverage the earlier in-process three-way check gave: each peer
    // independently satisfies the byte-order contract, and the
    // expected walk order is computed mathematically (sort the keys)
    // rather than borrowed from a reference peer's output.
    mod key_ordering {
        use std::ops::Bound;
        use std::path::Path;

        use super::super::{DefaultStore, WriteStore};

        /// Tokens that cross the 256 boundary in the low byte and in
        /// higher bytes, so their little-endian byte order differs from
        /// their integer order. Deliberately unsorted: the store imposes
        /// the walk order, not the insertion order.
        const BOUNDARY_TOKENS: &[u32] = &[
            0x0000_0200,
            0x0000_00FF,
            0x0700_0100,
            0x0000_0100,
            0x0000_0001,
            0x0000_FFFF,
            0x0000_01FF,
            0x0700_0001,
            0x0001_0000,
            0x0700_00FF,
            0x00FF_0000,
            0x0100_0000,
        ];

        type Rows = Vec<(Vec<u8>, Vec<u8>)>;

        /// Owns a temp store path; removes the data file and any `-lock`
        /// sidecar (redb keeps none; LMDB and tkrzw do) on drop, so a
        /// panicking test leaves nothing behind.
        struct TempPath(std::path::PathBuf);

        impl TempPath {
            fn new(tag: &str) -> Self {
                let path = std::env::temp_dir().join(format!(
                    "oxpinyin-store-keyorder-{tag}-{}.db",
                    std::process::id(),
                ));
                let this = Self(path);
                this.cleanup();
                this
            }

            fn cleanup(&self) {
                let _ = std::fs::remove_file(&self.0);
                let mut lock = self.0.clone().into_os_string();
                lock.push("-lock");
                let _ = std::fs::remove_file(std::path::PathBuf::from(lock));
            }
        }

        impl Drop for TempPath {
            fn drop(&mut self) {
                self.cleanup();
            }
        }

        /// Insert `keys` (each mapped to a 1-byte value) and return the
        /// `for_each` walk as owned rows.
        fn insert_and_walk<S: WriteStore>(path: &Path, keys: &[Vec<u8>]) -> Rows {
            let store = S::create(path).expect("create store");
            store
                .write(|txn| {
                    for key in keys {
                        txn.put("data", key, b"v")?;
                    }
                    Ok(())
                })
                .expect("bulk insert");
            let mut rows = Rows::new();
            store
                .for_each("data", &mut |key, value| {
                    rows.push((key.to_vec(), value.to_vec()));
                    Ok(())
                })
                .expect("for_each walk");
            drop(store);
            rows
        }

        /// Insert `keys` and return the `range` walk over `[lo, hi)`.
        fn insert_and_range<S: WriteStore>(
            path: &Path,
            keys: &[Vec<u8>],
            lo: &[u8],
            hi: &[u8],
        ) -> Rows {
            let store = S::create(path).expect("create store");
            store
                .write(|txn| {
                    for key in keys {
                        txn.put("data", key, b"v")?;
                    }
                    Ok(())
                })
                .expect("bulk insert");
            let mut rows = Rows::new();
            store
                .range(
                    "data",
                    Bound::Included(lo),
                    Bound::Excluded(hi),
                    &mut |key, value| {
                        rows.push((key.to_vec(), value.to_vec()));
                        Ok(())
                    },
                )
                .expect("range walk");
            drop(store);
            rows
        }

        fn le_keys() -> Vec<Vec<u8>> {
            BOUNDARY_TOKENS
                .iter()
                .map(|t| t.to_le_bytes().to_vec())
                .collect()
        }

        fn be_keys() -> Vec<Vec<u8>> {
            BOUNDARY_TOKENS
                .iter()
                .map(|t| t.to_be_bytes().to_vec())
                .collect()
        }

        // ── the store contract: byte order, not integer order ──────

        /// Ascending sort of `keys` by raw byte order — the store's one
        /// walk-order rule (`docs/findings/store-key-ordering.md`).
        fn sorted_by_bytes(keys: &[Vec<u8>]) -> Vec<Vec<u8>> {
            let mut sorted: Vec<Vec<u8>> = keys.to_vec();
            sorted.sort();
            sorted
        }

        #[test]
        fn walks_raw_keys_in_byte_order_and_that_is_not_integer_order() {
            let path = TempPath::new("byteorder");
            let rows = insert_and_walk::<DefaultStore>(&path.0, &le_keys());

            let keys: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
            assert_eq!(
                keys,
                sorted_by_bytes(&le_keys()),
                "store walk must be strictly ascending by raw byte order",
            );

            // Non-vacuity: the SAME walk, decoded to integers, is NOT
            // ascending — byte order and integer order genuinely differ
            // on this 256-crossing set.
            let tokens: Vec<u32> = rows
                .iter()
                .map(|(k, _)| u32::from_le_bytes([k[0], k[1], k[2], k[3]]))
                .collect();
            assert!(
                !tokens.is_sorted(),
                "the 256-boundary set must make byte order differ from integer order",
            );
        }

        #[test]
        fn range_walks_byte_order_across_256() {
            let path = TempPath::new("range");
            let keys = le_keys();
            // A window straddling 256 in byte order: LE(0x0000_0100)
            // (`00 01 00 00`) up to LE(0x0000_00FF) (`FF 00 00 00`).
            let lo = 0x0000_0100_u32.to_le_bytes();
            let hi = 0x0000_00FF_u32.to_le_bytes();
            let rows = insert_and_range::<DefaultStore>(&path.0, &keys, &lo, &hi);
            assert!(!rows.is_empty(), "the range must match rows");
            let out: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
            let expected: Vec<Vec<u8>> = sorted_by_bytes(&keys)
                .into_iter()
                .filter(|k| k.as_slice() >= &lo[..] && k.as_slice() < &hi[..])
                .collect();
            assert_eq!(
                out, expected,
                "range walk must return the ascending byte-ordered slice \
                 inside its bounds",
            );
        }

        #[test]
        fn perturbing_encode_endianness_changes_the_store_walk_order() {
            // Same logical tokens, two encode conventions. Non-vacuity in
            // pure form: had an encode site used the opposite endianness,
            // the store would hand back a different order, so any test
            // asserting a specific order would go red.
            let path_le = TempPath::new("perturb-le");
            let path_be = TempPath::new("perturb-be");
            let le = insert_and_walk::<DefaultStore>(&path_le.0, &le_keys());
            let be = insert_and_walk::<DefaultStore>(&path_be.0, &be_keys());

            let le_tokens: Vec<u32> = le
                .iter()
                .map(|(k, _)| u32::from_le_bytes([k[0], k[1], k[2], k[3]]))
                .collect();
            let be_tokens: Vec<u32> = be
                .iter()
                .map(|(k, _)| u32::from_be_bytes([k[0], k[1], k[2], k[3]]))
                .collect();

            assert!(
                be_tokens.is_sorted(),
                "big-endian keys walk in integer order",
            );
            assert!(
                !le_tokens.is_sorted(),
                "little-endian keys walk in byte order, not integer order",
            );
            assert_ne!(
                le_tokens, be_tokens,
                "swapping the encode endianness must change the observed walk order",
            );
        }

        // ── per-peer equivalence: the current peer walks byte order ──
        //
        // Under exactly-one-backend, cross-peer equivalence cannot be
        // proven in one process. Instead each build proves *its* peer
        // matches the mathematical byte-ordered sequence; running all
        // four peer builds through CI proves the four-way equivalence.

        #[test]
        fn for_each_matches_the_byte_ordered_sequence_le_keys() {
            let path = TempPath::new("xback-le");
            let rows = insert_and_walk::<DefaultStore>(&path.0, &le_keys());
            let keys: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
            assert_eq!(
                keys,
                sorted_by_bytes(&le_keys()),
                "the peer's for_each must be the byte-ordered walk",
            );
            let tokens: Vec<u32> = rows
                .iter()
                .map(|(k, _)| u32::from_le_bytes([k[0], k[1], k[2], k[3]]))
                .collect();
            assert!(
                !tokens.is_sorted(),
                "the shared order must be byte order across 256",
            );
        }

        #[test]
        fn for_each_matches_the_byte_ordered_sequence_be_keys() {
            let path = TempPath::new("xback-be");
            let rows = insert_and_walk::<DefaultStore>(&path.0, &be_keys());
            let keys: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
            assert_eq!(
                keys,
                sorted_by_bytes(&be_keys()),
                "the peer's for_each must be the byte-ordered walk",
            );
            let tokens: Vec<u32> = rows
                .iter()
                .map(|(k, _)| u32::from_be_bytes([k[0], k[1], k[2], k[3]]))
                .collect();
            assert!(
                tokens.is_sorted(),
                "big-endian keys share integer order across peers",
            );
        }

        #[test]
        fn range_matches_the_byte_ordered_slice_across_256() {
            let keys = le_keys();
            let lo = 0x0000_0100_u32.to_le_bytes();
            let hi = 0x0000_00FF_u32.to_le_bytes();
            let path = TempPath::new("xrange");
            let rows = insert_and_range::<DefaultStore>(&path.0, &keys, &lo, &hi);
            assert!(!rows.is_empty(), "the range must match rows");
            let expected: Vec<Vec<u8>> = sorted_by_bytes(&keys)
                .into_iter()
                .filter(|k| k.as_slice() >= &lo[..] && k.as_slice() < &hi[..])
                .collect();
            let out: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
            assert_eq!(
                out, expected,
                "range walk must be the ascending byte-ordered slice",
            );
        }

        #[test]
        fn composite_pair_walk_follows_byte_order() {
            // 8-byte (prev, cur) big-endian pairs — the user-store bigram
            // key shape — with both components crossing 256.
            let mut keys: Vec<Vec<u8>> = Vec::new();
            for &prev in &[0x0000_00FF_u32, 0x0000_0100, 0x0000_0101] {
                for &cur in &[0x0000_0001_u32, 0x0000_00FF, 0x0000_0100, 0x0001_0000] {
                    let mut key = Vec::with_capacity(8);
                    key.extend_from_slice(&prev.to_be_bytes());
                    key.extend_from_slice(&cur.to_be_bytes());
                    keys.push(key);
                }
            }
            let path = TempPath::new("xpair");
            let rows = insert_and_walk::<DefaultStore>(&path.0, &keys);
            let out: Vec<Vec<u8>> = rows.iter().map(|(k, _)| k.clone()).collect();
            assert_eq!(
                out,
                sorted_by_bytes(&keys),
                "the peer's for_each must be the byte-ordered walk",
            );
            let pairs: Vec<(u32, u32)> = rows
                .iter()
                .map(|(k, _)| {
                    (
                        u32::from_be_bytes([k[0], k[1], k[2], k[3]]),
                        u32::from_be_bytes([k[4], k[5], k[6], k[7]]),
                    )
                })
                .collect();
            assert!(
                pairs.is_sorted(),
                "big-endian (prev, cur) pairs walk in integer order",
            );
        }
    }
}
