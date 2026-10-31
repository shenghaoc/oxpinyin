//! The user pinyin index as the pin's own records.
//!
//! `ChewingLargeTable2` holds the user index as one DBM keyed by packed
//! `ChewingKey` runs; each value is a sorted `PinyinIndexItem2<L>` array.
//! This module keeps that table in one owned arena: the load walks the
//! DBM once, appends each key and value in the walk's (ascending) order,
//! and records a sorted offset vector. `indexed()` and the save read the
//! stored records; a session edit writes only the affected record into an
//! overlay, which the save merges in key order.
//!
//! The overlay is still the pin's table. `Entry` applies the same
//! `ChewingTableEntry::add_index` / `remove_index` / `mask_out` edits, and
//! the Kyoto Cabinet and tkrzw mask walk keeps its per-length entry
//! (`Slot`) because those backends store the whole reused chunk back and
//! can carry the previous record's tail (#785). Berkeley DB uses a fresh
//! chunk per record, so it masks each record independently.
//!
//! Keys the file carried are kept verbatim, including empty prefix
//! markers and malformed rows: the pin's `load_db` copies the table and
//! `save_db` copies it back.

use std::collections::{BTreeMap, BTreeSet};

use oxpinyin_core::ChewingKey;
use oxpinyin_data::row_format::pinyin_index::{
    encode_complete_key, encode_incomplete_key, encode_item, item2_stride,
};

use crate::phrase::MAX_PHRASE_LENGTH;

/// Record header: key length and value length, both little-endian `u32`.
const HEADER: usize = 8;

/// What one key length's table entry holds between operations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Nothing: a fresh entry, or a suggestion's `set_size(0)`.
    #[default]
    Empty,
    /// The record stored under this key, as the table holds it now.
    Record(Vec<u8>),
    /// The chunk a mask walk left.
    Bytes(Vec<u8>),
}

/// The entry operations on one record's bytes, over `len` syllables.
struct Entry<'a> {
    bytes: &'a mut Vec<u8>,
    len: usize,
    stride: usize,
}

impl<'a> Entry<'a> {
    fn new(bytes: &'a mut Vec<u8>, len: usize) -> Self {
        Self {
            bytes,
            len,
            stride: item2_stride(len),
        }
    }

    fn count(&self) -> usize {
        self.bytes.len() / self.stride
    }

    fn token(&self, index: usize) -> u32 {
        let base = index * self.stride;
        u32::from_le_bytes([
            self.bytes[base],
            self.bytes[base + 1],
            self.bytes[base + 2],
            self.bytes[base + 3],
        ])
    }

    /// Two standard partition searches in exact key order.
    /// Foreign or mask-carried unpartitioned records retain their bytes and
    /// use std's unspecified search result.
    fn equal_range(&self, value: &[ChewingKey]) -> (usize, usize) {
        if value.len() != self.len {
            return (0, 0);
        }
        match self.stride {
            8 => search_records::<8>(self.bytes, value),
            12 => search_records::<12>(self.bytes, value),
            16 => search_records::<16>(self.bytes, value),
            20 => search_records::<20>(self.bytes, value),
            24 => search_records::<24>(self.bytes, value),
            28 => search_records::<28>(self.bytes, value),
            32 => search_records::<32>(self.bytes, value),
            36 => search_records::<36>(self.bytes, value),
            _ => (0, 0),
        }
    }

    /// `ChewingTableEntry::add_index`: `false` when the item exists.
    fn add_index(&mut self, keys: &[ChewingKey], token: u32) -> bool {
        let (first, last) = self.equal_range(keys);
        let mut at = first;
        while at != last {
            let present = self.token(at);
            if present == token {
                return false;
            }
            if present > token {
                break;
            }
            at += 1;
        }
        let item = encode_item(token, keys);
        let offset = at * self.stride;
        self.bytes.splice(offset..offset, item);
        true
    }

    /// `ChewingTableEntry::remove_index`: `false` when the item is absent.
    fn remove_index(&mut self, keys: &[ChewingKey], token: u32) -> bool {
        let (first, last) = self.equal_range(keys);
        for at in first..last {
            if self.token(at) == token {
                let offset = at * self.stride;
                self.bytes.drain(offset..offset + self.stride);
                return true;
            }
        }
        false
    }

    /// `ChewingTableEntry::mask_out`.
    fn mask_out(&mut self, mask: u32, value: u32) {
        let mut at = 0;
        while at < self.count() {
            if (self.token(at) & mask) == value {
                let offset = at * self.stride;
                self.bytes.drain(offset..offset + self.stride);
            } else {
                at += 1;
            }
        }
    }
}

/// Compare packed keys in place, without allocating or decoding a row.
/// `pinyin_exact_compare2` (pinyin_phrase3.h:33-65 at 074a2219) compares
/// all initials, then each middle/final pair, then all tones. Neither the
/// token nor record padding participates.
fn compare_record(record: &[u8], value: &[ChewingKey]) -> std::cmp::Ordering {
    let (words, _) = record[4..].as_chunks::<2>();
    let keys = || {
        words
            .iter()
            .take(value.len())
            .map(|word| ChewingKey::from_packed(u16::from_le_bytes(*word)))
    };
    keys()
        .map(|key| key.initial)
        .cmp(value.iter().map(|key| key.initial))
        .then_with(|| {
            keys()
                .map(|key| (key.middle, key.final_))
                .cmp(value.iter().map(|key| (key.middle, key.final_)))
        })
        .then_with(|| {
            keys()
                .map(|key| key.tone)
                .cmp(value.iter().map(|key| key.tone))
        })
}

fn search_records<const N: usize>(bytes: &[u8], value: &[ChewingKey]) -> (usize, usize) {
    let (records, _) = bytes.as_chunks::<N>();
    let first = records.partition_point(|record| compare_record(record, value).is_lt());
    let last =
        first + records[first..].partition_point(|record| !compare_record(record, value).is_gt());
    (first, last)
}

/// The two keys `ChewingLargeTable2::add_index` / `remove_index` address,
/// incomplete space first.
fn index_keys(keys: &[ChewingKey]) -> [Vec<u8>; 2] {
    [encode_incomplete_key(keys), encode_complete_key(keys)]
}

/// The key `ChewingLargeTable2::search` / `search_suggestion` probe.
fn query_key(keys: &[ChewingKey]) -> Vec<u8> {
    // `contains_incomplete_pinyin` (`pinyin_phrase3.h:146`).
    if keys.iter().any(|key| key.middle == 0 && key.final_ == 0) {
        encode_incomplete_key(keys)
    } else {
        encode_complete_key(keys)
    }
}

/// The user pinyin index, owned in one arena.
#[derive(Clone, Debug, Default)]
pub(crate) struct UserIndex {
    /// `[u32 key_len][u32 value_len][key][value]` records.
    arena: Vec<u8>,
    /// Sorted offsets into `arena`, one per record.
    offsets: Vec<u32>,
    /// Records a session edited or created, by key, in key order.
    overlay: BTreeMap<Vec<u8>, Vec<u8>>,
    /// The pin's per-length entry state for the KC/tkrzw mask walk.
    slots: [Slot; MAX_PHRASE_LENGTH + 1],
}

impl UserIndex {
    pub(crate) const fn new() -> Self {
        Self {
            arena: Vec::new(),
            offsets: Vec::new(),
            overlay: BTreeMap::new(),
            slots: [const { Slot::Empty }; MAX_PHRASE_LENGTH + 1],
        }
    }

    /// Builds an index from value-model rows (the standalone store's
    /// derived table).
    pub(crate) fn from_entries(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        let mut index = Self::new();
        for (key, value) in &entries {
            index.push_record(key, value);
        }
        index.finish_load();
        index
    }

    /// Appends one DBM record. The walk is ascending, so offsets stay
    /// sorted; `finish_load` repairs a walk that is not.
    pub(crate) fn push_record(&mut self, key: &[u8], value: &[u8]) {
        let offset = u32::try_from(self.arena.len()).unwrap_or(u32::MAX);
        self.arena
            .extend_from_slice(&u32::try_from(key.len()).unwrap_or(u32::MAX).to_le_bytes());
        self.arena
            .extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_le_bytes());
        self.arena.extend_from_slice(key);
        self.arena.extend_from_slice(value);
        self.offsets.push(offset);
    }

    /// Sorts the offset vector after a walk that did not arrive in key
    /// order.
    pub(crate) fn finish_load(&mut self) {
        let arena = &self.arena;
        self.offsets
            .sort_by(|left, right| key_at(arena, *left).cmp(key_at(arena, *right)));
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

    /// Whether the exact bytes `key` names a table row. Used by the
    /// over-long extension gate's `DB_SET` probe.
    pub(crate) fn contains_raw_key(&self, key: &[u8]) -> bool {
        self.contains_key(key)
    }

    /// Whether any record for `token` under `packed` exists. The pin's
    /// `add_index` writes both projections, so either witnesses membership;
    /// a foreign file may carry only one.
    pub(crate) fn contains_reading(&self, token: u32, packed: &[u16]) -> bool {
        if packed.is_empty() || packed.len() > MAX_PHRASE_LENGTH {
            return false;
        }
        let keys: Vec<ChewingKey> = packed
            .iter()
            .copied()
            .map(ChewingKey::from_packed)
            .collect();
        for key in index_keys(&keys) {
            if let Some(value) = self.record(&key)
                && value_has_reading(value, keys.len(), token, packed)
            {
                return true;
            }
        }
        false
    }

    /// `ChewingLargeTable2::add_index`: both projections, prefix markers and
    /// the entry state. Answers whether any item was inserted.
    pub(crate) fn add(&mut self, keys: &[ChewingKey], token: u32) -> bool {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return false;
        }
        let mut inserted = false;
        for key in index_keys(keys) {
            if !self.contains_key(&key) {
                let mut bytes = Vec::new();
                Entry::new(&mut bytes, len).add_index(keys, token);
                self.overlay.insert(key.clone(), bytes);
                self.create_prefixes(&key);
                self.slot_mut(len).clone_from(&Slot::Empty);
                inserted = true;
                continue;
            }
            let mut bytes = self.record(&key).unwrap_or_default().to_vec();
            if Entry::new(&mut bytes, len).add_index(keys, token) {
                inserted = true;
            }
            self.overlay.insert(key.clone(), bytes);
            *self.slot_mut(len) = Slot::Record(key);
        }
        inserted
    }

    /// The pin's `add_index_internal` creates an empty marker for every
    /// proper prefix when the full record is new.
    fn create_prefixes(&mut self, key: &[u8]) {
        // `add_index_internal` walks shorter keys downward and stops at the
        // first prefix the table already holds.
        for words in (1..key.len() / 2).rev() {
            let prefix = key[..2 * words].to_vec();
            if self.contains_key(&prefix) {
                break;
            }
            self.overlay.insert(prefix, Vec::new());
        }
    }

    /// `ChewingLargeTable2::remove_index`. The pin stops after the first
    /// projection whose item is absent, so this does too.
    pub(crate) fn remove(&mut self, keys: &[ChewingKey], token: u32) -> bool {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return false;
        }
        for key in index_keys(keys) {
            let Some(mut bytes) = self.record(&key).map(<[u8]>::to_vec) else {
                return false;
            };
            let removed = Entry::new(&mut bytes, len).remove_index(keys, token);
            self.overlay.insert(key.clone(), bytes);
            *self.slot_mut(len) = Slot::Record(key);
            if !removed {
                return false;
            }
        }
        true
    }

    /// `ChewingLargeTable2::search`'s entry side: a hit on a non-empty
    /// record loads it into the length's slot.
    pub(crate) fn note_search(&mut self, keys: &[ChewingKey], natural: impl FnOnce() -> bool) {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return;
        }
        let key = query_key(keys);
        if !self.contains_key(&key) {
            return;
        }
        let nonempty = self
            .record(&key)
            .map_or_else(natural, |value| !value.is_empty());
        if nonempty {
            *self.slot_mut(len) = Slot::Record(key);
        }
    }

    /// `search_suggestion` empties every longer length it visits.
    pub(crate) fn note_suggestion(&mut self, keys: &[ChewingKey]) {
        let prefix_len = keys.len();
        if prefix_len == 0 || prefix_len > MAX_PHRASE_LENGTH {
            return;
        }
        let prefix = query_key(keys);
        let mut lengths = [false; MAX_PHRASE_LENGTH + 1];
        for key in self.keys_starting_with(&prefix) {
            if key.len() == prefix.len() {
                continue;
            }
            if let Some(seen) = lengths.get_mut(key.len() / 2) {
                *seen = true;
            }
        }
        for (len, seen) in lengths.iter().enumerate() {
            if *seen {
                *self.slot_mut(len) = Slot::Empty;
            }
        }
    }

    /// The pin's mask walk. Berkeley DB gives each record a fresh chunk;
    /// Kyoto Cabinet and tkrzw reuse one entry per length, so a shorter
    /// record written back can keep the previous one's tail (#785). The
    /// overlay ends holding exactly the bytes the pin stores for every key.
    pub(crate) fn mask(&mut self, mask: u32, value: u32) {
        let rows = self.rows();
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        {
            let mut chunks: Vec<Vec<u8>> = (0..=MAX_PHRASE_LENGTH)
                .map(|len| {
                    if len == 0 {
                        Vec::new()
                    } else {
                        self.slot_bytes(len)
                    }
                })
                .collect();
            for len in 1..=MAX_PHRASE_LENGTH {
                let chunk = &mut chunks[len];
                for (key, record) in rows.iter().filter(|(key, _)| key.len() / 2 == len) {
                    if chunk.len() < record.len() {
                        chunk.resize(record.len(), 0);
                    }
                    chunk[..record.len()].copy_from_slice(record);
                    Entry::new(chunk, len).mask_out(mask, value);
                    self.overlay.insert(key.clone(), chunk.clone());
                }
                *self.slot_mut(len) = if chunk.is_empty() {
                    Slot::Empty
                } else {
                    Slot::Bytes(chunk.clone())
                };
            }
        }
        #[cfg(not(any(feature = "kyotocabinet", feature = "tkrzw")))]
        {
            for (key, record) in rows {
                let mut bytes = record;
                if let Some(len) = std::num::NonZeroUsize::new(key.len() / 2)
                    && len.get() <= MAX_PHRASE_LENGTH
                {
                    Entry::new(&mut bytes, len.get()).mask_out(mask, value);
                }
                self.overlay.insert(key, bytes);
            }
        }
    }

    fn slot_mut(&mut self, len: usize) -> &mut Slot {
        &mut self.slots[len]
    }

    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn slot_bytes(&self, len: usize) -> Vec<u8> {
        match &self.slots[len] {
            Slot::Empty => Vec::new(),
            Slot::Bytes(bytes) => bytes.clone(),
            Slot::Record(key) => self.record(key).unwrap_or_default().to_vec(),
        }
    }

    /// The `(token, toned keys)` readings the records carry, deduplicated:
    /// the value model's `indexed` set.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn indexed_set(&self) -> BTreeSet<(u32, Vec<u16>)> {
        let mut set = BTreeSet::new();
        for (key, value) in self.rows() {
            if value.is_empty() || !key.len().is_multiple_of(2) {
                continue;
            }
            let len = key.len() / 2;
            if len == 0 || len > MAX_PHRASE_LENGTH {
                continue;
            }
            if let Ok(items) = oxpinyin_data::row_format::pinyin_index::decode_items(&value, len) {
                for item in items {
                    set.insert((
                        item.token,
                        item.keys.iter().map(|key| key.to_packed()).collect(),
                    ));
                }
            }
        }
        set
    }

    /// Every key in ascending order, arena and overlay merged.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn keys(&self) -> Vec<Vec<u8>> {
        let mut keys: Vec<Vec<u8>> = self
            .offsets
            .iter()
            .map(|offset| key_at(&self.arena, *offset).to_vec())
            .collect();
        for key in self.overlay.keys() {
            if !self.contains_arena_key(key) {
                keys.push(key.clone());
            }
        }
        keys.sort();
        keys
    }

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

    /// Every record in ascending key order: arena records with the
    /// overlay's edits applied, plus the keys the overlay created.
    pub(crate) fn rows(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = self
            .offsets
            .iter()
            .map(|offset| {
                let key = key_at(&self.arena, *offset);
                let value = self
                    .overlay
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| value_at(&self.arena, *offset).to_vec());
                (key.to_vec(), value)
            })
            .collect();
        for (key, value) in &self.overlay {
            if !self.contains_arena_key(key) {
                rows.push((key.clone(), value.clone()));
            }
        }
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        rows
    }

    /// `ChewingLargeTable2::search` over the stored records: the query's
    /// complete or incomplete key is looked up, and its records are filtered
    /// by the pin's `pinyin_compare_with_tones`. Tokens keep the record
    /// order (exact key, then token).
    pub(crate) fn search(&self, keys: &[ChewingKey]) -> Vec<(u32, Vec<ChewingKey>)> {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return Vec::new();
        }
        let Some(value) = self.record(&query_key(keys)) else {
            return Vec::new();
        };
        if value.is_empty() || !value.len().is_multiple_of(item2_stride(len)) {
            return Vec::new();
        }
        let stride = item2_stride(len);
        value
            .chunks_exact(stride)
            .filter_map(|record| {
                let stored: Vec<ChewingKey> = record[4..4 + 2 * len]
                    .chunks_exact(2)
                    .map(|word| ChewingKey::from_packed(u16::from_le_bytes([word[0], word[1]])))
                    .collect();
                matches_with_tones(keys, &stored, len).then(|| {
                    (
                        u32::from_le_bytes([record[0], record[1], record[2], record[3]]),
                        stored,
                    )
                })
            })
            .collect()
    }

    /// Whether the exact query record exists: `search`'s `m_db->Get` gate.
    pub(crate) fn has_record(&self, keys: &[ChewingKey]) -> bool {
        !keys.is_empty() && keys.len() <= MAX_PHRASE_LENGTH && self.contains_key(&query_key(keys))
    }

    /// Whether any record key extends `keys`' query key, including the key
    /// itself: `search_suggestion`'s `m_db->Get` gate.
    pub(crate) fn has_prefix(&self, keys: &[ChewingKey]) -> bool {
        if keys.is_empty() || keys.len() > MAX_PHRASE_LENGTH {
            return false;
        }
        !self.keys_starting_with(&query_key(keys)).is_empty()
    }

    /// `ChewingTableEntry::search_suggestion` over the stored records:
    /// longer keys extending the prefix, filtered with-tones.
    pub(crate) fn suggest(&self, prefix: &[ChewingKey]) -> Vec<u32> {
        let prefix_len = prefix.len();
        if prefix_len == 0 || prefix_len > MAX_PHRASE_LENGTH {
            return Vec::new();
        }
        let prefix_key = query_key(prefix);
        let mut tokens = Vec::new();
        for key in self.keys_starting_with(&prefix_key) {
            if key.len() <= prefix_key.len() || !key.len().is_multiple_of(2) {
                continue;
            }
            let len = key.len() / 2;
            let Some(value) = self.record(&key) else {
                continue;
            };
            let stride = item2_stride(len);
            if !value.len().is_multiple_of(stride) {
                continue;
            }
            for record in value.chunks_exact(stride) {
                let stored: Vec<ChewingKey> = record[4..4 + 2 * len]
                    .chunks_exact(2)
                    .map(|word| ChewingKey::from_packed(u16::from_le_bytes([word[0], word[1]])))
                    .collect();
                if matches_with_tones(prefix, &stored, prefix_len) {
                    tokens.push(u32::from_le_bytes([
                        record[0], record[1], record[2], record[3],
                    ]));
                }
            }
        }
        tokens
    }

    /// Adds records from `other` that this index does not hold. Used by a
    /// value-only save: the derived table is authoritative and the caller's
    /// index carries the retained empty keys.
    pub(crate) fn merge_missing(&mut self, other: &Self) {
        for (key, value) in other.rows() {
            if !self.contains_key(&key) {
                self.overlay.insert(key, value);
            }
        }
    }

    /// The keys `user_pinyin_index.bin` carried with no whole syllable or
    /// past the table's 16-syllable instantiation — the pin's two mask
    /// `switch` refusal sites.
    #[must_use]
    pub(crate) fn overlong_keys(&self) -> Vec<Vec<u16>> {
        let mut overlong = Vec::new();
        for offset in &self.offsets {
            let key = key_at(&self.arena, *offset);
            if key.len() < 2 {
                overlong.push(Vec::new());
            } else if key.len() / 2 > MAX_PHRASE_LENGTH {
                overlong.push(
                    key.chunks_exact(2)
                        .map(|word| u16::from_le_bytes([word[0], word[1]]))
                        .collect(),
                );
            }
        }
        overlong
    }
}

/// `pinyin_compare_with_tones` (`pinyin_phrase3.h:68-147` at 074a2219):
/// same length up to `len`; a zero middle/final on either side is a
/// wildcard, and so is a zero tone.
fn matches_with_tones(query: &[ChewingKey], stored: &[ChewingKey], len: usize) -> bool {
    if query.len() < len || stored.len() < len {
        return false;
    }
    for index in 0..len {
        let query = query[index];
        let stored = stored[index];
        if query.initial != stored.initial {
            return false;
        }
        if !((query.middle == stored.middle && query.final_ == stored.final_)
            || (query.middle == 0 && query.final_ == 0)
            || (stored.middle == 0 && stored.final_ == 0))
        {
            return false;
        }
        if !(query.tone == stored.tone || query.tone == 0 || stored.tone == 0) {
            return false;
        }
    }
    true
}

fn key_at(arena: &[u8], offset: u32) -> &[u8] {
    let at = offset as usize;
    let len = u32::from_le_bytes([arena[at], arena[at + 1], arena[at + 2], arena[at + 3]]) as usize;
    &arena[at + HEADER..at + HEADER + len]
}

fn value_at(arena: &[u8], offset: u32) -> &[u8] {
    let at = offset as usize;
    let key_len =
        u32::from_le_bytes([arena[at], arena[at + 1], arena[at + 2], arena[at + 3]]) as usize;
    let value_len =
        u32::from_le_bytes([arena[at + 4], arena[at + 5], arena[at + 6], arena[at + 7]]) as usize;
    &arena[at + HEADER + key_len..at + HEADER + key_len + value_len]
}

fn value_has_reading(value: &[u8], len: usize, token: u32, packed: &[u16]) -> bool {
    if value.is_empty() || len == 0 || packed.len() != len {
        return false;
    }
    let stride = item2_stride(len);
    value.len().is_multiple_of(stride)
        && value.chunks_exact(stride).any(|record| {
            u32::from_le_bytes([record[0], record[1], record[2], record[3]]) == token
                && record[4..4 + 2 * len]
                    .chunks_exact(2)
                    .map(|word| u16::from_le_bytes([word[0], word[1]]))
                    .eq(packed.iter().copied())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(initial: u8) -> ChewingKey {
        ChewingKey::new(initial, 1, 1, 0)
    }

    fn pair(a: u8, b: u8) -> Vec<ChewingKey> {
        vec![key(a), key(b)]
    }

    fn item(token: u32, keys: &[ChewingKey]) -> Vec<u8> {
        encode_item(token, keys)
    }

    fn record(items: &[(u32, &[ChewingKey])]) -> Vec<u8> {
        items
            .iter()
            .flat_map(|(token, keys)| item(*token, keys))
            .collect()
    }

    #[test]
    fn entry_ops_keep_the_pins_order_and_report_presence() {
        let (a, b) = (pair(2, 2), pair(3, 3));
        let mut bytes = Vec::new();
        let mut entry = Entry::new(&mut bytes, 2);
        assert!(entry.add_index(&b, 9));
        assert!(entry.add_index(&a, 7));
        assert!(entry.add_index(&a, 5));
        assert!(!entry.add_index(&a, 5));
        assert_eq!(*entry.bytes, record(&[(5, &a), (7, &a), (9, &b)]));
        assert!(!entry.remove_index(&a, 9));
        assert!(entry.remove_index(&a, 7));
        entry.mask_out(0xffff_ffff, 5);
        assert_eq!(*entry.bytes, record(&[(9, &b)]));
    }

    fn index(rows: &[(Vec<u8>, Vec<u8>)]) -> UserIndex {
        let mut index = UserIndex::new();
        for (key, value) in rows {
            index.push_record(key, value);
        }
        index.finish_load();
        index
    }

    #[test]
    fn a_loaded_record_answers_membership_without_a_side_set() {
        let keys = pair(2, 2);
        let complete = encode_complete_key(&keys);
        let incomplete = encode_incomplete_key(&keys);
        let mut index = index(&[
            (incomplete, record(&[(7, &keys)])),
            (complete, record(&[(7, &keys)])),
        ]);
        let packed: Vec<u16> = keys.iter().map(|key| key.to_packed()).collect();
        assert!(index.contains_reading(7, &packed));
        assert!(!index.contains_reading(8, &packed));
        assert!(index.add(&pair(3, 3), 9));
        assert!(index.contains_reading(
            9,
            &pair(3, 3).iter().map(|k| k.to_packed()).collect::<Vec<_>>()
        ));
    }

    #[test]
    fn add_and_remove_keep_prefix_markers_and_record_bytes() {
        let keys = pair(2, 3);
        let mut index = UserIndex::new();
        assert!(index.add(&keys, 7));
        let complete = encode_complete_key(&keys);
        let incomplete = encode_incomplete_key(&keys);
        assert_eq!(
            index.record(&complete),
            Some(record(&[(7, &keys)]).as_slice())
        );
        assert_eq!(
            index.record(&incomplete),
            Some(record(&[(7, &keys)]).as_slice())
        );
        assert_eq!(index.record(&complete[..2]), Some(&[][..]));
        assert!(!index.add(&keys, 7));
        assert!(index.remove(&keys, 7));
        assert!(index.record(&complete).is_some_and(<[u8]>::is_empty));
        assert!(!index.remove(&keys, 7));
    }

    #[test]
    fn mask_keeps_berkeley_db_records_independent() {
        let (a, b) = (pair(1, 1), pair(2, 2));
        let mut index = index(&[
            (encode_complete_key(&a), record(&[(1, &a), (5, &a)])),
            (encode_complete_key(&b), record(&[(2, &b)])),
        ]);
        index.mask(0xffff_ffff, 5);
        assert_eq!(
            index.record(&encode_complete_key(&a)),
            Some(record(&[(1, &a)]).as_slice())
        );
        assert_eq!(
            index.record(&encode_complete_key(&b)),
            Some(record(&[(2, &b)]).as_slice())
        );
    }
}
