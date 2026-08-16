//! Process-global live-handle registry for [`crate::UserStore`].
//!
//! redb refuses a second write handle to a file already open in this process.
//! A second [`crate::UserStore::open`] of the same path therefore reuses the
//! live handle (shared counts and shared §4 dirty flag) instead of failing —
//! the C ABI's degrade-to-`None` would otherwise silently disable learning.
//!
//! The last [`crate::UserStore`] drop drains dead `Weak`s and shrinks an empty
//! map so a `dlclose`d cdylib leaves no heap behind.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Mutex, MutexGuard, OnceLock, Weak};

use redb::Database;

pub(crate) struct CountSnapshot {
    pub(crate) generation: u64,
    /// Owned table handles. redb 4.1.0's `ReadOnlyTable` clones the
    /// transaction guard rather than borrowing `ReadTransaction`, so this is
    /// not self-referential.
    pub(crate) unigram: redb::ReadOnlyTable<u32, u64>,
    pub(crate) unigram_total: redb::ReadOnlyTable<u8, u64>,
    pub(crate) bigram: redb::ReadOnlyTable<(u32, u32), u64>,
    pub(crate) bigram_total: redb::ReadOnlyTable<u32, u64>,
    /// Kept so the read transaction outlives the table handles.
    pub(crate) _txn: redb::ReadTransaction,
}

pub(crate) struct StoreInner {
    /// Cached decode-time read transaction. Declared before `db` so the
    /// transaction is dropped first (struct fields drop in declaration order).
    pub(crate) count_snapshot: Mutex<Option<CountSnapshot>>,
    pub(crate) db: Mutex<Database>,
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

type StoreRegistry = HashMap<PathBuf, Weak<StoreInner>>;

static OPEN_STORES: OnceLock<Mutex<StoreRegistry>> = OnceLock::new();

pub(crate) fn registry_key(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) => match parent.canonicalize() {
            Ok(base) => base.join(name),
            Err(_) => path.to_path_buf(),
        },
        _ => path.to_path_buf(),
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
