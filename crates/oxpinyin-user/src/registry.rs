//! Process-global live-handle registry for [`crate::UserStore`].
//!
//! the backend refuses a second write handle to a file already open in this process.
//! A second [`crate::UserStore::open`] of the same path therefore reuses the
//! live handle (shared counts and shared §4 dirty flag) instead of failing —
//! the C ABI's degrade-to-`None` would otherwise silently disable learning.
//!
//! The last [`crate::UserStore`] drop drains dead `Weak`s and shrinks an empty
//! map so a `dlclose`d cdylib leaves no heap behind.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Mutex, MutexGuard, OnceLock, Weak};

use oxpinyin_core::hash::FixedHashMap;
use oxpinyin_store::{DefaultStore, WriteStore};

use crate::store::Token;

/// Decode-time memo of the unigram count tables and the user bigram's raw
/// grams, valid for one write generation.
///
/// The store used to answer these reads through an MVCC read snapshot
/// held open across calls. Nothing in this deployment shares a store
/// file with a concurrent writer — one user, own files, one process —
/// so the isolation that bought was never load-bearing; what was
/// load-bearing is that a read repeated inside one generation costs
/// nothing and answers the same value every time. A plain memo gives
/// both: a key is fetched from the store the first time it is asked
/// for and served from memory afterwards, and the whole cache is
/// dropped the moment `write_generation` moves.
///
/// This is the freshness contract [`crate::lookup::UserPhraseLookup`]
/// already runs on — keyed by generation, rebuilt when it moves, no
/// MVCC anywhere. The difference is that the count tables are the large
/// ones, so rows are memoised on demand rather than slurped in bulk: a
/// write costs no rebuild scan, and the cache never grows past the keys
/// decode actually asked about between two writes. An absent row
/// memoises as `0`, exactly what the store read it replaces returned.
#[derive(Debug)]
pub struct CountCache {
    /// The `write_generation` this cache was built for; a mismatch
    /// retires it wholesale.
    pub(crate) generation: u64,
    /// `UNIGRAM[token]`.
    pub(crate) unigram: FixedHashMap<Token, u64>,
    /// `UNIGRAM_TOTAL`'s single row.
    pub(crate) unigram_total: Option<u64>,
    /// The raw gram under each predecessor scored since the last write
    /// that has one. Counts and totals are read from the bytes as the pin
    /// reads them (`get_freq`, `get_total_freq`).
    pub(crate) bigram_rows: FixedHashMap<Token, Vec<u8>>,
}

/// Accepted exported increments since the last operation that writes counts.
/// The library totals are cached only while these live item deltas exist.
#[derive(Default)]
pub(crate) struct SystemUnigramBumps {
    pub(crate) tokens: HashMap<Token, u64>,
    pub(crate) libraries: HashMap<u8, u32>,
    pub(crate) total: u64,
}

pub struct StoreInner<S: WriteStore> {
    pub(crate) system_bumps_live: AtomicBool,
    pub(crate) system_bumps: Mutex<SystemUnigramBumps>,
    pub(crate) has_system_items: AtomicBool,
    pub(crate) system_items: Mutex<crate::system_items::SystemItems>,
    /// Whether [`Self::removed_system_items`] holds a token.
    pub(crate) has_removed_system_items: AtomicBool,
    /// The system tokens whose item a journal REMOVE took out of its
    /// sub-index: `get_phrase_item` fails for them from then on
    /// (074a2219 `phrase_index.cpp:180-206`, `:219-236`), though the shipped
    /// chunk still holds the item and the phrase table the token.
    pub(crate) removed_system_items: Mutex<std::collections::BTreeSet<Token>>,
    /// Decode-time count memo. Declared before `db` so it is dropped
    /// first (struct fields drop in declaration order).
    pub(crate) count_cache: Mutex<Option<CountCache>>,
    pub(crate) db: Mutex<crate::session_values::SessionValues<S>>,
    pub(crate) dirty: AtomicBool,
    /// Bumped after every committed user-data write transaction, which
    /// retires any `count_cache` built against an older generation.
    ///
    /// Future raw-write paths, including legacy migration import, must call
    /// `UserStore::mark_committed_write` after commit rather than touching
    /// this counter directly. Bumping the generation alone refreshes the
    /// cache but leaves `has_user_data` as it was, and that flag is
    /// consulted *first* — an import that only bumped the generation would
    /// leave the store reporting `UserCountDelta::ZERO` for every candidate,
    /// silently and permanently, however much data it wrote.
    pub(crate) write_generation: AtomicU64,
    /// Bumped only by writes that change the phrase or pronunciation
    /// tables; the reverse index keys its cache on this, so count-only
    /// training writes never trigger a rebuild.
    pub(crate) phrase_generation: AtomicU64,
    /// Exact text-to-token rows changed. Pronunciation-only merges leave
    /// this stamp alone, so repeated text queries can retain their row.
    pub(crate) phrase_table_generation: AtomicU64,
    /// Test only: the next library replacement's transaction fails after it
    /// has seeded, as a refused store write would.
    #[cfg(test)]
    pub(crate) fail_replace: AtomicBool,
    /// First gate on the decode read path: `false` answers `count_delta` and
    /// `unigram_delta` with zero from this one atomic, taking no mutex and
    /// opening no store transaction. Recomputed at `open` and maintained only
    /// by `UserStore::mark_committed_write`.
    pub(crate) has_user_data: AtomicBool,
    /// How far the facade total the loaded libraries' stored totals make
    /// lies from the one the seeded rows make (`store_libpinyin::seed_txn`),
    /// a wrapping `guint32`. `compact` never rebuilds the facade's total
    /// (074a2219 `phrase_index.cpp:648-675`); only a library loaded again
    /// moves it, by the old sub-index's total out and the new one's in.
    pub(crate) facade_offset: std::sync::atomic::AtomicU32,
    /// Whether any library still carries its own share of that difference
    /// in [`crate::store::LIBRARY_OFFSET`]: from the load until the next
    /// `compact` (a save, the end of an import, a mask_out), which
    /// rebuilds each library's total from its items.
    pub(crate) has_library_offsets: AtomicBool,
    /// The libpinyin session metadata (optional directory, system originals).
    /// Transient sessions retain originals without a directory; others use the
    /// pin's own file shapes (`open_libpinyin`): `save()` exports the
    /// session values and writes the file set. `None` on plain
    /// `open`/`create_standalone` stores.
    pub(crate) libpinyin: Option<std::sync::Arc<crate::store_libpinyin::Target>>,
    /// The pin's own in-memory user bigram container (`Bigram::m_db`
    /// after `load_db`): the session's only copy of the user bigram, the
    /// raw `SingleGram` values every bigram operation reads and writes, so
    /// its walk order — the export's order and the saved file's layout —
    /// is the pin's. Only a libpinyin session carries one; a plain or
    /// standalone store keeps its grams in the ordered
    /// [`crate::store::BIGRAM_GRAM`] table and walks them in ascending
    /// order. A writer takes the database guard first, then this one, and
    /// bumps `write_generation` before releasing the database guard.
    pub(crate) bigram_db: Option<Mutex<oxpinyin_store::DefaultUserBigramDb>>,
}

/// Declared last on [`crate::UserStore`] so this runs after the handle `Arc` dies.
pub struct RegistryLease;

impl Drop for RegistryLease {
    fn drop(&mut self) {
        drain();
    }
}

type StoreRegistry = HashMap<PathBuf, Weak<StoreInner<DefaultStore>>>;
type StandaloneRegistry = HashSet<PathBuf>;

static OPEN_STORES: OnceLock<Mutex<StoreRegistry>> = OnceLock::new();
static OPEN_STANDALONE_STORES: OnceLock<Mutex<StandaloneRegistry>> = OnceLock::new();

pub fn registry_key(path: &Path) -> PathBuf {
    // An existing file is keyed by its fully canonical path: `.`/`..`
    // segments and any symlink, including the final component, resolve,
    // so every alias of the same file collides into one key.
    if let Ok(canonical) = path.canonicalize() {
        return canonical;
    }
    // A missing file cannot canonicalize.  Anchor the key at the
    // canonical parent (resolving `.`/`..`/symlinks in the directory
    // part) so the key computed before creation matches the canonical
    // key computed after it; a bare file name anchors at the CWD.
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => {
            let base = if parent.as_os_str().is_empty() {
                // A bare name anchors at the CWD. Canonicalize it — not just
                // `current_dir()` — so this key takes the exact same form as
                // the `parent.canonicalize()` branch and the file-exists
                // branch above. On Windows `canonicalize` returns a `\\?\`
                // verbatim path while `current_dir` does not; mixing the two
                // gave a bare name and its `./` alias different keys (and a
                // key that would not match this file once created).
                std::env::current_dir()
                    .ok()
                    .and_then(|cwd| cwd.canonicalize().ok())
            } else {
                parent.canonicalize().ok()
            };
            base.map_or_else(
                // The parent chain does not exist either; keep the key
                // absolute so a relative input still matches later
                // absolute lookups of the same not-yet-created file.
                || absolutize(path),
                |base| base.join(name),
            )
        }
        _ => absolutize(path),
    }
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| path.to_path_buf(), |cwd| cwd.join(path))
    }
}

/// Lease held by a standalone store and its clones while its path is live.
pub struct StandaloneLease {
    key: PathBuf,
}

impl Drop for StandaloneLease {
    fn drop(&mut self) {
        let Some(registry) = OPEN_STANDALONE_STORES.get() else {
            return;
        };
        let mut stores = registry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        stores.remove(&self.key);
    }
}

/// Reserve `path` for one standalone handle until its last clone drops.
///
/// `path` is keyed by [`registry_key`]: fully canonical when the file
/// exists (so aliases, including a symlinked final component, collide
/// into one lease), and anchored at the canonical parent otherwise —
/// which keeps the key stable across the file's creation, so a second
/// live [`crate::GenericUserStore::create_standalone`] observes
/// [`UserStoreError::AlreadyOpen`] regardless of which alias either
/// call used.
pub fn acquire_standalone(path: &Path) -> Option<StandaloneLease> {
    let key = registry_key(path);
    let mut stores = OPEN_STANDALONE_STORES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if stores.insert(key.clone()) {
        Some(StandaloneLease { key })
    } else {
        None
    }
}

pub fn lock_registry() -> MutexGuard<'static, StoreRegistry> {
    OPEN_STORES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn drain() {
    let Some(registry) = OPEN_STORES.get() else {
        return;
    };
    // `open` must not drop a `UserStore` while it holds this lock (it
    // only constructs the handle as the return value). `std::sync::Mutex`
    // is not reentrant; a drop-under-lock would deadlock here.
    let mut reg = registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reg.retain(|_, handle| handle.strong_count() > 0);
    if reg.is_empty() {
        reg.shrink_to_fit();
    }
}

#[cfg(test)]
pub fn contains_key(path: &Path) -> bool {
    let key = registry_key(path);
    let Some(registry) = OPEN_STORES.get() else {
        return false;
    };
    let reg = registry
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    reg.contains_key(&key)
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::registry_key;
    use crate::{GenericUserStore, UserStoreError};
    use oxpinyin_store::DefaultStore;

    fn temp_paths(tag: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir();
        let unique = format!("oxpinyin-user-registry-{tag}-{}", std::process::id());
        let canonical = dir.join(&unique);
        let dotted = dir.join(format!("./{unique}"));
        (canonical, dotted)
    }

    fn cleanup(path: &Path) {
        let _ = std::fs::remove_file(path);
        let mut lock = path.as_os_str().to_os_string();
        lock.push("-lock");
        let _ = std::fs::remove_file(Path::new(&lock));
    }

    #[test]
    fn registry_key_collapses_bare_and_dotted_aliases() {
        let (canonical, dotted) = temp_paths("key-alias");
        // Before creation: anchored at the canonical parent.
        assert_eq!(registry_key(&canonical), registry_key(&dotted));
        // After creation: fully canonical, still the same key.
        std::fs::write(&canonical, b"").unwrap();
        assert_eq!(registry_key(&canonical), registry_key(&dotted));
        cleanup(&canonical);
        // A bare relative name and its `./` form resolve against the
        // process working directory into the same key.
        let unique = format!("oxpinyin-user-registry-cwd-{}", std::process::id());
        let cwd = std::env::current_dir().unwrap();
        let bare = PathBuf::from(&unique);
        let dotted_cwd = cwd.join(format!("./{unique}"));
        assert_eq!(registry_key(&dotted_cwd), registry_key(&bare));
        // The key anchors at the *canonical* CWD (a no-op on Unix, but on
        // Windows `canonicalize` adds the `\\?\` prefix that `current_dir`
        // omits), so compare against the canonicalized form.
        assert_eq!(
            registry_key(&bare),
            cwd.canonicalize().unwrap().join(&unique)
        );
    }

    #[test]
    fn standalone_lease_survives_creation_with_dotted_alias() {
        let (canonical, dotted) = temp_paths("dotted-lease");
        cleanup(&canonical);
        // Reserve through the dotted alias while the file is absent, then
        // re-reserve through the canonical name now that it exists: both
        // must observe the same lease.
        let first = GenericUserStore::<DefaultStore>::create_standalone(&dotted).unwrap();
        assert!(matches!(
            GenericUserStore::<DefaultStore>::create_standalone(&canonical),
            Err(UserStoreError::AlreadyOpen)
        ));
        drop(first);
        cleanup(&canonical);
    }

    #[cfg(unix)]
    #[test]
    fn standalone_lease_collapses_symlinked_final_component() {
        let (canonical, _dotted) = temp_paths("symlink-lease");
        let link = canonical.with_extension("mdb.link");
        cleanup(&canonical);
        let _ = std::fs::remove_file(&link);
        let store = GenericUserStore::<DefaultStore>::create_standalone(&canonical).unwrap();
        std::os::unix::fs::symlink(&canonical, &link).unwrap();
        assert!(matches!(
            GenericUserStore::<DefaultStore>::create_standalone(&link),
            Err(UserStoreError::AlreadyOpen)
        ));
        drop(store);
        cleanup(&canonical);
        let _ = std::fs::remove_file(&link);
    }
}
