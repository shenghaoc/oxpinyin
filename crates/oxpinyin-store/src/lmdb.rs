//! LMDB backend for the store capability tiers, powered by [heed].
//!
//! Enabled by the `lmdb` cargo feature.  Key ordering uses the default
//! LMDB byte-lexicographic comparator, so big-endian encoded keys sort
//! identically to the redb backend.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::mem::ManuallyDrop;
use std::ops::{Bound, Deref};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, Weak};

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
/// Two independent hazards live here.
///
/// **Env close/reopen.** heed 0.22 holds its own process-global
/// `OPENED_ENV` write lock across `mdb_env_close`, so a fresh
/// `EnvOpenOptions::open` on the same path blocks until the close
/// finishes and two closes never overlap inside liblmdb. What that
/// alone did not cover is that the last `Arc<SharedEnv>` drop
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
/// **[`mdb_dbi_open`](http://www.lmdb.tech/doc/group__mdb.html#gac08cad5b096925642ca359a6d6f0562a).**
/// LMDB's own contract: *"A transaction that uses this function must
/// finish (either commit or abort) before any other transaction in the
/// process may use this function"*. Its body writes into the env-wide
/// `me_dbxs`/`me_dbiseqs` arrays (through the `mt_dbxs` pointer
/// `mdb_txn_begin` aliases at `mdb.c:3283`), and its `mdb_txn_end`
/// counterpart on abort walks the same arrays and `free()`s
/// `me_dbxs[i].md_name.mv_data` for every `DB_NEW` slot the aborting
/// txn opened (`mdb.c:3399-3405`). heed exposes
/// [`Env::open_database`]/[`Env::create_database`] as thin wrappers
/// around it, and every store op — `get`/`for_each`/`range`/`is_empty`
/// for reads, the write txn's `put`/`remove`/… for writes — would
/// otherwise re-open the same table by name on every call, colliding
/// on `me_dbxs` writes when two threads on one env raced (the tcache
/// trip surfaced in CI). The fix takes LMDB's own hint: *"once the
/// transaction that called `mdb_dbi_open` successfully commits, the
/// handle resides in the shared environment and may be used by other
/// transactions"* — cache the `Database` handle once, reuse it on
/// every subsequent op without holding any lock. Two mutexes carry
/// this: [`SharedEnv::dbis`] wraps a `HashMap` and is held only long
/// enough to read or write one entry; [`SharedEnv::dbi_open`] is the
/// process-wide serialization LMDB needs across the actual
/// `mdb_dbi_open` call. Read-side cache misses hold `dbi_open` for
/// the whole open-and-commit sequence and release it before returning;
/// write-side cache misses (through [`LmdbWriteTxn`]) hold `dbi_open`
/// from the first miss until the write txn commits or aborts, per the
/// spec's "opening txn must finish" clause. After the miss path
/// caches the handle, later store ops open their txn, use the cached
/// handle, run their reads or writes, and finish the txn — all with
/// no lock. That restores LMDB's concurrent-reader model.
///
/// Negative results are deliberately *not* cached. LMDB supports
/// several processes on one data file; a table that does not exist
/// when this env first probes it may be created later by another
/// process, and a cached "absent" would hide it for this env's
/// lifetime. Only present handles enter the cache; a miss re-probes
/// on every call, cheap next to the read txn that follows.
/// Newly-created tables land in [`LmdbWriteTxn::pending_cache`] and
/// are only promoted after the write txn commits (LMDB frees a
/// `DB_NEW` DBI on abort, so caching earlier would leave a dangling
/// handle).
struct SharedEnv {
    /// Kept in a [`ManuallyDrop`] so [`Drop::drop`] can run
    /// [`ManuallyDrop::drop`] on it explicitly, before releasing the
    /// registry mutex. Without that indirection Rust would drop the
    /// field after the impl returned and the serialization we rely on
    /// would be gone.
    inner: ManuallyDrop<Env>,
    key: PathBuf,
    /// Committed [`Database`] handles keyed by table name — positive
    /// cache only. The mutex is held only for the one map read or
    /// write; the actual `mdb_dbi_open` runs under [`Self::dbi_open`].
    dbis: Mutex<HashMap<String, Database<Bytes, Bytes>>>,
    /// Serializes `mdb_dbi_open` calls across all txns on this env,
    /// per LMDB's exclusion rule. Read-side misses in
    /// [`SharedEnv::database`] hold it across `open_database` + commit;
    /// write-side misses via [`LmdbWriteTxn`] hold it until the write
    /// txn commits or aborts (see [`LmdbWriteTxn::dbi_open_guard`]).
    /// Cache-hit paths never touch this mutex.
    dbi_open: Mutex<()>,
}

impl Deref for SharedEnv {
    type Target = Env;

    fn deref(&self) -> &Env {
        &self.inner
    }
}

impl SharedEnv {
    /// Return the [`Database`] handle for `name` if it exists,
    /// `None` when it does not. Positive results are cached. Misses
    /// are *not* cached — see the [`SharedEnv`] docblock for why.
    ///
    /// On a cache miss opens a private read txn, runs `mdb_dbi_open`,
    /// commits it (so the DBI persists env-wide per the LMDB spec),
    /// and — if the table exists — inserts the handle into
    /// [`Self::dbis`]. The [`Self::dbi_open`] mutex is held across
    /// the whole open-and-commit sequence to satisfy LMDB's
    /// exclusion rule; [`Self::dbis`] is held only for the one map
    /// operation on each side. Cache hits take neither mutex beyond
    /// the one map read.
    fn database(&self, name: &str) -> Result<Option<Database<Bytes, Bytes>>, StoreError> {
        // Fast path: positive cache hit.
        {
            let cache = self.lock_dbis();
            if let Some(db) = cache.get(name) {
                return Ok(Some(*db));
            }
        }
        // Slow path: serialize the `mdb_dbi_open` under `dbi_open`,
        // then re-check the cache — another thread may have promoted
        // the DBI while we waited on the mutex.
        let _open_guard = self.lock_dbi_open();
        {
            let cache = self.lock_dbis();
            if let Some(db) = cache.get(name) {
                return Ok(Some(*db));
            }
        }
        let txn = self.inner.read_txn().map_err(map_heed_error)?;
        let db: Option<Database<Bytes, Bytes>> = self
            .inner
            .open_database(&txn, Some(name))
            .map_err(map_heed_error)?;
        txn.commit().map_err(map_heed_error)?;
        if let Some(d) = db {
            self.lock_dbis().insert(name.to_owned(), d);
        }
        Ok(db)
    }

    /// Insert tables a just-committed write txn created into the
    /// shared cache, so subsequent reads and writes find them on the
    /// fast path.
    fn promote_created(&self, entries: &[(String, Database<Bytes, Bytes>)]) {
        if entries.is_empty() {
            return;
        }
        let mut cache = self.lock_dbis();
        for (name, db) in entries {
            cache.insert(name.clone(), *db);
        }
    }

    fn lock_dbis(&self) -> MutexGuard<'_, HashMap<String, Database<Bytes, Bytes>>> {
        self.dbis
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lock_dbi_open(&self) -> MutexGuard<'_, ()> {
        self.dbi_open
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
                    dbis: Mutex::new(HashMap::new()),
                    dbi_open: Mutex::new(()),
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
        let Some(db) = self.env.database(table)? else {
            return Ok(None);
        };
        let txn = self.env.read_txn().map_err(map_heed_error)?;
        Ok(db
            .get(&txn, key)
            .map_err(map_heed_error)?
            .map(|v| v.to_vec()))
    }

    fn for_each(&self, table: &str, visit: &mut Visitor<'_>) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let Some(db) = self.env.database(table)? else {
            return Ok(());
        };
        let txn = self.env.read_txn().map_err(map_heed_error)?;
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
        if is_empty_upper_bound(hi) {
            return Ok(());
        }
        let Some(db) = self.env.database(table)? else {
            return Ok(());
        };
        let txn = self.env.read_txn().map_err(map_heed_error)?;
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
        let Some(db) = self.env.database(table)? else {
            return Ok(true);
        };
        let txn = self.env.read_txn().map_err(map_heed_error)?;
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
            pending_cache: Vec::new(),
            dbi_open_guard: RefCell::new(None),
        };
        match f(&mut wtxn) {
            Ok(result) => {
                let LmdbWriteTxn {
                    env,
                    txn,
                    pending_cache,
                    dbi_open_guard,
                } = wtxn;
                txn.commit().map_err(map_heed_error)?;
                // Only after the write txn's `mdb_txn_end` commits are
                // the DBIs it opened valid env-wide; promote them into
                // the shared cache now so subsequent reads and writes
                // find them on the fast path.
                env.promote_created(&pending_cache);
                // Release `dbi_open` last: LMDB's exclusion rule keeps
                // no other txn's `mdb_dbi_open` from running until the
                // opening txn's `mdb_txn_end` has fully returned, and
                // that only completes above.
                drop(dbi_open_guard);
                Ok(result)
            }
            Err(error) => {
                let LmdbWriteTxn {
                    env: _,
                    txn,
                    pending_cache: _,
                    dbi_open_guard,
                } = wtxn;
                // Abort under the guard: `mdb_txn_end` walks
                // `me_dbxs` and `free()`s every `DB_NEW` slot the
                // txn opened (`mdb.c:3399-3405`); a concurrent
                // reader's `mdb_dbi_open` scan must be blocked while
                // that free runs.
                txn.abort();
                drop(dbi_open_guard);
                // Aborted DBIs were freed by `mdb_txn_end`; do not
                // touch the shared cache.
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
    env: &'a SharedEnv,
    txn: RwTxn<'a>,
    /// Tables the closure `put`'s into this write txn that were not
    /// already in the shared cache. Promoted into the cache in
    /// [`WriteStore::write`] on commit; discarded on abort, because
    /// LMDB frees a `DB_NEW` DBI when the txn ends without commit.
    pending_cache: Vec<(String, Database<Bytes, Bytes>)>,
    /// Populated the first time this txn hits a cache miss and calls
    /// `mdb_dbi_open` (via `open_database` in [`Self::open_existing`]
    /// or `create_database` in [`WriteTxn::put`]). LMDB's spec
    /// requires the opening txn to finish (commit or abort) before
    /// any other txn on the env may call `mdb_dbi_open`, so
    /// [`WriteStore::write`] holds the guard until after
    /// `txn.commit()`/`txn.abort()` returns and only then drops it.
    /// A [`RefCell`] gives interior mutability so the `&self` read
    /// methods on [`WriteTxn`] can lazily acquire the guard too.
    dbi_open_guard: RefCell<Option<MutexGuard<'a, ()>>>,
}

impl<'a> LmdbWriteTxn<'a> {
    /// Ensure this txn holds the env's [`SharedEnv::dbi_open`] guard
    /// before we call `mdb_dbi_open`. First-miss on the txn acquires
    /// the guard; every later miss finds it already held and just
    /// keeps it.
    fn hold_dbi_open(&self) {
        let mut slot = self.dbi_open_guard.borrow_mut();
        if slot.is_none() {
            *slot = Some(self.env.lock_dbi_open());
        }
    }

    /// Look the table up in the shared cache or open it read-only
    /// through the write txn without inserting a new DBI. Read-side
    /// ops on the write txn never need `MDB_CREATE` — an absent
    /// table just answers "empty" per the trait contract. On a
    /// cache miss `open_database` calls `mdb_dbi_open`, so hold
    /// [`SharedEnv::dbi_open`] for the rest of the txn's life.
    fn open_existing(&self, table: &str) -> Result<Option<Database<Bytes, Bytes>>, StoreError> {
        {
            let cache = self.env.lock_dbis();
            if let Some(db) = cache.get(table) {
                return Ok(Some(*db));
            }
        }
        self.hold_dbi_open();
        // Re-check under the guard: another writer's `promote_created`
        // may have raced us into the miss path and already inserted the
        // handle. This is unlikely (write txns on one env serialize
        // through heed's writer lock and would have blocked us on
        // `write_txn()`), but the check keeps the invariant tight for
        // any cross-env-instance sharing.
        {
            let cache = self.env.lock_dbis();
            if let Some(db) = cache.get(table) {
                return Ok(Some(*db));
            }
        }
        self.env
            .inner
            .open_database(&self.txn, Some(table))
            .map_err(map_heed_error)
    }

    /// Return the cached [`Database`] for `table` if positive, else
    /// `create_database` on this txn (staging the new DBI in
    /// [`Self::pending_cache`] for promotion after commit). The
    /// returned handle is *not* published to the shared cache from
    /// here — that would leak a dangling DBI on abort, since LMDB
    /// frees the slot when a `DB_NEW` txn ends without commit.
    fn ensure_created(&mut self, table: &str) -> Result<Database<Bytes, Bytes>, StoreError> {
        {
            let cache = self.env.lock_dbis();
            if let Some(db) = cache.get(table) {
                return Ok(*db);
            }
        }
        // We're about to call `mdb_dbi_open` on the write txn; hold
        // the env's `dbi_open` guard until the txn commits or aborts.
        self.hold_dbi_open();
        {
            let cache = self.env.lock_dbis();
            if let Some(db) = cache.get(table) {
                return Ok(*db);
            }
        }
        let db = self
            .env
            .inner
            .create_database(&mut self.txn, Some(table))
            .map_err(|e| match e {
                // LMDB caps the environment at MAX_DBS named tables;
                // the over-limit table name is caller input, so
                // surface it as InvalidInput rather than an opaque
                // backend error.
                heed::Error::Mdb(heed::MdbError::DbsFull) => {
                    StoreError::InvalidInput("too many distinct tables (LMDB caps a store at 32)")
                }
                other => map_heed_error(other),
            })?;
        // Stage the handle for post-commit promotion.
        if !self.pending_cache.iter().any(|(name, _)| name == table) {
            self.pending_cache.push((table.to_owned(), db));
        }
        Ok(db)
    }
}

impl WriteTxn for LmdbWriteTxn<'_> {
    fn get(&self, table: &str, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        validate_table_name(table)?;
        let Some(db) = self.open_existing(table)? else {
            return Ok(None);
        };
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
        let db = self.ensure_created(table)?;
        db.put(&mut self.txn, key, value).map_err(map_heed_error)?;
        Ok(())
    }

    fn remove(&mut self, table: &str, key: &[u8]) -> Result<(), StoreError> {
        validate_table_name(table)?;
        let Some(db) = self.open_existing(table)? else {
            return Ok(());
        };
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
        if is_empty_upper_bound(hi) {
            return Ok(());
        }
        let Some(db) = self.open_existing(table)? else {
            return Ok(());
        };
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
        let Some(db) = self.open_existing(table)? else {
            return Ok(());
        };
        let iter = db.iter(&self.txn).map_err(map_heed_error)?;
        for result in iter {
            let (key, value) = result.map_err(map_heed_error)?;
            visit(key, value)?;
        }
        Ok(())
    }

    fn is_empty(&self, table: &str) -> Result<bool, StoreError> {
        validate_table_name(table)?;
        let Some(db) = self.open_existing(table)? else {
            return Ok(true);
        };
        db.is_empty(&self.txn).map_err(map_heed_error)
    }
}
