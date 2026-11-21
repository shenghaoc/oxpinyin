//! The user pinyin index as the pin's own records.
//!
//! `ChewingLargeTable2` holds the user index as one DBM keyed by packed
//! `ChewingKey` runs; each value is a sorted `PinyinIndexItem2<L>` array.
//! The session keeps these records in the backend's ordered in-memory
//! tree. `Entry` applies the pin's
//! `ChewingTableEntry::add_index` / `remove_index` / `mask_out` edits, and
//! the Kyoto Cabinet and tkrzw mask walk keeps its per-length entry
//! (`Slot`) because those backends store the whole reused chunk back and
//! can carry the previous record's tail (#785). Berkeley DB uses a fresh
//! chunk per record, so it masks each record independently.
//!
//! Keys the file carried are kept verbatim, including empty prefix
//! markers and malformed rows: the pin's `load_db` copies the table and
//! `save_db` copies it back.

use crate::store::StoreError;

use std::collections::BTreeSet;

use crate::data::phrase_library::CHEWING_KEY_SIZE;
use crate::data::row_format::pinyin_index::{
    encode_complete_key, encode_incomplete_key, encode_item, item2_stride,
};
use smallvec::SmallVec;

use crate::parser::ChewingKey;
use crate::user::records::NativeTable;
use crate::vocab::MAX_PHRASE_LENGTH;

/// Whether the table keeps one entry per key length between operations,
/// as the Kyoto Cabinet and tkrzw `ChewingLargeTable2` do: `search` loads
/// the record into it (`chewing_large_table2_kyotodb.cpp:170`,
/// `chewing_large_table2_tkrzwdb.cpp:156` at 074a2219), `search_suggestion`
/// empties it (`:233`, `:216`) and the mask walk stores its whole chunk
/// back (`:471`, `:439`). Berkeley DB points the entry at each record
/// (`chewing_large_table2_bdb.cpp:189, 498`), so nothing carries over.
pub(crate) const ENTRY_PER_LENGTH: bool = cfg!(not(feature = "bdb"));

/// What one key length's table entry holds between operations.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum Slot {
    /// Nothing: a fresh entry, or a suggestion's `set_size(0)`.
    #[default]
    Empty,
    /// The record stored under this key, as the table holds it now.
    Record(Vec<u8>),
    /// The chunk a mask walk left.
    #[cfg(not(feature = "bdb"))]
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
        record_token(&self.bytes[index * self.stride..])
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
type QueryKey = SmallVec<[u8; CHEWING_KEY_SIZE * MAX_PHRASE_LENGTH]>;

/// The key `ChewingLargeTable2::search` / `search_suggestion` probe, the
/// bytes `encode_incomplete_key` or `encode_complete_key` give. Callers
/// reject an empty or over-long `keys` first.
fn query_key(keys: &[ChewingKey]) -> QueryKey {
    // `contains_incomplete_pinyin` (`pinyin_phrase3.h:146`).
    let incomplete = keys.iter().any(|key| key.middle == 0 && key.final_ == 0);
    let mut query = QueryKey::with_capacity(CHEWING_KEY_SIZE * keys.len());
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

/// The user pinyin index and its per-length entry state.
#[derive(Debug, Default)]
pub(crate) struct UserIndex {
    /// The pin's `(key, value)` rows.
    records: NativeTable,
    /// The pin's per-length entry state for the KC/tkrzw mask walk.
    slots: [Slot; MAX_PHRASE_LENGTH + 1],
}

impl UserIndex {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self {
            records: NativeTable::default(),
            slots: [const { Slot::Empty }; MAX_PHRASE_LENGTH + 1],
        }
    }

    /// Builds an index from the rows of `user_pinyin_index.bin` (the
    /// loaded profile's table).
    #[cfg(test)]
    pub(crate) fn from_entries(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        let mut index = Self::new();
        for (key, value) in &entries {
            index.push_record(key, value).unwrap();
        }
        index
    }

    /// Sets one DBM record.
    pub(crate) fn push_record(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.records.insert(key, value)
    }

    /// The value stored under `key`, in the native tree.
    pub(crate) fn record(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.records.record(key)
    }

    pub(crate) fn save(&self, path: &std::path::Path) -> Result<(), StoreError> {
        self.records.save(path)
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.records.contains_key(key)
    }

    /// Every key in ascending order, in native order.
    #[cfg(test)]
    pub(crate) fn keys(&self) -> Vec<Vec<u8>> {
        self.records.keys()
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
                && value_has_reading(&value, keys.len(), token, packed)
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
                if self.records.insert(&key, &bytes).is_err() {
                    return false;
                }
                if !self.create_prefixes(&key) {
                    return false;
                }
                // A new key is built in a local entry; the length's shared
                // entry keeps what it held (`add_index_internal`,
                // `chewing_large_table2_tkrzwdb.cpp:274-299` and
                // `chewing_large_table2_kyotodb.cpp:292-302` at 074a2219).
                inserted = true;
                continue;
            }
            let mut bytes = self.record(&key).unwrap_or_default();
            if Entry::new(&mut bytes, len).add_index(keys, token) {
                inserted = true;
            }
            if self.records.insert(&key, &bytes).is_err() {
                return false;
            }
            *self.slot_mut(len) = Slot::Record(key);
        }
        inserted
    }

    /// The pin's `add_index_internal` creates an empty marker for every
    /// proper prefix when the full record is new.
    fn create_prefixes(&mut self, key: &[u8]) -> bool {
        // `add_index_internal` walks shorter keys downward and stops at the
        // first prefix the table already holds.
        for words in (1..key.len() / CHEWING_KEY_SIZE).rev() {
            let prefix = key[..CHEWING_KEY_SIZE * words].to_vec();
            if self.contains_key(&prefix) {
                break;
            }
            if self.records.insert(&prefix, &[]).is_err() {
                return false;
            }
        }
        true
    }

    /// Check the ordered removal sequence using only the affected rows.
    pub(crate) fn can_remove(
        &self,
        readings: impl IntoIterator<Item = Vec<ChewingKey>>,
        token: u32,
    ) -> bool {
        let mut rows = std::collections::BTreeMap::new();
        for keys in readings {
            let len = keys.len();
            if len == 0 || len > MAX_PHRASE_LENGTH {
                return false;
            }
            for key in index_keys(&keys) {
                let bytes = rows.entry(key.clone()).or_insert_with(|| self.record(&key));
                let Some(bytes) = bytes else { return false };
                if !Entry::new(bytes, len).remove_index(&keys, token) {
                    return false;
                }
            }
        }
        true
    }

    /// `ChewingLargeTable2::remove_index`. The pin stops after the first
    /// projection whose item is absent, so this does too.
    pub(crate) fn remove(&mut self, keys: &[ChewingKey], token: u32) -> bool {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return false;
        }
        for key in index_keys(keys) {
            let Some(mut bytes) = self.record(&key) else {
                return false;
            };
            let removed = Entry::new(&mut bytes, len).remove_index(keys, token);
            if self.records.insert(&key, &bytes).is_err() {
                return false;
            }
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
            if let Some(seen) = lengths.get_mut(key.len() / CHEWING_KEY_SIZE) {
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
    /// The visitor writes each replacement into the native container.
    pub(crate) fn mask(&mut self, mask: u32, value: u32) {
        #[cfg(not(feature = "bdb"))]
        let mut chunks: [Vec<u8>; MAX_PHRASE_LENGTH + 1] = std::array::from_fn(|len| {
            if len == 0 {
                Vec::new()
            } else {
                self.slot_bytes(len)
            }
        });
        #[cfg(feature = "bdb")]
        let mut scratch = Vec::new();
        self.records
            .rewrite(&mut |key, record, kept| {
                let len = key.len() / CHEWING_KEY_SIZE;
                if len == 0 || len > MAX_PHRASE_LENGTH {
                    kept.extend_from_slice(record);
                    return;
                }
                #[cfg(not(feature = "bdb"))]
                {
                    let chunk = &mut chunks[len];
                    if chunk.len() < record.len() {
                        chunk.resize(record.len(), 0);
                    }
                    chunk[..record.len()].copy_from_slice(record);
                    Entry::new(chunk, len).mask_out(mask, value);
                    kept.extend_from_slice(chunk);
                }
                #[cfg(feature = "bdb")]
                {
                    scratch.clear();
                    scratch.extend_from_slice(record);
                    Entry::new(&mut scratch, len).mask_out(mask, value);
                    kept.extend_from_slice(&scratch);
                }
            })
            .unwrap_or_else(|_| std::process::abort());
        #[cfg(not(feature = "bdb"))]
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

    #[cfg(not(feature = "bdb"))]
    fn slot_bytes(&self, len: usize) -> Vec<u8> {
        match &self.slots[len] {
            Slot::Empty => Vec::new(),
            Slot::Bytes(bytes) => bytes.clone(),
            Slot::Record(key) => self.record(key).unwrap_or_default(),
        }
    }

    /// The `(token, toned keys)` readings the records carry, deduplicated:
    /// the value model's `indexed` set.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn indexed_set(&self) -> BTreeSet<(u32, Vec<u16>)> {
        let mut set = BTreeSet::new();
        let Ok(()) = self
            .records
            .for_each_record::<std::convert::Infallible>(|key, value| {
                if value.is_empty() || !key.len().is_multiple_of(CHEWING_KEY_SIZE) {
                    return Ok(());
                }
                let len = key.len() / CHEWING_KEY_SIZE;
                if len == 0 || len > MAX_PHRASE_LENGTH {
                    return Ok(());
                }
                if let Ok(items) = crate::data::row_format::pinyin_index::decode_items(value, len) {
                    for item in items {
                        set.insert((
                            item.token,
                            item.keys.iter().map(|key| key.to_packed()).collect(),
                        ));
                    }
                }
                Ok(())
            });
        set
    }

    /// `ChewingLargeTable2::search` over the stored records: the query's
    /// complete or incomplete key is looked up, and its records are filtered
    /// by the pin's `pinyin_compare_with_tones`. Tokens keep the record
    /// order (exact key, then token), duplicates kept.
    pub(crate) fn search(&self, keys: &[ChewingKey]) -> Vec<u32> {
        let mut tokens = Vec::new();
        self.visit_matches(keys, |token| tokens.push(token));
        tokens
    }

    pub(crate) fn visit_matches(&self, keys: &[ChewingKey], visit: impl FnMut(u32)) {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return;
        }
        let Some(value) = self.record(&query_key(keys)) else {
            return;
        };
        // `ChewingLargeTable2::search` reads `IndexItem` pointers from the
        // chunk's begin to its end (`chewing_large_table2.h:145-150` at
        // 074a2219), so a trailing partial record is never read (register
        // row 129).
        value
            .chunks_exact(item2_stride(len))
            .filter(|record| record_matches_with_tones(keys, record))
            .map(record_token)
            .for_each(visit);
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
            if key.len() <= prefix_key.len() || !key.len().is_multiple_of(CHEWING_KEY_SIZE) {
                continue;
            }
            let len = key.len() / CHEWING_KEY_SIZE;
            let Some(value) = self.record(&key) else {
                continue;
            };
            // `search_suggestion` reads whole records the same way
            // (`chewing_large_table2.h:182-200`).
            tokens.extend(
                value
                    .chunks_exact(item2_stride(len))
                    .filter(|record| record_matches_with_tones(prefix, record))
                    .map(record_token),
            );
        }
        tokens
    }

    /// Adds records from `other` that this index does not hold. Used by a
    /// value-only save: the derived table is authoritative and the caller's
    /// index carries the retained empty keys.
    #[cfg(test)]
    pub(crate) fn merge_missing(&mut self, other: &Self) {
        let mut missing = Vec::new();
        let Ok(()) = other
            .records
            .for_each_record::<std::convert::Infallible>(|key, value| {
                if !self.contains_key(key) {
                    missing.push((key.to_vec(), value.to_vec()));
                }
                Ok(())
            });
        for (key, value) in missing {
            self.records.insert(&key, &value).unwrap();
        }
    }
}

/// [`ChewingKey::matches_with_tones`] over the first `query.len()` packed words of
/// one `PinyinIndexItem2` record, read in place. Each word decodes through
/// `from_packed`, padding bit 15 masked; the query's own fields are
/// compared, never a packed copy that would truncate an out-of-range one.
/// The record holds at least `query.len()` words.
fn record_matches_with_tones(query: &[ChewingKey], record: &[u8]) -> bool {
    let (words, _) = record[4..4 + CHEWING_KEY_SIZE * query.len()].as_chunks::<2>();
    words.iter().zip(query).all(|(word, query)| {
        query.matches_with_tones(ChewingKey::from_packed(u16::from_le_bytes(*word)))
    })
}

/// The `phrase_token_t` leading a `PinyinIndexItem2` record.
fn record_token(record: &[u8]) -> u32 {
    u32::from_le_bytes(record.as_chunks::<4>().0[0])
}

fn value_has_reading(value: &[u8], len: usize, token: u32, packed: &[u16]) -> bool {
    if value.is_empty() || len == 0 || packed.len() != len {
        return false;
    }
    // Whole records only, as `search` reads them.
    value.chunks_exact(item2_stride(len)).any(|record| {
        record_token(record) == token
            && record[4..4 + CHEWING_KEY_SIZE * len]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|word| u16::from_le_bytes(*word))
                .eq(packed.iter().copied())
    })
}

#[cfg(test)]
mod tests;
