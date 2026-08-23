//! Process-global live-handle registry for [`crate::UserStore`].
//!
//! redb refuses a second write handle to a file already open in this process.
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

use oxpinyin_store::{DefaultStore, OrderedStore};

pub(crate) struct CountSnapshot<S: OrderedStore> {
    pub(crate) generation: u64,
    pub(crate) snap: S::ReadSnapshot,
}

pub(crate) struct StoreInner<S: OrderedStore> {
    /// Cached decode-time read snapshot. Declared before `db` so the
    /// snapshot is dropped first (struct fields drop in declaration order).
    pub(crate) count_snapshot: Mutex<Option<CountSnapshot<S>>>,
    pub(crate) db: Mutex<S>,
    pub(crate) dirty: AtomicBool,
    /// Bumped after every committed user-data write transaction, which
    /// retires any `count_snapshot` cached against an older generation.
    ///
    /// Future raw-write paths, including legacy migration import, must call
    /// `UserStore::mark_committed_write` after commit rather than touching
    /// this counter directly. Bumping the generation alone refreshes the
    /// snapshot but leaves `has_user_data` as it was, and that flag is
    /// consulted *first* — an import that only bumped the generation would
    /// leave the store reporting `UserCountDelta::ZERO` for every candidate,
    /// silently and permanently, however much data it wrote.
    pub(crate) write_generation: AtomicU64,
    /// First gate on the decode read path: `false` answers `count_delta` and
    /// `unigram_delta` with zero from this one atomic, taking no mutex and
    /// opening no redb transaction. Recomputed at `open` and maintained only
    /// by `UserStore::mark_committed_write`.
    pub(crate) has_user_data: AtomicBool,
}

/// Declared last on [`crate::UserStore`] so this runs after the handle `Arc` dies.
pub(crate) struct RegistryLease;

impl Drop for RegistryLease {
    fn drop(&mut self) {
        drain();
    }
}

type StoreRegistry = HashMap<PathBuf, Weak<StoreInner<DefaultStore>>>;
type StandaloneRegistry = HashSet<PathBuf>;

static OPEN_STORES: OnceLock<Mutex<StoreRegistry>> = OnceLock::new();
static OPEN_STANDALONE_STORES: OnceLock<Mutex<StandaloneRegistry>> = OnceLock::new();

pub(crate) fn registry_key(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => match parent.canonicalize() {
            Ok(base) => base.join(name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
    }
}

/// Lease held by a standalone store and its clones while its path is live.
pub(crate) struct StandaloneLease {
    key: PathBuf,
}

impl Drop for StandaloneLease {
    fn drop(&mut self) {
        let Some(registry) = OPEN_STANDALONE_STORES.get() else {
            return;
        };
        let mut stores = registry
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        stores.remove(&self.key);
    }
}

/// Reserve `path` for one standalone handle until its last clone drops.
///
/// An existing file is keyed by its fully canonicalized path, so equivalent
/// aliases — including a symlinked final component — collide into one lease
/// and a second [`crate::GenericUserStore::create_standalone`] observes
/// [`UserStoreError::AlreadyOpen`] instead of a second backend handle. A
/// missing file cannot be resolved that way; it falls back to
/// [`registry_key`], which canonicalizes the parent directory and joins the
/// file name.
pub(crate) fn acquire_standalone(path: &Path) -> Option<StandaloneLease> {
    let resolved = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let key = registry_key(&resolved);
    let mut stores = OPEN_STANDALONE_STORES
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if stores.insert(key.clone()) {
        Some(StandaloneLease { key })
    } else {
        None
    }
}

pub(crate) fn lock_registry() -> MutexGuard<'static, StoreRegistry> {
    OPEN_STORES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reg.retain(|_, handle| handle.strong_count() > 0);
    if reg.is_empty() {
        reg.shrink_to_fit();
    }
}

#[cfg(test)]
pub(crate) fn contains_key(path: &Path) -> bool {
    let key = registry_key(path);
    let Some(registry) = OPEN_STORES.get() else {
        return false;
    };
    let reg = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    reg.contains_key(&key)
}
