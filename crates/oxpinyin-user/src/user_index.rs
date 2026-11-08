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

use std::collections::BTreeSet;

use oxpinyin_core::ChewingKey;
use oxpinyin_data::row_format::pinyin_index::{
    encode_complete_key, encode_incomplete_key, encode_item, item2_stride,
};
use smallvec::SmallVec;

use crate::phrase::MAX_PHRASE_LENGTH;
use crate::records::RecordArena;

/// Whether the table keeps one entry per key length between operations,
/// as the Kyoto Cabinet and tkrzw `ChewingLargeTable2` do: `search` loads
/// the record into it (`chewing_large_table2_kyotodb.cpp:170`,
/// `chewing_large_table2_tkrzwdb.cpp:156` at 074a2219), `search_suggestion`
/// empties it (`:233`, `:216`) and the mask walk stores its whole chunk
/// back (`:471`, `:439`). Berkeley DB points the entry at each record
/// (`chewing_large_table2_bdb.cpp:189, 498`), so nothing carries over.
pub(crate) const ENTRY_PER_LENGTH: bool = cfg!(any(feature = "kyotocabinet", feature = "tkrzw"));

/// What one key length's table entry holds between operations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Nothing: a fresh entry, or a suggestion's `set_size(0)`.
    #[default]
    Empty,
    /// The record stored under this key, as the table holds it now.
    Record(Vec<u8>),
    /// The chunk a mask walk left.
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
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

/// A probe key built on the stack: at most `MAX_PHRASE_LENGTH` packed
/// words, always inside the inline capacity.
type QueryKey = SmallVec<[u8; 2 * MAX_PHRASE_LENGTH]>;

/// The key `ChewingLargeTable2::search` / `search_suggestion` probe, the
/// bytes `encode_incomplete_key` or `encode_complete_key` give. Callers
/// reject an empty or over-long `keys` first.
fn query_key(keys: &[ChewingKey]) -> QueryKey {
    // `contains_incomplete_pinyin` (`pinyin_phrase3.h:146`).
    let incomplete = keys.iter().any(|key| key.middle == 0 && key.final_ == 0);
    let mut query = QueryKey::with_capacity(2 * keys.len());
    for key in keys {
        let key = if incomplete {
            ChewingKey::new(key.initial, 0, 0, 0)
        } else {
            ChewingKey::new(key.initial, key.middle, key.final_, 0)
        };
        query.extend_from_slice(&key.to_packed().to_le_bytes());
    }
    query
}

/// The user pinyin index, owned in one arena.
#[derive(Clone, Debug, Default)]
pub(crate) struct UserIndex {
    /// The pin's `(key, value)` rows.
    records: RecordArena,
    /// The pin's per-length entry state for the KC/tkrzw mask walk.
    slots: [Slot; MAX_PHRASE_LENGTH + 1],
}

impl UserIndex {
    pub(crate) const fn new() -> Self {
        Self {
            records: RecordArena::new(),
            slots: [const { Slot::Empty }; MAX_PHRASE_LENGTH + 1],
        }
    }

    /// Builds an index from the rows of `user_pinyin_index.bin` (the
    /// loaded profile's table).
    pub(crate) fn from_entries(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        let mut index = Self::new();
        for (key, value) in &entries {
            index.push_record(key, value);
        }
        index.finish_load();
        index
    }

    /// Appends one DBM record.
    pub(crate) fn push_record(&mut self, key: &[u8], value: &[u8]) {
        self.records.push_record(key, value);
    }

    /// Sorts the offset vector after a walk that did not arrive in key
    /// order.
    pub(crate) fn finish_load(&mut self) {
        self.records.finish_load();
    }

    /// The value stored under `key`, arena or overlay.
    pub(crate) fn record(&self, key: &[u8]) -> Option<&[u8]> {
        self.records.record(key)
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.records.contains_key(key)
    }

    /// Every record in ascending key order: arena records with the
    /// overlay's edits applied, plus the keys the overlay created.
    pub(crate) fn rows(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        self.records.rows()
    }

    /// Every key in ascending order, arena and overlay merged.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<Vec<u8>> {
        self.records.keys()
    }

    /// Visits every record in ascending key order, arena and overlay
    /// merged, without materialising a row vector.
    pub(crate) fn for_each_record<E>(
        &self,
        visit: impl FnMut(&[u8], &[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        self.records.for_each_record(visit)
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
                self.records.insert(key.clone(), bytes);
                self.create_prefixes(&key);
                // A new key is built in a local entry; the length's shared
                // entry keeps what it held (`add_index_internal`,
                // `chewing_large_table2_tkrzwdb.cpp:274-299` and
                // `chewing_large_table2_kyotodb.cpp:292-302` at 074a2219).
                inserted = true;
                continue;
            }
            let mut bytes = self.record(&key).unwrap_or_default().to_vec();
            if Entry::new(&mut bytes, len).add_index(keys, token) {
                inserted = true;
            }
            self.records.insert(key.clone(), bytes);
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
            self.records.insert(prefix, Vec::new());
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
            self.records.insert(key.clone(), bytes);
            *self.slot_mut(len) = Slot::Record(key);
            if !removed {
                return false;
            }
        }
        true
    }

    /// `ChewingLargeTable2::search`'s entry side: a hit on a non-empty
    /// record loads it into the length's slot.
    pub(crate) fn note_search(&mut self, keys: &[ChewingKey]) {
        let len = keys.len();
        if !ENTRY_PER_LENGTH || len == 0 || len > MAX_PHRASE_LENGTH {
            return;
        }
        let key = query_key(keys);
        let nonempty = self.record(&key).is_some_and(|value| !value.is_empty());
        if nonempty {
            *self.slot_mut(len) = Slot::Record(key.to_vec());
        }
    }

    /// `search_suggestion` empties every longer length it visits, but only
    /// when the prefix key itself is a row: the pin returns before it makes a
    /// cursor when `check(prefix)` is `-1`
    /// (`chewing_large_table2_kyotodb.cpp:541-544`), when `DB_SET` on the
    /// prefix fails (`chewing_large_table2_bdb.cpp:579-583`) and when
    /// `Get(prefix)` is not OK (`chewing_large_table2_tkrzwdb.cpp:511-512`,
    /// all at 074a2219). An empty marker row counts as present.
    pub(crate) fn note_suggestion(&mut self, keys: &[ChewingKey]) {
        let prefix_len = keys.len();
        if !ENTRY_PER_LENGTH || prefix_len == 0 || prefix_len > MAX_PHRASE_LENGTH {
            return;
        }
        let prefix = query_key(keys);
        if !self.contains_key(&prefix) {
            return;
        }
        let mut lengths = [false; MAX_PHRASE_LENGTH + 1];
        for key in self.records.keys_starting_with(&prefix) {
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

    /// The pin's mask walk: one pass over every record in key order, as
    /// `ChewingLargeTable2::mask_out`'s `ProcessEach` makes it
    /// (`chewing_large_table2_tkrzwdb.cpp:419-491` and its Kyoto Cabinet
    /// and Berkeley DB twins at 074a2219). Berkeley DB gives each record a
    /// fresh chunk; Kyoto Cabinet and tkrzw reuse one entry per length, so a
    /// shorter record written back can keep the previous one's tail (#785).
    /// The records are rewritten into a new arena in the same pass and the
    /// overlay is emptied, so no record is held twice.
    pub(crate) fn mask(&mut self, mask: u32, value: u32) {
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        let mut chunks: [Vec<u8>; MAX_PHRASE_LENGTH + 1] = std::array::from_fn(|len| {
            if len == 0 {
                Vec::new()
            } else {
                self.slot_bytes(len)
            }
        });
        #[cfg(not(any(feature = "kyotocabinet", feature = "tkrzw")))]
        let mut scratch = Vec::new();
        self.records.rebuild(|key, record, kept| {
            let len = key.len() / 2;
            if len == 0 || len > MAX_PHRASE_LENGTH {
                kept.extend_from_slice(record);
                return;
            }
            #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
            {
                let chunk = &mut chunks[len];
                if chunk.len() < record.len() {
                    chunk.resize(record.len(), 0);
                }
                chunk[..record.len()].copy_from_slice(record);
                Entry::new(chunk, len).mask_out(mask, value);
                kept.extend_from_slice(chunk);
            }
            #[cfg(not(any(feature = "kyotocabinet", feature = "tkrzw")))]
            {
                scratch.clear();
                scratch.extend_from_slice(record);
                Entry::new(&mut scratch, len).mask_out(mask, value);
                kept.extend_from_slice(&scratch);
            }
        });
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        for (len, chunk) in chunks.into_iter().enumerate().skip(1) {
            *self.slot_mut(len) = if chunk.is_empty() {
                Slot::Empty
            } else {
                Slot::Bytes(chunk)
            };
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
        for (key, value) in self.records.rows() {
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

    /// `ChewingLargeTable2::search` over the stored records: the query's
    /// complete or incomplete key is looked up, and its records are filtered
    /// by the pin's `pinyin_compare_with_tones`. Tokens keep the record
    /// order (exact key, then token), duplicates kept.
    pub(crate) fn search(&self, keys: &[ChewingKey]) -> Vec<u32> {
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
            .filter(|record| record_matches_with_tones(keys, record))
            .map(record_token)
            .collect()
    }

    /// Whether the exact query record exists: `search`'s `m_db->Get` gate.
    pub(crate) fn has_record(&self, keys: &[ChewingKey]) -> bool {
        !keys.is_empty() && keys.len() <= MAX_PHRASE_LENGTH && self.contains_key(&query_key(keys))
    }

    /// Whether the exact query key exists, including an empty marker.
    pub(crate) fn has_prefix(&self, keys: &[ChewingKey]) -> bool {
        self.has_record(keys)
    }

    /// `ChewingTableEntry::search_suggestion` over the stored records:
    /// longer keys extending the prefix, filtered with-tones.
    pub(crate) fn suggest(&self, prefix: &[ChewingKey]) -> Vec<u32> {
        if !self.has_record(prefix) {
            return Vec::new();
        }
        let prefix_key = query_key(prefix);
        let mut tokens = Vec::new();
        for key in self.records.keys_starting_with(&prefix_key) {
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
            tokens.extend(
                value
                    .chunks_exact(stride)
                    .filter(|record| record_matches_with_tones(prefix, record))
                    .map(record_token),
            );
        }
        tokens
    }

    /// Adds records from `other` that this index does not hold. Used by a
    /// value-only save: the derived table is authoritative and the caller's
    /// index carries the retained empty keys.
    pub(crate) fn merge_missing(&mut self, other: &Self) {
        for (key, value) in other.records.rows() {
            if !self.contains_key(&key) {
                self.records.insert(key, value);
            }
        }
    }

    /// The keys `user_pinyin_index.bin` carried with no whole syllable or
    /// past the table's 16-syllable instantiation — the pin's two mask
    /// `switch` refusal sites.
    #[must_use]
    pub(crate) fn overlong_keys(&self) -> Vec<Vec<u16>> {
        let mut overlong = Vec::new();
        for key in self.records.arena_keys() {
            if key.len() < 2 {
                overlong.push(Vec::new());
            } else if key.len() / 2 > MAX_PHRASE_LENGTH {
                overlong.push(
                    key.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|word| u16::from_le_bytes([word[0], word[1]]))
                        .collect(),
                );
            }
        }
        overlong
    }
}

/// Whether `pinyin_compare_with_tones` (`pinyin_phrase3.h:111-142` at
/// 074a2219) answers 0 for one position. Its three passes (initials, then
/// middle/final pairs, then tones) answer 0 only when every position
/// passes all three tests, so the passes reduce to this conjunction per
/// position: equal initials (`pinyin_compare_initial3`, `:69-72`); equal
/// middle and final, or a zero middle and final on either side
/// (`pinyin_compare_middle_and_final3`, `:74-91`); equal tones, or a zero
/// tone on either side (`pinyin_compare_tone3`, `:93-102`).
fn key_matches_with_tones(query: ChewingKey, stored: ChewingKey) -> bool {
    query.initial == stored.initial
        && ((query.middle == stored.middle && query.final_ == stored.final_)
            || (query.middle == 0 && query.final_ == 0)
            || (stored.middle == 0 && stored.final_ == 0))
        && (query.tone == stored.tone || query.tone == 0 || stored.tone == 0)
}

/// `pinyin_compare_with_tones` over the first `query.len()` packed words of
/// one `PinyinIndexItem2` record, read in place. Each word decodes through
/// `from_packed`, padding bit 15 masked; the query's own fields are
/// compared, never a packed copy that would truncate an out-of-range one.
/// The record holds at least `query.len()` words.
fn record_matches_with_tones(query: &[ChewingKey], record: &[u8]) -> bool {
    let (words, _) = record[4..4 + 2 * query.len()].as_chunks::<2>();
    words.iter().zip(query).all(|(word, query)| {
        key_matches_with_tones(*query, ChewingKey::from_packed(u16::from_le_bytes(*word)))
    })
}

/// The `phrase_token_t` leading a `PinyinIndexItem2` record.
fn record_token(record: &[u8]) -> u32 {
    u32::from_le_bytes([record[0], record[1], record[2], record[3]])
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
                    .as_chunks::<2>()
                    .0
                    .iter()
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
    fn has_prefix_probes_the_arena_and_the_overlay_markers() {
        let full = pair(1, 2);
        let mut index = index(&[(encode_complete_key(&full), record(&[(7, &full)]))]);
        assert!(!index.has_prefix(&full[..1]));
        assert!(index.suggest(&full[..1]).is_empty());
        assert!(index.has_prefix(&full));
        assert!(!index.has_prefix(&pair(3, 4)[..1]));
        // Adding a longer key creates its empty prefix marker, which counts
        // as a stored key.
        assert!(index.add(&pair(3, 4), 8));
        assert!(
            index
                .record(&encode_complete_key(&pair(3, 4)[..1]))
                .is_some_and(<[u8]>::is_empty)
        );
        assert!(index.has_prefix(&pair(3, 4)[..1]));
        assert!(!index.has_prefix(&pair(5, 6)[..1]));
    }

    fn toned(initial: u8, tone: u8) -> ChewingKey {
        ChewingKey::new(initial, 1, 1, tone)
    }

    #[test]
    fn query_key_builds_the_encoders_bytes_on_the_stack() {
        let complete = [toned(2, 3), toned(4, 0)];
        assert_eq!(*query_key(&complete), *encode_complete_key(&complete));
        let incomplete = [toned(2, 3), ChewingKey::new(4, 0, 0, 0)];
        assert_eq!(*query_key(&incomplete), *encode_incomplete_key(&incomplete));
        let longest = [toned(5, 1); MAX_PHRASE_LENGTH];
        assert_eq!(*query_key(&longest), *encode_complete_key(&longest));
    }

    #[test]
    fn search_filters_a_mixed_tone_row_in_stored_order() {
        let (one, two, none) = (
            [toned(2, 1), toned(3, 4)],
            [toned(2, 2), toned(3, 4)],
            [toned(2, 0), toned(3, 0)],
        );
        // Unsorted, with a duplicate: stored as found.
        let row = record(&[(9, &two), (7, &one), (8, &none), (7, &one), (6, &two)]);
        let index = index(&[(encode_complete_key(&one), row)]);
        assert_eq!(index.search(&one), [7, 8, 7]);
        assert_eq!(index.search(&two), [9, 8, 6]);
        assert_eq!(index.search(&[toned(2, 3), toned(3, 4)]), [8]);
        assert_eq!(index.search(&[toned(2, 1), toned(3, 5)]), [8]);
    }

    #[test]
    fn a_zero_tone_query_matches_every_toned_record() {
        let records = [
            [toned(2, 1), toned(3, 4)],
            [toned(2, 2), toned(3, 1)],
            [toned(2, 5), toned(3, 0)],
        ];
        let row = record(&[(1, &records[0]), (2, &records[1]), (3, &records[2])]);
        let index = index(&[(encode_complete_key(&records[0]), row)]);
        assert_eq!(index.search(&[toned(2, 0), toned(3, 0)]), [1, 2, 3]);
        assert_eq!(index.search(&[toned(2, 0), toned(3, 1)]), [2, 3]);
    }

    #[test]
    fn a_malformed_stride_row_answers_nothing() {
        let keys = pair(2, 3);
        let mut row = record(&[(7, &keys), (8, &keys)]);
        row.push(0);
        let longer = [key(2), key(3), key(4)];
        let mut longer_row = record(&[(9, &longer)]);
        longer_row.pop();
        let index = index(&[
            (encode_complete_key(&keys), row),
            (encode_complete_key(&longer), longer_row),
        ]);
        assert!(index.has_record(&keys));
        assert!(index.search(&keys).is_empty());
        assert!(index.suggest(&keys).is_empty());
    }

    #[test]
    fn suggest_filters_longer_records_with_tones() {
        let prefix = [toned(2, 1)];
        let (one, two) = ([toned(2, 1), toned(3, 2)], [toned(2, 2), toned(3, 2)]);
        let index = index(&[
            (encode_complete_key(&prefix), Vec::new()),
            (
                encode_complete_key(&one),
                record(&[(5, &two), (4, &one), (4, &one)]),
            ),
        ]);
        assert_eq!(index.suggest(&prefix), [4, 4]);
        assert_eq!(index.suggest(&[toned(2, 0)]), [5, 4, 4]);
    }

    /// The pin's `pinyin_compare_with_tones` (`pinyin_phrase3.h:111-142` at
    /// 074a2219) transcribed pass for pass, over decoded keys.
    fn pin_compare_with_tones(lhs: &[ChewingKey], rhs: &[ChewingKey]) -> i32 {
        for (lhs, rhs) in lhs.iter().zip(rhs) {
            let result = i32::from(lhs.initial) - i32::from(rhs.initial);
            if result != 0 {
                return result;
            }
        }
        for (lhs, rhs) in lhs.iter().zip(rhs) {
            if (lhs.middle == rhs.middle && lhs.final_ == rhs.final_)
                || (lhs.middle == 0 && lhs.final_ == 0)
                || (rhs.middle == 0 && rhs.final_ == 0)
            {
                continue;
            }
            let middle = i32::from(lhs.middle) - i32::from(rhs.middle);
            if middle != 0 {
                return middle;
            }
            return i32::from(lhs.final_) - i32::from(rhs.final_);
        }
        for (lhs, rhs) in lhs.iter().zip(rhs) {
            if lhs.tone == rhs.tone || lhs.tone == 0 || rhs.tone == 0 {
                continue;
            }
            return i32::from(lhs.tone) - i32::from(rhs.tone);
        }
        0
    }

    #[test]
    fn the_in_place_compare_agrees_with_decoding_for_every_packed_word() {
        let mut queries = Vec::new();
        for initial in [0, 1, 23, 31, 32, 40] {
            for (middle, final_) in [(0, 0), (0, 3), (1, 0), (2, 17), (3, 31), (4, 0), (0, 32)] {
                for tone in [0, 1, 5, 7, 8] {
                    queries.push(ChewingKey::new(initial, middle, final_, tone));
                }
            }
        }
        let mut record = [0u8; 6];
        for word in 0..=u16::MAX {
            record[4..].copy_from_slice(&word.to_le_bytes());
            let stored = [ChewingKey::from_packed(word)];
            for query in &queries {
                assert_eq!(
                    record_matches_with_tones(std::slice::from_ref(query), &record),
                    pin_compare_with_tones(std::slice::from_ref(query), &stored) == 0,
                    "word {word:#06x} query {query:?}",
                );
            }
        }
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

    /// Two length-2 keys, `short` sorting first with one item and `long`
    /// after it with two, as the Kyoto Cabinet and tkrzw mask walk visits
    /// them (ascending key order).
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn short_then_long() -> (Vec<ChewingKey>, Vec<ChewingKey>) {
        let (x, y) = (pair(1, 1), pair(2, 2));
        if encode_complete_key(&x) < encode_complete_key(&y) {
            (x, y)
        } else {
            (y, x)
        }
    }

    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn long_record(long: &[ChewingKey]) -> Vec<u8> {
        record(&[(2, long), (3, long)])
    }

    /// The short record after a mask that removes nothing, with `tail`
    /// appended: the previous record's items beyond its length that a
    /// reused chunk keeps.
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    fn masked_short(short: &[ChewingKey], tail: &[u8]) -> Vec<u8> {
        let mut expected = record(&[(1, short)]);
        expected.extend_from_slice(tail);
        expected
    }

    #[cfg(feature = "bdb")]
    #[test]
    fn a_mask_pass_starts_every_record_from_its_own_bytes() {
        // `ChewingLargeTable2::mask_out` points the entry at each record's
        // own data with `set_chunk(db_data.data, db_data.size, NULL)` and
        // puts the entry back with `DB_CURRENT` (chewing_large_table2_bdb.cpp:
        // 498-506 at 074a2219), so a longer record never leaves a tail behind
        // a shorter one, in either walk order and over repeated passes.
        let (x, y) = (pair(1, 1), pair(2, 2));
        let (short, long) = if encode_complete_key(&x) < encode_complete_key(&y) {
            (x, y)
        } else {
            (y, x)
        };
        let long_items = record(&[(2, &long), (3, &long)]);
        let mut index = index(&[
            (encode_complete_key(&short), record(&[(1, &short)])),
            (encode_complete_key(&long), long_items.clone()),
        ]);
        for _ in 0..2 {
            index.mask(0xffff_ffff, 99);
            assert_eq!(
                index.record(&encode_complete_key(&short)),
                Some(record(&[(1, &short)]).as_slice())
            );
            assert_eq!(
                index.record(&encode_complete_key(&long)),
                Some(long_items.as_slice())
            );
        }
        // The same walk also removes what the mask names, record by record.
        index.mask(0xffff_ffff, 3);
        assert_eq!(
            index.record(&encode_complete_key(&long)),
            Some(record(&[(2, &long)]).as_slice())
        );
        assert_eq!(
            index.record(&encode_complete_key(&short)),
            Some(record(&[(1, &short)]).as_slice())
        );
    }

    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    #[test]
    fn a_search_hit_leaves_its_record_in_the_entry_the_next_mask_starts_from() {
        // `search_internal` loads a non-empty record into the length's
        // entry (chewing_large_table2_kyotodb.cpp:160-174 at 074a2219).
        // `MaskOutVisitor2::visit_full` then writes each record over the
        // start of that entry with `set_content(0, ...)` and returns the
        // whole entry (:471-478); `set_content` keeps the larger of the old
        // size and the new length (memory_chunk.h:290-296). The first record
        // the walk visits is shorter than the searched one, so it leaves
        // with the searched record's second item behind it.
        let (short, long) = short_then_long();
        let rows = [
            (encode_complete_key(&short), record(&[(1, &short)])),
            (encode_complete_key(&long), long_record(&long)),
        ];

        let mut searched = index(&rows);
        searched.note_search(&long);
        searched.mask(0xffff_ffff, 99);
        let item_len = record(&[(2, &long)]).len();
        assert_eq!(
            searched.record(&encode_complete_key(&short)),
            Some(masked_short(&short, &long_record(&long)[item_len..]).as_slice()),
            "the searched record's tail follows the first record"
        );
        assert_eq!(
            searched.record(&encode_complete_key(&long)),
            Some(long_record(&long).as_slice()),
            "the next record overwrites the whole entry"
        );

        // Without a search the entry starts empty and nothing is carried.
        let mut fresh = index(&rows);
        fresh.mask(0xffff_ffff, 99);
        assert_eq!(
            fresh.record(&encode_complete_key(&short)),
            Some(record(&[(1, &short)]).as_slice())
        );
    }

    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    #[test]
    fn a_second_mask_starts_from_the_chunks_the_first_left_in_the_entries() {
        // `m_entries` outlives each `mask_out` call: the entry of a length
        // keeps the last record's chunk when the walk ends
        // (chewing_large_table2_kyotodb.cpp:459-479 at 074a2219 never
        // resets it). The first pass ends on the long record, so the second
        // pass's first (short) record is written over the start of that
        // chunk and keeps its second item behind it (memory_chunk.h:290-296).
        let (short, long) = short_then_long();
        let mut index = index(&[
            (encode_complete_key(&short), record(&[(1, &short)])),
            (encode_complete_key(&long), long_record(&long)),
        ]);
        index.mask(0xffff_ffff, 99);
        assert_eq!(
            index.record(&encode_complete_key(&short)),
            Some(record(&[(1, &short)]).as_slice()),
            "the first pass starts from an empty entry"
        );
        index.mask(0xffff_ffff, 99);
        let item_len = record(&[(2, &long)]).len();
        assert_eq!(
            index.record(&encode_complete_key(&short)),
            Some(masked_short(&short, &long_record(&long)[item_len..]).as_slice()),
            "the second pass starts from the long record's chunk"
        );
        assert_eq!(
            index.record(&encode_complete_key(&long)),
            Some(long_record(&long).as_slice())
        );
    }

    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    #[test]
    fn a_suggestion_empties_the_entries_of_the_lengths_it_visits() {
        // `search_suggestion` checks the prefix key, jumps to it, and steps
        // through every longer key that extends it
        // (chewing_large_table2_kyotodb.cpp:541-582, `kyotodb_chewing_
        // continue_search` :34-50 at 074a2219). Each visit borrows the
        // record into that length's entry and ends with `set_size(0)`
        // (:229-233), so the entry the earlier search loaded is empty.
        let (short, long) = short_then_long();
        let prefix = &long[..1];
        let mut index = index(&[
            (encode_complete_key(prefix), Vec::new()),
            (encode_complete_key(&short), record(&[(1, &short)])),
            (encode_complete_key(&long), long_record(&long)),
        ]);
        index.note_search(&long);
        index.note_suggestion(prefix);
        index.mask(0xffff_ffff, 99);
        assert_eq!(
            index.record(&encode_complete_key(&short)),
            Some(record(&[(1, &short)]).as_slice()),
            "the visited length's entry was emptied"
        );
        assert_eq!(index.record(&encode_complete_key(prefix)), Some(&[][..]));
    }

    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    #[test]
    fn a_suggestion_on_an_absent_prefix_key_leaves_the_entries_alone() {
        // The pin's first call is `m_db->check(prefix)`; `-1` returns before
        // any cursor is made (chewing_large_table2_kyotodb.cpp:541-544,
        // chewing_large_table2_bdb.cpp:579-583 and
        // chewing_large_table2_tkrzwdb.cpp:511-512 at 074a2219), so no
        // longer key is visited and no entry is emptied, even though a
        // longer key exists.
        let (short, long) = short_then_long();
        let mut index = index(&[
            (encode_complete_key(&short), record(&[(1, &short)])),
            (encode_complete_key(&long), long_record(&long)),
        ]);
        index.note_search(&long);
        index.note_suggestion(&long[..1]);
        index.mask(0xffff_ffff, 99);
        let item_len = record(&[(2, &long)]).len();
        assert_eq!(
            index.record(&encode_complete_key(&short)),
            Some(masked_short(&short, &long_record(&long)[item_len..]).as_slice()),
        );
    }
}
