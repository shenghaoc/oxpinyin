//! LMDB backend for the store capability tiers, powered by [heed].
//!
//! Enabled by the `lmdb` cargo feature.  Key ordering uses the default
//! LMDB byte-lexicographic comparator, so big-endian encoded keys sort
//! identically to the redb backend.

use std::collections::HashMap;
use std::fmt;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

use heed::types::Bytes;
use heed::{Database, EnvFlags, EnvOpenOptions, RwTxn, WithoutTls};

use crate::{ReadStore, StoreError, Visitor, WriteStore, WriteTxn, validate_table_name};

type Env = heed::Env<WithoutTls>;

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

// ── env sharing ───────────────────────────────────────────────────
//
// LMDB (via heed) refuses to open the same environment file twice in one
// process ("environment already open in this program"), while every other
// backend tolerates concurrent opens of one path — and the engine's tests
// and adapters lean on that (many capi tests open the same fixture
// tables in parallel). This registry restores the common contract for
// LMDB: one shared `Env` per canonical path, handed out by `Weak`
// upgrade, dropped (and the path reopenable) when the last store using
// it goes away — the same shape as `oxpinyin-user`'s store registry.

/// One registry row: the live environment, whether it was opened
/// writable, and the map-size ceiling it was opened with — heed can
/// neither reopen a live environment at a different ceiling nor resize
/// it, so a mismatching request must be refused rather than silently
/// handed the live ceiling.
type EnvSlot = (Weak<Env>, bool, usize);

static OPEN_ENVS: OnceLock<Mutex<HashMap<PathBuf, EnvSlot>>> = OnceLock::new();

/// The canonical registry key for `path` (falls back to the path itself
/// when it does not exist yet, e.g. `create` of a new file).
fn env_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// A re-open raced the previous environment's teardown. heed keeps a
/// process-global registry of open environments keyed by canonical path
/// (`heed::EnvOpenOptions::open` refuses a second open with
/// `EnvAlreadyOpened`) and removes the entry in `EnvInner::drop` —
/// **before** calling `mdb_env_close`, and after a wait on that
/// registry's single write lock. Two consequences for this registry:
///
/// * Between our `Weak` dying and heed removing its entry, a re-open of
///   the same path fails spuriously. Every open in this crate funnels
///   through `shared_env`, so a live conflicting environment cannot
///   exist and the failure is always transient — retry, never surface it.
/// * After heed removes the entry, `mdb_env_close` may still be running.
///   heed surfaces that window as `env_closing_event(path)`, and opening
///   the path while the close is in flight races it inside liblmdb —
///   observed as heap corruption (`malloc(): unaligned tcache chunk`)
///   under a newer glibc when parallel capi tests tore down and reopened
///   the same fixture files. So a re-open WAITS for the effective-close
///   event first; the retry is only a backstop.
fn is_transient_reopen(error: &StoreError) -> bool {
    let StoreError::Backend(inner) = error else {
        return false;
    };
    inner
        .downcast_ref::<heed::Error>()
        .is_some_and(|e| matches!(e, heed::Error::EnvAlreadyOpened))
}

/// One shared environment per path. A read-only request shares any live
/// env (the store-level `read_only` flag still refuses writes); a
/// writable request shares only a writable env — an env opened read-only
/// cannot be upgraded, so that mismatch is refused rather than handed a
/// handle that cannot write. A live env is shared only at the map size it
/// was opened with: a different ceiling cannot be applied to it, so that
/// mismatch is refused too, before the caller grows data past a ceiling
/// it does not actually hold.
fn shared_env(path: &Path, read_only: bool, map_size: usize) -> Result<Arc<Env>, StoreError> {
    let registry = OPEN_ENVS.get_or_init(|| Mutex::new(HashMap::new()));
    // 1ms doubling to 256ms: ~500ms total, orders of magnitude past the
    // teardown window, while a genuine stuck close still fails in bounded
    // time rather than hanging the caller.
    let mut backoff = std::time::Duration::from_millis(1);
    for attempt in 0..10 {
        if attempt > 0 {
            // Sleep without the registry lock: the thread finishing the
            // old environment's teardown never takes this mutex, and
            // unrelated paths keep flowing through the registry.
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(std::time::Duration::from_millis(256));
        }
        let mut map = registry.lock().unwrap_or_else(|p| p.into_inner());
        let key = env_key(path);
        if let Some((weak, writable, live_map_size)) = map.get(&key)
            && let Some(env) = weak.upgrade()
        {
            if *live_map_size != map_size {
                return Err(StoreError::InvalidInput(
                    "this LMDB file is already open in this process with a different map size; close those handles before opening it with this ceiling",
                ));
            }
            if read_only || *writable {
                return Ok(env);
            }
            return Err(StoreError::InvalidInput(
                "this LMDB file is already open read-only in this process; close those handles before opening it writable",
            ));
        }
        // No live environment for this path. A dead entry does not mean
        // the old environment has finished closing (see
        // `is_transient_reopen`'s doc): wait for heed's effective-close
        // event before this thread tries to open the path. The live path
        // above returns without waiting it out. No deadlock is possible —
        // this thread holds no copy of the environment (its Weak is
        // dead), and the closing thread never takes this registry's
        // mutex.
        if let Some(closing) = heed::env_closing_event(env_key(path)) {
            closing.wait_timeout(std::time::Duration::from_millis(1000));
        }
        // The open itself happens under the lock, so two first opens of
        // one path serialize here rather than both reaching heed.
        match open_env(path, read_only, map_size) {
            Ok(env) => {
                let env = Arc::new(env);
                // Re-key by the now-existing file so later opens of the
                // same file through a different spelling collide correctly.
                map.insert(env_key(path), (Arc::downgrade(&env), !read_only, map_size));
                return Ok(env);
            }
            Err(error) if is_transient_reopen(&error) => continue,
            Err(error) => return Err(error),
        }
    }
    Err(StoreError::Backend(Box::new(std::io::Error::other(
        "LMDB environment teardown did not complete in time to reopen it",
    ))))
}

// ── store ─────────────────────────────────────────────────────────

/// An LMDB-backed store implementing both capability tiers.
///
/// Feature-gated behind `lmdb`.  Uses a single file (`NO_SUB_DIR`)
/// and the default byte-lexicographic comparator.
///
/// LMDB caps an environment at 32 named tables (`MAX_DBS`). Writing to a
/// 33rd distinct table fails with [`StoreError::InvalidInput`]; the redb
/// backend has no such limit. Keep the total number of distinct table
/// names at or below 32 for cross-backend parity.
///
/// LMDB additionally refuses a second in-process open of the same
/// environment file; stores therefore share one `Env` per path through a
/// process-wide registry (see `shared_env`), matching the other backends'
/// open-many contract.
pub struct LmdbStore {
    env: Arc<Env>,
    #[allow(dead_code)]
    path: PathBuf,
    read_only: bool,
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
    /// [`WriteStore::create`] when the 1 GiB default is too small; use
    /// one consistent ceiling for a given file across processes.
    pub fn create_with_map_size(path: &Path, map_size: usize) -> Result<Self, StoreError> {
        let env = shared_env(path, false, map_size)?;
        Ok(Self {
            env,
            path: path.to_path_buf(),
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
        let env = shared_env(path, true, map_size)?;
        Ok(Self {
            env,
            path: path.to_path_buf(),
            read_only: true,
        })
    }
}

impl ReadStore for LmdbStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        let env = shared_env(path, true, MAP_SIZE)?;
        Ok(Self {
            env,
            path: path.to_path_buf(),
            read_only: true,
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
}

impl WriteStore for LmdbStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        let env = shared_env(path, false, MAP_SIZE)?;
        Ok(Self {
            env,
            path: path.to_path_buf(),
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
        // work here. redb's compaction can no longer fail either, now that
        // no read view outlives a call, so both backends succeed.
        Ok(())
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
