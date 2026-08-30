//! LMDB backend for the store capability tiers, powered by [heed].
//!
//! Enabled by the `lmdb` cargo feature.  Key ordering uses the default
//! LMDB byte-lexicographic comparator, so big-endian encoded keys sort
//! identically to the redb backend.

use std::collections::HashMap;
use std::fmt;
use std::mem::ManuallyDrop;
use std::ops::{Bound, Deref};
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
type EnvSlot = (Weak<SharedEnv>, bool, usize);

static OPEN_ENVS: OnceLock<Mutex<HashMap<PathBuf, EnvSlot>>> = OnceLock::new();

fn open_envs() -> &'static Mutex<HashMap<PathBuf, EnvSlot>> {
    OPEN_ENVS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// The canonical registry key for `path` (falls back to the path itself
/// when it does not exist yet, e.g. `create` of a new file).
fn env_key(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// One shared heed environment plus the machinery that keeps LMDB
/// operations on it safe to call from many threads at once.
///
/// Two hazards live here.
///
/// The first is our registry's own close/reopen gap. heed 0.22 holds its
/// own process-global `OPENED_ENV` write lock across `mdb_env_close`, so
/// a fresh `EnvOpenOptions::open` on the same path blocks until the
/// close finishes and two closes never overlap inside liblmdb. What
/// that alone did not cover is that the last `Arc<SharedEnv>` drop
/// decrements the strong count to zero *before* Rust runs this Drop
/// impl, so a concurrent [`shared_env`] caller can see
/// [`Weak::upgrade`] return `None`, call [`heed::env_closing_event`]
/// once heed's Drop has removed its entry, get `None` back, and rush
/// into [`open_env`] before liblmdb has finished the previous env's
/// teardown work. Under a newer glibc that window manifested as
/// `malloc(): unaligned tcache chunk detected` aborts in parallel capi
/// and workspace test runs (main tip `fcf0559`). [`Drop`] takes the
/// registry mutex, evicts the (dead) entry, and only then drops the
/// inner env — so heed's `mdb_env_close` runs while we hold the same
/// mutex that [`shared_env`] takes for its live-check and its
/// `open_env` call. A concurrent caller either sees the live env on
/// the fast path or blocks until the close has fully finished, and
/// never races liblmdb.
///
/// The second is [`mdb_dbi_open`](http://www.lmdb.tech/doc/group__mdb.html#gac08cad5b096925642ca359a6d6f0562a).
/// LMDB's own contract: *"A transaction that uses this function must
/// finish (either commit or abort) before any other transaction in the
/// process may use this function"*. Its body writes into the env-wide
/// `me_dbxs`/`me_dbiseqs` arrays (through the `mt_dbxs` pointer
/// `mdb_txn_begin` aliases at `mdb.c:3283`), and its `mdb_txn_end`
/// counterpart on abort walks the same arrays and `free()`s
/// `me_dbxs[i].md_name.mv_data` for every `DB_NEW` slot the aborting
/// txn opened (`mdb.c:3399-3405`). heed exposes
/// [`Env::open_database`]/[`Env::create_database`] as thin wrappers
/// around `mdb_dbi_open` and does not serialize them or the abort. Every
/// `LmdbStore` op (`get`/`for_each`/`range`/`is_empty` for reads, the
/// write txn's `put`/`remove` closures for writes) opens a fresh txn and
/// then opens (or creates) the same table by name, so two threads on one
/// env would allocate the same unused slot for a first-time open and
/// the loser's `mdb_txn_end` abort would `free()` the winner's still-in-use
/// `md_name.mv_data` — the tcache trip surfaced in CI. Even with the
/// narrower "lock only across `open_database`" wrapper (commit c504428),
/// the aborting-side of that race stays open: releasing the mutex after
/// `mdb_dbi_open` returns lets a second thread scan `me_dbxs` while the
/// first thread's txn drop is freeing entries it wrote.
///
/// [`SharedEnv::with_dbi_lock`] holds a per-env [`Mutex<()>`] for the
/// full closure — the caller opens its txn, does its
/// `open_database`/`create_database` and reads or writes with those
/// handles, and finishes the txn (commit or abort) inside that closure.
/// The mutex releases only after all of that, restoring LMDB's pinned
/// exclusion end to end. Contention stays inside a single env —
/// unrelated `SharedEnv`s never touch this lock — and stores in this
/// crate are opened for one lifetime and used sequentially in the
/// production runtime; the mutex only serializes the parallel-tests
/// workload the earlier crash surfaced from.
struct SharedEnv {
    /// Kept in a [`ManuallyDrop`] so [`Drop::drop`] can run
    /// [`ManuallyDrop::drop`] on it explicitly, before releasing the
    /// registry mutex. Without that indirection Rust would drop the
    /// field after the impl returned and the serialization we rely on
    /// would be gone.
    inner: ManuallyDrop<Env>,
    key: PathBuf,
    /// Held for the full lifetime of any txn that will call
    /// `mdb_dbi_open`, which LMDB requires as a per-process single-writer
    /// on both the open and the txn-end paths.
    dbi_lock: Mutex<()>,
}

impl Deref for SharedEnv {
    type Target = Env;

    fn deref(&self) -> &Env {
        &self.inner
    }
}

impl SharedEnv {
    /// Runs `f` while holding this env's `dbi_lock`. Callers open their
    /// txn, open or create the databases they need, do the reads or
    /// writes on those handles, and let the txn finish (`commit` /
    /// `abort` / drop) — all *inside* `f`, so the aborting-txn free
    /// (see the struct docstring's second hazard) can never race
    /// another thread's `open_database` scan on this env.
    fn with_dbi_lock<R>(&self, f: impl FnOnce(&Env) -> R) -> R {
        let _guard = self
            .dbi_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        f(&self.inner)
    }
}

#[allow(unsafe_code)]
impl Drop for SharedEnv {
    fn drop(&mut self) {
        let mut map = open_envs()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Evict our own entry only. A concurrent `shared_env` waiting on
        // this mutex may have already registered a fresh env under the
        // same key (e.g. after a re-open replaced our dead Weak), and
        // that entry is not ours to clear.
        if map
            .get(&self.key)
            .is_some_and(|slot| slot.0.strong_count() == 0)
        {
            map.remove(&self.key);
        }
        // SAFETY: `self` is being dropped and `inner` is not read again.
        // We drop it here (rather than letting Rust drop it after this
        // function returns) so heed's `EnvInner::drop` — which calls
        // `mdb_env_close` — runs while we still hold the registry
        // mutex. A concurrent `shared_env` caller waits on that mutex,
        // so it cannot re-enter liblmdb on this or any other path
        // before the close is fully done.
        unsafe { ManuallyDrop::drop(&mut self.inner) };
    }
}

/// A re-open observed heed's `EnvAlreadyOpened` because our SharedEnv
/// wrapper's Drop had already evicted our Weak entry but hadn't yet
/// reached heed's own Drop (both are ordered under our registry mutex,
/// but the window between "Weak strong count reaches 0" and "our Drop
/// takes the mutex" is inherent to `Arc`). Every open in this crate
/// funnels through `shared_env`, so a genuinely-live conflicting env
/// cannot exist and the failure is always transient — retry, never
/// surface it.
fn is_transient_reopen(error: &StoreError) -> bool {
    let StoreError::Backend(inner) = error else {
        return false;
    };
    inner
        .downcast_ref::<heed::Error>()
        .is_some_and(|e| matches!(e, heed::Error::EnvAlreadyOpened))
}

/// The registry's answer for a live environment: the shared handle
/// paired with a compatibility check (`Ok(())` on a match, an
/// [`StoreError::InvalidInput`] refusal on a mismatch), or `None` when
/// no live environment exists for `key`. The caller holds the registry
/// lock; nothing here waits.
///
/// The handle comes back paired with — never swallowed by — the check
/// so the caller can release the registry lock before it drops. If a
/// concurrent thread already dropped its Arc, `weak.upgrade()` here can
/// hold the LAST strong reference; dropping that Arc while the registry
/// mutex is held would re-enter [`SharedEnv::drop`], which locks the
/// same non-reentrant mutex — a self-deadlock that would freeze every
/// later `shared_env` call and every `SharedEnv::drop` in the process.
fn live_env(
    map: &HashMap<PathBuf, EnvSlot>,
    key: &Path,
    read_only: bool,
    map_size: usize,
) -> Option<(Arc<SharedEnv>, Result<(), StoreError>)> {
    let (weak, writable, live_map_size) = map.get(key)?;
    let env = weak.upgrade()?;
    if *live_map_size != map_size {
        return Some((
            env,
            Err(StoreError::InvalidInput(
                "this LMDB file is already open in this process with a different map size; close those handles before opening it with this ceiling",
            )),
        ));
    }
    if read_only || *writable {
        return Some((env, Ok(())));
    }
    Some((
        env,
        Err(StoreError::InvalidInput(
            "this LMDB file is already open read-only in this process; close those handles before opening it writable",
        )),
    ))
}

/// One shared environment per path. A read-only request shares any live
/// env (the store-level `read_only` flag still refuses writes); a
/// writable request shares only a writable env — an env opened read-only
/// cannot be upgraded, so that mismatch is refused rather than handed a
/// handle that cannot write. A live env is shared only at the map size it
/// was opened with: a different ceiling cannot be applied to it, so that
/// mismatch is refused too, before the caller grows data past a ceiling
/// it does not actually hold.
///
/// The whole open sequence — live-env check, then `open_env` on a miss —
/// runs while the registry mutex is held, so it serializes with
/// [`SharedEnv::drop`] (which takes the same mutex around heed's own
/// `mdb_env_close`). A very short backoff loop remains only to absorb
/// the small `Arc`-decrement/Drop-start window: when a concurrent Arc's
/// strong count has just reached zero, `Weak::upgrade` already returns
/// `None` but our Drop has not yet taken the mutex, so a first attempt
/// can race heed's own drop and come back `EnvAlreadyOpened`. The retry
/// runs behind the mutex and observes the freshly-evicted slot.
fn shared_env(path: &Path, read_only: bool, map_size: usize) -> Result<Arc<SharedEnv>, StoreError> {
    let registry = open_envs();
    // 1ms doubling to 256ms: ~500ms total, orders of magnitude past the
    // Arc-decrement window we retry against, while a genuine stuck close
    // still fails in bounded time rather than hanging the caller.
    let mut backoff = std::time::Duration::from_millis(1);
    for attempt in 0..10 {
        if attempt > 0 {
            // Sleep without the registry lock: the closing thread needs
            // that mutex to make progress, and unrelated paths keep
            // flowing through the registry.
            std::thread::sleep(backoff);
            backoff = (backoff * 2).min(std::time::Duration::from_millis(256));
        }
        let key = env_key(path);
        let mut map = registry.lock().unwrap_or_else(|p| p.into_inner());
        if let Some((env, check)) = live_env(&map, &key, read_only, map_size) {
            // Release the registry mutex before `env` can drop: on the
            // mismatch paths `check` is `Err`, so `env` is dropped when
            // `check.map(..)` discards it, and that drop must not
            // re-enter `SharedEnv::drop` while we still hold the mutex.
            drop(map);
            return check.map(|()| env);
        }
        match open_env(path, read_only, map_size) {
            Ok(env) => {
                let env = Arc::new(SharedEnv {
                    inner: ManuallyDrop::new(env),
                    key: env_key(path),
                    dbi_lock: Mutex::new(()),
                });
                // Re-key by the now-existing file so later opens of the
                // same file through a different spelling collide correctly.
                map.insert(
                    env.key.clone(),
                    (Arc::downgrade(&env), !read_only, map_size),
                );
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
    env: Arc<SharedEnv>,
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
        self.env.with_dbi_lock(|env| {
            let txn = env.read_txn().map_err(map_heed_error)?;
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(&txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(None) };
            Ok(db
                .get(&txn, key)
                .map_err(map_heed_error)?
                .map(|v| v.to_vec()))
        })
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        validate_table_name(table)?;
        self.env.with_dbi_lock(|env| {
            let txn = env.read_txn().map_err(map_heed_error)?;
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(&txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(()) };
            let iter = db.iter(&txn).map_err(map_heed_error)?;
            for result in iter {
                let (key, value) = result.map_err(map_heed_error)?;
                visit(key, value)?;
            }
            Ok(())
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
        if is_empty_upper_bound(hi) {
            return Ok(());
        }
        self.env.with_dbi_lock(|env| {
            let txn = env.read_txn().map_err(map_heed_error)?;
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(&txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(()) };
            let bounds = (normalize_bound(lo), normalize_bound(hi));
            let iter = db.range(&txn, &bounds).map_err(map_heed_error)?;
            for result in iter {
                let (key, value) = result.map_err(map_heed_error)?;
                visit(key, value)?;
            }
            Ok(())
        })
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        self.env.with_dbi_lock(|env| {
            let txn = env.read_txn().map_err(map_heed_error)?;
            let db: Option<Database<Bytes, Bytes>> = env
                .open_database(&txn, Some(table))
                .map_err(map_heed_error)?;
            let Some(db) = db else { return Ok(true) };
            db.is_empty(&txn).map_err(map_heed_error)
        })
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
        self.env.with_dbi_lock(|env| {
            let txn = env.write_txn().map_err(map_heed_error)?;
            let mut wtxn = LmdbWriteTxn { env, txn };
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
        })
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
