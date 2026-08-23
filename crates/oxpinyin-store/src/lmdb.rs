//! LMDB backend for [`OrderedStore`], powered by [heed].
//!
//! Enabled by the `lmdb` cargo feature.  Key ordering uses the default
//! LMDB byte-lexicographic comparator, so big-endian encoded keys sort
//! identically to the redb backend.

use std::fmt;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use heed::types::Bytes;
use heed::{Database, EnvFlags, EnvOpenOptions, RwTxn, WithoutTls};
use self_cell::self_cell;

use crate::{OrderedStore, ReadSnapshot, StoreError, Visitor, WriteTxn, validate_table_name};

type Env = heed::Env<WithoutTls>;
type SnapTxn<'a> = heed::RoTxn<'a, WithoutTls>;

// ── helpers ───────────────────────────────────────────────────────

const MAX_DBS: u32 = 32;
/// LMDB accepts keys of at most 511 bytes and rejects the empty key
/// (`MDB_BAD_VALSIZE`); enforced ahead of insertion so callers get
/// [`StoreError::InvalidInput`] instead of a backend error.
const MAX_KEY_LEN: usize = 511;
/// Default map-size ceiling: 1 GiB of virtual address space.  LMDB
/// commits address space sparsely, so this is a cap on database size,
/// not an up-front allocation.  heed cannot resize an open environment,
/// so exceeding the cap fails every write with [`MdbError::MapFull`];
/// users with larger corpora should open via
/// [`LmdbStore::create_with_map_size`].
const MAP_SIZE: usize = 1 << 30;

fn map_heed_error(e: heed::Error) -> StoreError {
    match e {
        heed::Error::Io(io) => StoreError::Io(io),
        heed::Error::Mdb(heed::MdbError::MapFull) => StoreError::Backend(Box::new(MapFullError)),
        other => StoreError::Backend(Box::new(other)),
    }
}

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

/// Compaction was requested while a read snapshot was still open.
#[derive(Debug)]
struct SnapshotOpenError;

impl fmt::Display for SnapshotOpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("cannot compact while a read snapshot is open; drop it first")
    }
}

impl std::error::Error for SnapshotOpenError {}

fn validate_path(path: &Path) -> Result<(), StoreError> {
    if path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(StoreError::InvalidInput("path contains NUL"));
    }
    Ok(())
}

fn normalize_bound(bound: Bound<&[u8]>) -> Bound<&[u8]> {
    match bound {
        Bound::Included([]) | Bound::Excluded([]) => Bound::Unbounded,
        other => other,
    }
}

fn is_empty_upper_bound(bound: Bound<&[u8]>) -> bool {
    matches!(bound, Bound::Included([]) | Bound::Excluded([]))
}

#[allow(unsafe_code)]
fn open_env(path: &Path, read_only: bool, map_size: usize) -> Result<Env, StoreError> {
    validate_path(path)?;
    if map_size == 0 {
        return Err(StoreError::InvalidInput("map size must be nonzero"));
    }
    let mut opts = EnvOpenOptions::new().read_txn_without_tls();
    opts.max_dbs(MAX_DBS);
    opts.map_size(map_size);
    let mut flags = EnvFlags::NO_SUB_DIR;
    if read_only {
        flags |= EnvFlags::READ_ONLY;
    }
    // SAFETY: we uphold LMDB's contract — a single process opens each
    // data file with a consistent map-size, and heed's RwTxn borrow
    // enforces the single-writer invariant within this process.
    // `flags()` is unsafe because certain flag combinations can violate
    // LMDB invariants; our chosen flags (NO_SUB_DIR ± READ_ONLY) are safe.
    unsafe {
        opts.flags(flags);
        opts.open(path)
    }
    .map_err(|e| match e {
        // heed rejects a map size that is not a multiple of the system
        // page size before touching LMDB, surfacing it as an
        // `ErrorKind::InvalidInput` I/O error; reclassify it so callers
        // see StoreError::InvalidInput rather than Io.
        heed::Error::Io(io) if io.kind() == std::io::ErrorKind::InvalidInput => {
            StoreError::InvalidInput("map size must be a multiple of the system page size")
        }
        other => map_heed_error(other),
    })
}

// ── store ─────────────────────────────────────────────────────────

/// An LMDB-backed [`OrderedStore`].
///
/// Feature-gated behind `lmdb`.  Uses a single file (`NO_SUB_DIR`)
/// and the default byte-lexicographic comparator.
///
/// LMDB caps an environment at 32 named tables (`MAX_DBS`). Writing to a
/// 33rd distinct table fails with [`StoreError::InvalidInput`]; the redb
/// backend has no such limit. Keep the total number of distinct table
/// names at or below 32 for cross-backend parity.
pub struct LmdbStore {
    env: Arc<Env>,
    #[allow(dead_code)]
    path: PathBuf,
    read_only: bool,
    live_snapshots: Arc<AtomicUsize>,
}

impl LmdbStore {
    /// Open or create the store with a non-default map-size ceiling
    /// (`bytes` of virtual address space; LMDB commits it sparsely).
    ///
    /// `map_size` must be a multiple of the system page size; other
    /// values fail with [`StoreError::InvalidInput`].
    ///
    /// heed cannot resize an open environment, so the ceiling chosen at
    /// open time is fixed for the store's lifetime.  Use this instead of
    /// [`OrderedStore::create`] when the 1 GiB default is too small; use
    /// one consistent ceiling for a given file across processes.
    pub fn create_with_map_size(path: &Path, map_size: usize) -> Result<Self, StoreError> {
        let env = open_env(path, false, map_size)?;
        Ok(Self {
            env: Arc::new(env),
            path: path.to_path_buf(),
            read_only: false,
            live_snapshots: Arc::new(AtomicUsize::new(0)),
        })
    }

    /// Open the store read-only with a non-default map-size ceiling.
    ///
    /// [`OrderedStore::open_read_only`] uses the 1 GiB default, which
    /// cannot reopen a store that was grown past that ceiling with
    /// [`LmdbStore::create_with_map_size`]: LMDB rejects a map size smaller
    /// than the data already on disk.  Pass the same (or a larger) ceiling
    /// used to create the store.
    ///
    /// `map_size` must be a multiple of the system page size; other values
    /// fail with [`StoreError::InvalidInput`].
    pub fn open_read_only_with_map_size(path: &Path, map_size: usize) -> Result<Self, StoreError> {
        let env = open_env(path, true, map_size)?;
        Ok(Self {
            env: Arc::new(env),
            path: path.to_path_buf(),
            read_only: true,
            live_snapshots: Arc::new(AtomicUsize::new(0)),
        })
    }
}

impl OrderedStore for LmdbStore {
    type ReadSnapshot = LmdbReadSnapshot;

    fn snapshot(&self) -> Result<LmdbReadSnapshot, StoreError> {
        let inner = SnapshotInner::try_new(self.env.clone(), |env| {
            env.read_txn().map_err(map_heed_error)
        })?;
        self.live_snapshots.fetch_add(1, Ordering::Release);
        Ok(LmdbReadSnapshot {
            inner,
            _live: LiveSnapshotGuard(Arc::clone(&self.live_snapshots)),
        })
    }

    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let env = open_env(path, true, MAP_SIZE)?;
        Ok(Self {
            env: Arc::new(env),
            path: path.to_path_buf(),
            read_only: true,
            live_snapshots: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn create(path: &Path) -> Result<Self, StoreError> {
        let env = open_env(path, false, MAP_SIZE)?;
        Ok(Self {
            env: Arc::new(env),
            path: path.to_path_buf(),
            read_only: false,
            live_snapshots: Arc::new(AtomicUsize::new(0)),
        })
    }

    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        let txn = self.env.read_txn().map_err(map_heed_error)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(None) };
        Ok(db
            .get(&txn, key)
            .map_err(map_heed_error)?
            .map(|v| v.to_vec()))
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let txn = self.env.read_txn().map_err(map_heed_error)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(()) };
        let iter = db.iter(&txn).map_err(map_heed_error)?;
        for result in iter {
            let (key, value) = result.map_err(map_heed_error)?;
            visit(key, value)?;
        }
        Ok(())
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let txn = self.env.read_txn().map_err(map_heed_error)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(()) };
        if is_empty_upper_bound(hi) {
            return Ok(());
        }
        let bounds = (normalize_bound(lo), normalize_bound(hi));
        let iter = db.range(&txn, &bounds).map_err(map_heed_error)?;
        for result in iter {
            let (key, value) = result.map_err(map_heed_error)?;
            visit(key, value)?;
        }
        Ok(())
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let txn = self.env.read_txn().map_err(map_heed_error)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(true) };
        db.is_empty(&txn).map_err(map_heed_error)
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&mut dyn WriteTxn) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        let txn = self.env.write_txn().map_err(map_heed_error)?;
        let mut wtxn = LmdbWriteTxn {
            env: &self.env,
            txn,
        };
        match f(&mut wtxn) {
            Ok(result) => {
                wtxn.txn.commit().map_err(map_heed_error)?;
                Ok(result)
            }
            Err(error) => {
                wtxn.txn.abort();
                Err(error)
            }
        }
    }

    fn compact(&mut self) -> Result<(), StoreError> {
        if self.read_only {
            return Err(StoreError::ReadOnly);
        }
        // LMDB reclaims freed pages in place, so compaction itself does no
        // work here. The live-snapshot check is nonetheless retained so this
        // backend rejects the same precondition as redb (whose compaction
        // cannot run while a read snapshot pins pages); keeping the two
        // backends' contracts identical. Do not remove it as dead logic.
        if self.live_snapshots.load(Ordering::Acquire) > 0 {
            return Err(StoreError::Backend(Box::new(SnapshotOpenError)));
        }
        Ok(())
    }
}

// ── read snapshot ─────────────────────────────────────────────────

self_cell!(
    struct SnapshotInner {
        owner: Arc<Env>,
        #[not_covariant]
        dependent: SnapTxn,
    }
);

/// Decrements the live-snapshot counter when it drops.
///
/// Held as the last field of [`LmdbReadSnapshot`] so it runs *after*
/// `inner` releases the LMDB read transaction (fields drop in declaration
/// order). The counter therefore never reaches zero while a transaction is
/// still open, which closes a latent TOCTOU should [`LmdbStore::compact`]
/// ever be changed to take `&self` instead of `&mut self`.
struct LiveSnapshotGuard(Arc<AtomicUsize>);

impl Drop for LiveSnapshotGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

/// A consistent read snapshot backed by an LMDB read transaction.
pub struct LmdbReadSnapshot {
    /// Dropped before `_live` (declaration order) so the read transaction
    /// is released before the live-snapshot counter is decremented.
    inner: SnapshotInner,
    _live: LiveSnapshotGuard,
}

impl ReadSnapshot for LmdbReadSnapshot {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        self.inner.with_dependent(|env, txn| {
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(None) };
            Ok(db
                .get(txn, key)
                .map_err(map_heed_error)?
                .map(|v| v.to_vec()))
        })
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.inner.with_dependent(|env, txn| {
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(()) };
            if is_empty_upper_bound(hi) {
                return Ok(());
            }
            let bounds = (normalize_bound(lo), normalize_bound(hi));
            let iter = db.range(txn, &bounds).map_err(map_heed_error)?;
            for result in iter {
                let (key, value) = result.map_err(map_heed_error)?;
                visit(key, value)?;
            }
            Ok(())
        })
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.inner.with_dependent(|env, txn| {
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(()) };
            let iter = db.iter(txn).map_err(map_heed_error)?;
            for result in iter {
                let (key, value) = result.map_err(map_heed_error)?;
                visit(key, value)?;
            }
            Ok(())
        })
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        self.inner.with_dependent(|env, txn| {
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(true) };
            db.is_empty(txn).map_err(map_heed_error)
        })
    }
}

// ── write transaction ─────────────────────────────────────────────

struct LmdbWriteTxn<'a> {
    env: &'a Env,
    txn: RwTxn<'a>,
}

impl WriteTxn for LmdbWriteTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&self.txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(None) };
        Ok(db
            .get(&self.txn, key)
            .map_err(map_heed_error)?
            .map(|v| v.to_vec()))
    }

    fn put(&mut self, table: &str, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        if key.is_empty() || key.len() > MAX_KEY_LEN {
            return Err(StoreError::InvalidInput("key length must be 1..=511 bytes"));
        }
        let db: Database<Bytes, Bytes> = self
            .env
            .create_database(&mut self.txn, Some(table))
            .map_err(|e| match e {
                // LMDB caps the environment at MAX_DBS named tables; the
                // over-limit table name is caller input, so surface it as
                // InvalidInput rather than an opaque backend error.
                heed::Error::Mdb(heed::MdbError::DbsFull) => {
                    StoreError::InvalidInput("too many distinct tables (LMDB caps a store at 32)")
                }
                other => map_heed_error(other),
            })?;
        db.put(&mut self.txn, key, value).map_err(map_heed_error)?;
        Ok(())
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&self.txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(()) };
        db.delete(&mut self.txn, key).map_err(map_heed_error)?;
        Ok(())
    }

    fn range(
        &self,
        table: &str,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: &mut Visitor<'_>,
    ) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&self.txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(()) };
        if is_empty_upper_bound(hi) {
            return Ok(());
        }
        let bounds = (normalize_bound(lo), normalize_bound(hi));
        let iter = db.range(&self.txn, &bounds).map_err(map_heed_error)?;
        for result in iter {
            let (key, value) = result.map_err(map_heed_error)?;
            visit(key, value)?;
        }
        Ok(())
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&self.txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(()) };
        let iter = db.iter(&self.txn).map_err(map_heed_error)?;
        for result in iter {
            let (key, value) = result.map_err(map_heed_error)?;
            visit(key, value)?;
        }
        Ok(())
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .env
            .open_database(&self.txn, Some(table))
            .map_err(map_heed_error)?;
        let Some(db) = db else { return Ok(true) };
        db.is_empty(&self.txn).map_err(map_heed_error)
    }
}
