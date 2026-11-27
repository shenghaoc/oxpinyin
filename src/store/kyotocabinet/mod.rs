//! Kyoto Cabinet storage for raw keys and values. [`KcStore`] opens the tree or hash
//! class that the caller requests. [`RawReadStore::scan_raw`] and
//! [`RawReadStore::range_raw`] visit keys in ascending byte order for both classes. For
//! a hash container, they collect the rows and sort them by key.
//! [`RawReadStore::native_keys`] keeps the container's native iteration order.
//!
//! Writes are individual native operations: [`WriteStore::set_raw`] is one
//! `set`, [`WriteStore::remove_raw`] one `remove`. The pin has no
//! multi-record operation, so neither has this backend. The profile save
//! (`write_user_index`) applies the pin's snapshot sequence: it unlinks the
//! target and writes the in-memory tree with `dump_snapshot`, and no
//! synchronize call follows.
//!
//! The C caller serializes access to a context and its handles. The safe store uses a
//! native bridge for database calls.
//!
//! [`RawReadStore::scan_raw`]: crate::store::RawReadStore::scan_raw
//! [`RawReadStore::range_raw`]: crate::store::RawReadStore::range_raw
//! [`RawReadStore::native_keys`]: crate::store::RawReadStore::native_keys

// TreeDB uses LEXICALCOMP without rcomp. The explicit class selection avoids PolyDB
// suffix dispatch (kclangc.h:312–320). The user snapshots use
// chewing_large_table2_kyotodb.cpp:124–130, phrase_large_table3_kyotodb.cpp:139–145 and
// ngram_kyotodb.cpp:82–101; hash attachment is ngram_kyotodb.cpp:110–120.

mod bridge;
mod ffi;

use std::ops::Bound;
use std::path::Path;

use crate::store::common::in_bounds;
use crate::store::{Flow, StoreError, WriteStore};

use ffi::{Db, DbType};

/// A raw-key Kyoto Cabinet tree or hash store implementing the read and write traits.
pub struct KcStore {
    db: Db,
}

/// Attaches a system table with the pin's native open mode
/// (`chewing_large_table2_kyotodb.cpp:77-90`,
/// `phrase_large_table3_kyotodb.cpp:92-105`, `punct_table_kyotodb.cpp:52-65`,
/// `ngram_kyotodb.cpp:110-120` at 074a2219). A successful attachment
/// synchronizes before closing.
pub(crate) fn open_system(
    path: &Path,
    kind: crate::store::DbmKind,
    access: crate::store::AccessMode,
) -> Result<Option<KcStore>, StoreError> {
    let kind = match kind {
        crate::store::DbmKind::Tree => DbType::Tree,
        crate::store::DbmKind::Hash => DbType::Hash,
    };
    let read_only = access == crate::store::AccessMode::ReadOnly;
    let (db, opened) = Db::open_native(path, kind, read_only, !read_only)?;
    Ok(opened.then(|| KcStore {
        db: db.with_reset_sync(),
    }))
}

impl KcStore {
    /// Allocates one native tree or hash object without opening a file.
    ///
    /// # Errors
    /// Returns an unsupported Kyoto Cabinet runtime version.
    #[doc(hidden)]
    pub fn allocate_native(hash: bool, read_only: bool) -> Result<Self, StoreError> {
        let kind = if hash { DbType::Hash } else { DbType::Tree };
        Ok(Self {
            db: Db::new_unopened(kind, read_only)?,
        })
    }

    /// Attempts one native open on the same object, retaining it on failure.
    ///
    /// # Errors
    /// Returns failures that occur before the native open.
    #[doc(hidden)]
    pub fn try_open_native(&mut self, path: &Path, create: bool) -> Result<bool, StoreError> {
        let flags = if self.db.is_read_only() {
            ffi::OREADER
        } else {
            ffi::OREADER | ffi::OWRITER
        };
        let flags = if create { flags | ffi::OCREATE } else { flags };
        self.db.open_mode(path, flags)
    }

    /// Every row of the file, sorted by key. Not generic, so the many
    /// visitors share one copy of the iteration and the sort.
    #[inline(never)]
    fn sorted_rows(&self) -> Result<Vec<ffi::Record>, StoreError> {
        let mut rows = self.db.records()?;
        rows.sort_unstable_by(|a, b| a.key().cmp(b.key()));
        Ok(rows)
    }

    /// [`crate::store::RawReadStore::scan_raw`] on an ordered class: one cursor,
    /// positioned at `lo` and stepped until the first key past `hi`, so the
    /// walk costs the rows it returns and one more — libpinyin's own
    /// `search_suggestion` walk, `cursor->jump(key)` and `step()`
    /// (`phrase_large_table3_kyotodb.cpp:211-244` at 074a2219).
    ///
    /// `TreeDB` and `ProtoTreeDB` order keys with `LEXICALCOMP`, byte-wise
    /// with the shorter key first on a shared prefix, which is the order of
    /// `[u8]`; libpinyin never tunes the comparator. So this visits the
    /// same rows, in the same order, as sorting the whole file would.
    ///
    /// The visitor runs over the cursor's copy of each row after
    /// `kccuraccept` has returned, outside the record lock, so it may call
    /// back into the store.
    fn walk_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, StoreError>,
    ) -> Result<(), StoreError> {
        let mut cursor = self.db.cursor();
        let positioned = match lo {
            // `kccurjumpkey("")` is the first record on an ordered class,
            // but the no-key jump says so directly.
            Bound::Unbounded => cursor.jump_first()?,
            Bound::Included(key) | Bound::Excluded(key) => cursor.jump_to(key)?,
        };
        if !positioned {
            return Ok(());
        }
        while cursor.next()? {
            let key = cursor.key();
            if matches!(lo, Bound::Excluded(bound) if key == bound) {
                continue;
            }
            if !in_bounds(key, Bound::Unbounded, hi) {
                return Ok(());
            }
            if visit(key, cursor.value())?.is_break() {
                return Ok(());
            }
        }
        Ok(())
    }
}

impl crate::store::RawReadStore for KcStore {
    fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Tree, true, false)?,
        })
    }

    fn get_raw_bounded(&self, key: &[u8], value: &mut [u8]) -> Result<Option<usize>, StoreError> {
        self.db.get_bounded(key, value)
    }

    fn get_raw(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, crate::store::StoreError> {
        #[cfg(test)]
        if self.db.is_read_only() {
            crate::store::read_faults::point_read(key)?;
        }
        self.db.get(key, value)
    }

    fn get_bigram_raw(
        &self,
        key: &[u8],
        value: &mut Vec<u8>,
    ) -> Result<bool, crate::store::StoreError> {
        // A failed size check is the pin's silent miss
        // (`ngram_kyotodb.cpp:131-134` at 074a2219).
        #[cfg(test)]
        if self.db.is_read_only() && crate::store::read_faults::point_read(key).is_err() {
            value.clear();
            return Ok(false);
        }
        self.db.get_bigram(key, value)
    }

    fn get_pinyin_row_raw(
        &self,
        key: &[u8],
        value: &mut Vec<u8>,
    ) -> Result<bool, crate::store::StoreError> {
        value.clear();
        #[cfg(test)]
        if self.db.is_read_only() && crate::store::read_faults::point_read(key).is_err() {
            return Ok(false);
        }
        // `-1 == vsiz`: a miss and a failed check are one answer
        // (`chewing_large_table2_kyotodb.cpp:160-163` at 074a2219).
        let Some(size) = self.db.check_size(key).unwrap_or(None) else {
            return Ok(false);
        };
        // `0 == vsiz`: the key continues the search and no body is read
        // (`:166-168`).
        if size == 0 {
            return Ok(true);
        }
        #[cfg(test)]
        if self.db.is_read_only() && crate::store::read_faults::point_read(key).is_err() {
            return Err(crate::store::StoreError::ReadAfterCheck);
        }
        // `check_result(vsiz == m_db->get(...))` (`:170-174`).
        match self.db.get(key, value) {
            Ok(true) if value.len() == size => Ok(true),
            _ => Err(crate::store::StoreError::ReadAfterCheck),
        }
    }

    fn contains_raw(&self, key: &[u8]) -> Result<bool, crate::store::StoreError> {
        #[cfg(test)]
        if self.db.is_read_only() {
            crate::store::read_faults::point_read(key)?;
        }
        self.db.check(key)
    }

    fn scan_raw(
        &self,
        lo: Bound<&[u8]>,
        hi: Bound<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<Flow, crate::store::StoreError>,
    ) -> Result<(), crate::store::StoreError> {
        #[cfg(test)]
        let visit = {
            crate::store::read_faults::scan_start(self.db.is_read_only(), lo)?;
            crate::store::read_faults::scan_visitor(self.db.is_read_only(), visit)
        };
        let mut visit = visit;
        if self.db.is_ordered() {
            return self.walk_raw(lo, hi, visit);
        }
        // The contract is ascending key order, but a cursor over the
        // unordered containers (HashDB, and the StashDB the user bigram
        // loads into) walks in bucket order — jump positions at a hash
        // slot, not at the first key at or above a lower bound. Collect
        // every row, sort by key, and apply the bounds here. Each row is
        // dropped once visited, so a visitor that copies rows out never
        // holds two copies of the file.
        for record in self.sorted_rows()? {
            let (key, value) = (record.key(), record.value());
            if !in_bounds(key, lo, Bound::Unbounded) {
                // Below the lower bound, which an `Excluded` bound's
                // equality test alone did not catch.
                continue;
            }
            if !in_bounds(key, Bound::Unbounded, hi) {
                return Ok(());
            }
            if visit(key, value)?.is_break() {
                return Ok(());
            }
        }
        Ok(())
    }

    /// The library's own O(1) record count — over the whole file, which
    /// on this backend is exactly the raw keyspace.
    #[cfg(test)]
    fn count_raw(&self) -> Result<u64, crate::store::StoreError> {
        self.db.count()
    }

    /// `Bigram::get_all_items`' `iterate` walk (`ngram_kyotodb.cpp:186-196`),
    /// its one native call: Kyoto Cabinet's own order, which for `HashDB`
    /// is bucket order.
    fn native_keys(&self, keys: &mut Vec<Vec<u8>>) -> Result<(), crate::store::StoreError> {
        self.db.raw_keys(keys)
    }

    fn open_hash_read_only(path: &Path) -> Result<Self, crate::store::StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, true, false)?,
        })
    }

    fn open_user_index(path: &Path) -> Result<Self, crate::store::StoreError> {
        // 074a2219 chewing_large_table2_kyotodb.cpp:94-105 and
        // phrase_large_table3_kyotodb.cpp:109-121: ProtoTreeDB snapshots.
        // A snapshot that fails part way keeps the records it set before
        // the failure (`BasicDB::load_snapshot` sets record by record,
        // `kcdb.h:2339-2418`), and the pin keeps that container; init
        // ignores the `load_db` answer.
        let db = Db::open_proto_tree()?.with_reset_sync();
        let _ = db.load_snapshot(path);
        Ok(Self {
            db: db.into_read_only(),
        })
    }
}

impl WriteStore for KcStore {
    fn create(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Tree, false, true)?,
        })
    }

    fn create_hash(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open(path, DbType::Hash, false, true)?,
        })
    }

    fn set_raw(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db.set(key, value)
    }

    fn remove_raw(&self, key: &[u8]) -> Result<bool, StoreError> {
        self.db.remove(key)
    }

    fn is_read_only(&self) -> bool {
        self.db.is_read_only()
    }

    fn rewrite_raw(&self, rewrite: &mut crate::store::RecordRewrite<'_>) -> Result<(), StoreError> {
        self.db.rewrite(rewrite)
    }

    fn save_user_index(&self, path: &Path) -> Result<(), StoreError> {
        crate::store::remove_if_present(path)?;
        self.db.dump_snapshot(path)
    }

    /// The pin's own in-memory ordered container, `ProtoTreeDB` (what its
    /// user indexes live in between loads and saves,
    /// `chewing_large_table2_kyotodb.cpp:124-130`): no file, no WAL, no
    /// lock file.
    fn create_in_memory() -> Result<Self, StoreError> {
        Ok(Self {
            db: Db::open_proto_tree()?.with_reset_sync(),
        })
    }

    fn write_user_index(
        path: &Path,
        records: impl FnOnce(&mut crate::store::RecordSink<'_>) -> Result<(), StoreError>,
    ) -> Result<(), StoreError> {
        // The pin unlinks then dump_snapshot's the in-memory tree
        // (chewing_large_table2_kyotodb.cpp:124-130,
        // phrase_large_table3_kyotodb.cpp:139-145). Persistence passes
        // the staged .tmp path, so the previous profile remains intact.
        // The records are set into a `ProtoTreeDB`, the pin's container,
        // as they stream in; the file is the snapshot, never a `TreeDB`.
        crate::store::remove_if_present(path)?;
        let db = Db::open_proto_tree()?;
        records(&mut |key, value| db.set(key, value))?;
        db.dump_snapshot(path)
    }

    /// `m_db->synchronize()` (`flexible_ngram_kyotodb.h:86-92`), made on a
    /// read-only container too, as the pin's `reset()` makes it.
    fn flush(&self) -> Result<(), StoreError> {
        self.db.sync(false)
    }
}

// ── the user bigram's in-memory container ─────────────────────────

/// libpinyin's Kyoto Cabinet user bigram as `Bigram::load_db` leaves it:
/// an in-memory `StashDB` filled by `load_snapshot`
/// (`ngram_kyotodb.cpp:54-64`), so its bucket walk is the pin's.
pub struct KcUserBigramDb {
    db: Db,
}

impl crate::store::UserBigramDb for KcUserBigramDb {
    fn empty() -> Result<Self, StoreError> {
        // `m_db = new StashDB; m_db->open("-", …)`; `reset()` synchronizes
        // before it closes (`ngram_kyotodb.cpp:43-61`). The pin's `return
        // false` after that open cannot be reached: a fresh `StashDB` refuses
        // an open only when it is already open (`kcstashdb.h:640-661`), and
        // its bucket allocation throws `std::bad_alloc` (`kcutil.cc:165`),
        // which no pin frame catches, so the process ends. A refused open is
        // that impossible case, and it ends here too.
        let (db, opened) = Db::open_native(Path::new("-"), DbType::Stash, false, true)?;
        if !opened {
            std::process::abort();
        }
        Ok(Self {
            db: db.with_reset_sync(),
        })
    }

    fn load_db(path: &Path) -> Result<Self, StoreError> {
        // Then `load_snapshot(dbfile)`; a snapshot that does not load
        // leaves the stash as far as it got — empty for an absent file.
        let this = Self::empty()?;
        let _ = this.db.load_snapshot(path);
        Ok(this)
    }

    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, StoreError> {
        self.db.get(key, value)
    }

    fn store(&self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db.set(key, value)
    }

    fn remove(&self, key: &[u8]) -> Result<(), StoreError> {
        self.db.remove(key).map(drop)
    }

    fn keys(&self) -> Result<Vec<Option<[u8; 4]>>, StoreError> {
        // `get_all_items`' own call: `iterate` with a key-collecting
        // visitor (`ngram_kyotodb.cpp:163-196`), the stash's bucket order.
        self.db.keys()
    }

    fn is_empty(&self) -> Result<bool, StoreError> {
        Ok(self.db.count()? == 0)
    }

    fn save_db(&self, path: &Path) -> Result<(), StoreError> {
        save_bigram_db(&self.db, path)
    }
}

fn save_bigram_db(db: &Db, path: &Path) -> Result<(), StoreError> {
    // `Bigram::save_db` (`ngram_kyotodb.cpp:82-101`): unlink, then
    // `dump_snapshot`.
    crate::store::remove_if_present(path)?;
    db.dump_snapshot(path)
}

#[cfg(test)]
impl KcStore {
    pub(crate) fn save_bigram_db(&self, path: &Path) -> Result<(), StoreError> {
        save_bigram_db(&self.db, path)
    }
}

#[cfg(test)]
mod tests;
