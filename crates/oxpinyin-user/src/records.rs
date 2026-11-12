//! One owned record arena over the pin's raw `(key, value)` rows.
//!
//! `ChewingLargeTable2` and `PhraseLargeTable3` both keep a DBM of raw
//! byte rows, so the user pinyin index and the user phrase table keep the
//! same `[u32 key_len][u32 value_len][key][value]` arena, one sorted
//! offset vector and one overlay for session edits. This module holds that
//! structure once; each table adds only its own value decoding.

use std::collections::BTreeMap;

/// Record header: key length and value length, both little-endian `u32`.
///
/// On the tkrzw and Berkeley DB backends, a `u32` holds every key and
/// value length that the pin's own containers can hold, so the
/// conversions to `u32` below add no check. tkrzw keeps
/// `int32_t key_size` and `int32_t value_size` in both the in-memory
/// `BabyRecord` (`tkrzw_dbm_baby.cc:37-42`, tkrzw 1.0.34) that `load_db` fills
/// (`chewing_large_table2_tkrzwdb.cpp:92-109` and
/// `phrase_large_table3_tkrzwdb.cpp:83-100` at 074a2219) and the file's
/// `TreeRecord` (`tkrzw_dbm_tree_impl.h:40-42`). The Berkeley DB load
/// copies `DBT` rows (`bdb_utils.h:43-74`), whose `size` is a `u_int32_t`.
///
/// Kyoto Cabinet sets no such bound. Its `load_db` fills a `ProtoTreeDB`
/// from a snapshot (`chewing_large_table2_kyotodb.cpp:94-105` and
/// `phrase_large_table3_kyotodb.cpp:109-120` at 074a2219).
/// `BasicDB::load_snapshot` reads `size_t` lengths (`kcdb.h:2370-2389`,
/// Kyoto Cabinet 1.2.80), and `ProtoTreeDB` keeps `std::string` keys and
/// values (`kcprotodb.h:1351`, `kcutil.h:89`). On that backend, a key or
/// value of 4 GiB or more affects only the read of that record: its
/// length saturates to `u32::MAX`, and every other record keeps its own
/// offset.
///
/// The pin sets no limit on the total size, so the offsets are `usize`.
const HEADER: usize = 8;

/// The pin's rows in one owned arena.
///
/// The load walks the DBM once, appends each key and value in the walk's
/// (ascending) order, and records a sorted offset vector. A session edit
/// writes only the affected record into the overlay, which iteration
/// merges in key order.
#[derive(Clone, Debug, Default)]
pub(crate) struct RecordArena {
    /// `[u32 key_len][u32 value_len][key][value]` records.
    arena: Vec<u8>,
    /// Sorted offsets into `arena`, one per record.
    offsets: Vec<usize>,
    /// Records a session edited or created, by key, in key order.
    overlay: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl RecordArena {
    pub(crate) const fn new() -> Self {
        Self {
            arena: Vec::new(),
            offsets: Vec::new(),
            overlay: BTreeMap::new(),
        }
    }

    /// Appends one DBM record. Every producer appends a key at most once:
    /// the user index files are key-unique containers and the in-memory
    /// builders deduplicate by key. Order is not guaranteed; a build or walk
    /// that did not arrive in key order calls [`Self::finish_load`] before
    /// the arena is read.
    pub(crate) fn push_record(&mut self, key: &[u8], value: &[u8]) {
        append_record(&mut self.arena, &mut self.offsets, key, value);
    }

    /// Sorts the offset vector after a walk or a builder that did not append
    /// in key order: an in-memory builder such as `UserIndex::from_entries`
    /// may pass rows in any order, so the repair stays.
    pub(crate) fn finish_load(&mut self) {
        let arena = &self.arena;
        self.offsets
            .sort_by(|left, right| key_at(arena, *left).cmp(key_at(arena, *right)));
    }

    /// Records a session edit of `key`, replacing any earlier one.
    pub(crate) fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.overlay.insert(key, value);
    }

    /// The value stored under `key`, arena or overlay.
    pub(crate) fn record(&self, key: &[u8]) -> Option<&[u8]> {
        if let Some(value) = self.overlay.get(key) {
            return Some(value);
        }
        self.arena_record(key)
    }

    fn arena_record(&self, key: &[u8]) -> Option<&[u8]> {
        let at = self
            .offsets
            .binary_search_by(|offset| key_at(&self.arena, *offset).cmp(key))
            .ok()?;
        Some(value_at(&self.arena, self.offsets[at]))
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.overlay.contains_key(key) || self.arena_record(key).is_some()
    }

    /// Visits every record in ascending key order, arena and overlay
    /// merged, without materialising a row vector.
    pub(crate) fn for_each_record<E>(
        &self,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut overlay = self.overlay.iter().peekable();
        for offset in &self.offsets {
            let key = key_at(&self.arena, *offset);
            while let Some((overlay_key, overlay_value)) = overlay.peek() {
                if overlay_key.as_slice() < key {
                    visit(overlay_key, overlay_value)?;
                    overlay.next();
                } else {
                    break;
                }
            }
            if let Some((overlay_key, overlay_value)) = overlay.peek()
                && overlay_key.as_slice() == key
            {
                visit(overlay_key, overlay_value)?;
                overlay.next();
            } else {
                visit(key, value_at(&self.arena, *offset))?;
            }
        }
        for (key, value) in overlay {
            visit(key, value)?;
        }
        Ok(())
    }

    /// Rebuilds the arena from every record it holds, in key order, with
    /// `keep` appending the value to store for each one to the arena, then
    /// installs the rebuilt arena and offsets with the overlay cleared.
    ///
    /// The value is appended straight into the new arena, so a rebuild
    /// copies each kept value once, as the per-record append does.
    pub(crate) fn rebuild(&mut self, mut keep: impl FnMut(&[u8], &[u8], &mut Vec<u8>)) {
        let mut arena = Vec::with_capacity(self.arena.len());
        let mut offsets = Vec::with_capacity(self.offsets.len() + self.overlay.len());
        if let Err(infallible) = self.for_each_record::<std::convert::Infallible>(|key, record| {
            let offset = arena.len();
            arena.extend_from_slice(&u32::try_from(key.len()).unwrap_or(u32::MAX).to_le_bytes());
            arena.extend_from_slice(&0_u32.to_le_bytes());
            arena.extend_from_slice(key);
            let value_start = arena.len();
            keep(key, record, &mut arena);
            let value_len = u32::try_from(arena.len() - value_start).unwrap_or(u32::MAX);
            arena[offset + 4..offset + 8].copy_from_slice(&value_len.to_le_bytes());
            offsets.push(offset);
            Ok(())
        }) {
            match infallible {}
        }
        self.arena = arena;
        self.offsets = offsets;
        self.overlay.clear();
    }

    /// Whether the arena, not the overlay, holds `key`.
    fn contains_arena_key(&self, key: &[u8]) -> bool {
        self.offsets
            .binary_search_by(|offset| key_at(&self.arena, *offset).cmp(key))
            .is_ok()
    }

    /// The keys extending `prefix`, arena and overlay merged in order.
    pub(crate) fn keys_starting_with(&self, prefix: &[u8]) -> Vec<Vec<u8>> {
        let mut keys: Vec<Vec<u8>> = Vec::new();
        let start = self
            .offsets
            .partition_point(|offset| key_at(&self.arena, *offset) < prefix);
        for offset in &self.offsets[start..] {
            let key = key_at(&self.arena, *offset);
            if !key.starts_with(prefix) {
                break;
            }
            keys.push(key.to_vec());
        }
        for (key, _) in self.overlay.range(prefix.to_vec()..) {
            if !key.starts_with(prefix) {
                break;
            }
            if !self.contains_arena_key(key) {
                keys.push(key.clone());
            }
        }
        keys.sort();
        keys
    }

    /// Every key in ascending order, arena and overlay merged.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<Vec<u8>> {
        let mut keys: Vec<Vec<u8>> = self.arena_keys().map(<[u8]>::to_vec).collect();
        for key in self.overlay.keys() {
            if !self.contains_arena_key(key) {
                keys.push(key.clone());
            }
        }
        keys.sort();
        keys
    }

    /// Every arena key in offset order, overlay excluded.
    #[cfg(test)]
    pub(crate) fn arena_keys(&self) -> impl Iterator<Item = &[u8]> {
        self.offsets
            .iter()
            .map(|offset| key_at(&self.arena, *offset))
    }
}

/// Appends one `[u32 key_len][u32 value_len][key][value]` record and its
/// offset.
fn append_record(arena: &mut Vec<u8>, offsets: &mut Vec<usize>, key: &[u8], value: &[u8]) {
    let offset = arena.len();
    arena.extend_from_slice(&u32::try_from(key.len()).unwrap_or(u32::MAX).to_le_bytes());
    arena.extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_le_bytes());
    arena.extend_from_slice(key);
    arena.extend_from_slice(value);
    offsets.push(offset);
}

fn key_at(arena: &[u8], at: usize) -> &[u8] {
    let len = u32::from_le_bytes([arena[at], arena[at + 1], arena[at + 2], arena[at + 3]]) as usize;
    &arena[at + HEADER..at + HEADER + len]
}

fn value_at(arena: &[u8], at: usize) -> &[u8] {
    let key_len =
        u32::from_le_bytes([arena[at], arena[at + 1], arena[at + 2], arena[at + 3]]) as usize;
    let value_len =
        u32::from_le_bytes([arena[at + 4], arena[at + 5], arena[at + 6], arena[at + 7]]) as usize;
    &arena[at + HEADER + key_len..at + HEADER + key_len + value_len]
}
