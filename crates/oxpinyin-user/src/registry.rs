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
            match base {
                Some(base) => base.join(name),
                // The parent chain does not exist either; keep the key
                // absolute so a relative input still matches later
                // absolute lookups of the same not-yet-created file.
                None => absolutize(path),
            }
        }
        _ => absolutize(path),
    }
}

fn absolutize(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
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
/// `path` is keyed by [`registry_key`]: fully canonical when the file
/// exists (so aliases, including a symlinked final component, collide
/// into one lease), and anchored at the canonical parent otherwise —
/// which keeps the key stable across the file's creation, so a second
/// live [`crate::GenericUserStore::create_standalone`] observes
/// [`UserStoreError::AlreadyOpen`] regardless of which alias either
/// call used.
pub(crate) fn acquire_standalone(path: &Path) -> Option<StandaloneLease> {
    let key = registry_key(path);
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
