//! The state the pin's Kyoto Cabinet and tkrzw user `ChewingLargeTable2`
//! carries between operations (#785).
//!
//! The table owns one `ChewingTableEntry<len>` per key length
//! (`m_entries`) and every operation on the user table runs through it
//! (074a2219 `chewing_large_table2_kyotodb.cpp:148-178` search,
//! `:217-234` suggestion, `:278-337` add, `:376-400` remove;
//! `chewing_large_table2_tkrzwdb.cpp:133-160`, `:200-217` and twins). Its
//! `MemoryChunk` never shrinks on `set_content`, so the mask walk
//! (`MaskOutVisitor2` `:447-520`, `MaskOutProcessor2`
//! `chewing_large_table2_tkrzwdb.cpp:419-489`) starts every key length
//! from whatever the last operation of that length left behind, writes
//! each record over the front of it and stores the whole chunk, masked,
//! back: a record shorter than the one before it keeps that record's tail.
//! Empty records are visited like the rest (`visit_full` /
//! `ProcessFull`). Berkeley DB sets a fresh chunk per record and carries
//! nothing.
//!
//! The model keeps one slot per key length and the records whose bytes
//! differ from what the held readings derive ([`IndexKeys::tails`]). A slot
//! is lazy while the record it names cannot change: only an operation of
//! the same length edits a record, and each one rewrites the slot.

use std::collections::BTreeMap;

use oxpinyin_core::ChewingKey;
use oxpinyin_data::row_format::pinyin_index::{
    encode_complete_key, encode_incomplete_key, encode_item, item2_stride,
};
use oxpinyin_data::table_entries::Entries;

use crate::persistence::{IndexKeys, SystemOriginals, UserState};
use crate::phrase::MAX_PHRASE_LENGTH;

/// What one key length's entry holds.
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

/// A table key held inline: at most `MAX_PHRASE_LENGTH` packed syllables.
/// Ordered as the key bytes are, so a range over it walks the table's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ChainKey {
    bytes: [u8; 2 * MAX_PHRASE_LENGTH],
    len: u8,
}

impl ChainKey {
    fn new(key: &[u8]) -> Option<Self> {
        let mut bytes = [0; 2 * MAX_PHRASE_LENGTH];
        bytes.get_mut(..key.len())?.copy_from_slice(key);
        Some(Self {
            bytes,
            len: u8::try_from(key.len()).ok()?,
        })
    }

    fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
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
    /// Foreign or mask-carried unpartitioned records
    /// retain their bytes and use std's unspecified search result.
    fn equal_range(&self, value: &[ChewingKey]) -> (usize, usize) {
        if value.len() != self.len {
            return (0, 0);
        }
        // Lengths 1..=16 have eight strides. Borrow the whole records;
        // as_chunks leaves the same partial tail that count() excludes.
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

/// The key `ChewingLargeTable2::search` and `search_suggestion` probe.
fn query_key(keys: &[ChewingKey]) -> Vec<u8> {
    // `contains_incomplete_pinyin` (`pinyin_phrase3.h:146`).
    if keys.iter().any(|key| key.middle == 0 && key.final_ == 0) {
        encode_incomplete_key(keys)
    } else {
        encode_complete_key(keys)
    }
}

impl IndexKeys {
    /// Whether the table holds no key at all: no operation can leave state.
    pub(crate) fn is_idle(&self) -> bool {
        self.pinyin.is_empty() && self.added.is_empty()
    }

    /// Whether the table holds `key`: a key the files carried or a removal
    /// kept, a key an add wrote, or a proper prefix of one (the empty marker
    /// `add_index_internal` leaves for every prefix).
    fn holds(&self, key: &[u8]) -> bool {
        self.pinyin.contains(key)
            || ChainKey::new(key).is_some_and(|probe| {
                self.added
                    .range(probe..)
                    .next()
                    .is_some_and(|first| first.as_slice().starts_with(key))
            })
    }

    /// `search_internal` for a query of `keys`: a hit on a non-empty record
    /// loads it into the length's entry (`:148-178`). `natural` answers
    /// whether the held readings put an item under the probed key.
    pub(crate) fn note_search(&mut self, keys: &[ChewingKey], natural: impl FnOnce() -> bool) {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return;
        }
        let key = query_key(keys);
        if !self.holds(&key) {
            return;
        }
        let nonempty = self
            .tails
            .get(&key)
            .map_or_else(natural, |bytes| !bytes.is_empty());
        if nonempty {
            self.slots[len] = Slot::Record(key);
        }
    }

    /// `search_suggestion` for the prefix `keys`: every longer key that
    /// extends the probed key borrows its record into that length's entry
    /// and empties it (`:217-234`, `:540-596`).
    pub(crate) fn note_suggestion(&mut self, keys: &[ChewingKey]) {
        let prefix_len = keys.len();
        if prefix_len == 0 || prefix_len > MAX_PHRASE_LENGTH {
            return;
        }
        let prefix = query_key(keys);
        let Some(probe) = ChainKey::new(&prefix) else {
            return;
        };
        if !self.holds(&prefix) {
            return;
        }
        let mut lengths = [false; MAX_PHRASE_LENGTH + 1];
        for key in self.pinyin.range::<[u8], _>((
            std::ops::Bound::Excluded(prefix.as_slice()),
            std::ops::Bound::Unbounded,
        )) {
            if !key.starts_with(&prefix) {
                break;
            }
            if let Some(seen) = lengths.get_mut(key.len() / 2) {
                *seen = true;
            }
        }
        // An added key stands for itself and for every prefix of it.
        for key in self.added.range(probe..) {
            let key = key.as_slice();
            if !key.starts_with(&prefix) {
                break;
            }
            for seen in lengths
                .iter_mut()
                .take(key.len() / 2 + 1)
                .skip(prefix_len + 1)
            {
                *seen = true;
            }
        }
        for (len, seen) in lengths.iter().enumerate() {
            if *seen {
                self.slots[len] = Slot::Empty;
            }
        }
    }

    /// `ChewingLargeTable2::add_index` of `(keys, token)`; `exists` is
    /// whether the held readings already carry the item. Creates the
    /// records the pin creates, edits any record whose bytes differ from the
    /// derived ones, and leaves the entry holding the last existing record
    /// it touched.
    pub(crate) fn note_add(&mut self, keys: &[ChewingKey], token: u32, exists: bool) {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return;
        }
        for key in index_keys(keys) {
            if !self.holds(&key) {
                // A new record; the entry is not used, and every proper
                // prefix of the key is held from now on.
                if let Some(key) = ChainKey::new(&key) {
                    self.added.insert(key);
                }
                continue;
            }
            self.slots[len] = Slot::Record(key.clone());
            let inserted = match self.tails.get_mut(&key) {
                Some(bytes) => Entry::new(bytes, len).add_index(keys, token),
                None => !exists,
            };
            if !inserted {
                return;
            }
        }
    }

    /// `ChewingLargeTable2::remove_index` of `(keys, token)`, called while
    /// the readings still carry the item.
    pub(crate) fn note_remove(&mut self, keys: &[ChewingKey], token: u32) {
        let len = keys.len();
        if len == 0 || len > MAX_PHRASE_LENGTH {
            return;
        }
        for key in index_keys(keys) {
            self.slots[len] = Slot::Record(key.clone());
            if let Some(bytes) = self.tails.get_mut(&key)
                && !Entry::new(bytes, len).remove_index(keys, token)
            {
                return;
            }
        }
    }

    /// The bytes a slot holds, with `records` the table as it stands.
    fn slot_bytes(&self, len: usize, records: &BTreeMap<&[u8], &[u8]>) -> Vec<u8> {
        match &self.slots[len] {
            Slot::Empty => Vec::new(),
            Slot::Bytes(bytes) => bytes.clone(),
            Slot::Record(key) => records
                .get(key.as_slice())
                .map(|bytes| bytes.to_vec())
                .unwrap_or_default(),
        }
    }

    /// The mask walk over `before`, the table's records in ascending key
    /// order (derived ones with the tails laid over them): each record is
    /// written over the front of its length's entry, the entry is masked
    /// and stored back whole. `after` is what the readings left by the
    /// mask derive; a record that differs from it becomes a tail.
    pub(crate) fn mask_walk(&mut self, before: &Entries, after: &Entries, mask: u32, value: u32) {
        let held: BTreeMap<&[u8], &[u8]> = before
            .iter()
            .map(|(key, bytes)| (key.as_slice(), bytes.as_slice()))
            .collect();
        let mut chunks: Vec<Vec<u8>> = (0..=MAX_PHRASE_LENGTH)
            .map(|len| {
                if len == 0 {
                    Vec::new()
                } else {
                    self.slot_bytes(len, &held)
                }
            })
            .collect();
        let natural: BTreeMap<&[u8], &[u8]> = after
            .iter()
            .map(|(key, bytes)| (key.as_slice(), bytes.as_slice()))
            .collect();
        let mut tails: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
        for (key, bytes) in before {
            let len = key.len() / 2;
            if len == 0 || len > MAX_PHRASE_LENGTH {
                continue;
            }
            let chunk = &mut chunks[len];
            if chunk.len() < bytes.len() {
                chunk.resize(bytes.len(), 0);
            }
            chunk[..bytes.len()].copy_from_slice(bytes);
            Entry::new(chunk, len).mask_out(mask, value);
            let derived = natural.get(key.as_slice()).copied().unwrap_or(&[]);
            if chunk.as_slice() != derived {
                tails.insert(key.clone(), chunk.clone());
            }
        }
        self.tails = tails;
        for (len, chunk) in chunks.into_iter().enumerate().skip(1) {
            self.slots[len] = if chunk.is_empty() {
                Slot::Empty
            } else {
                Slot::Bytes(chunk)
            };
        }
    }

    /// `rows` with every tail laid over its key (or added as a record).
    pub(crate) fn with_tails(&self, rows: Entries) -> Entries {
        if self.tails.is_empty() {
            return rows;
        }
        let mut merged: BTreeMap<Vec<u8>, Vec<u8>> = rows.into_iter().collect();
        for (key, bytes) in &self.tails {
            merged.insert(key.clone(), bytes.clone());
        }
        merged.into_iter().collect()
    }
}

/// The records of the loaded file that the loaded readings do not derive,
/// by key: the pin keeps every record as it read it, tails included.
pub(crate) fn file_tails(
    state: &UserState,
    originals: &SystemOriginals,
    keys: &std::collections::BTreeSet<Vec<u8>>,
    records: Vec<(Vec<u8>, Vec<u8>)>,
) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let rows = crate::persistence::chewing_rows(state, originals);
    let derived: BTreeMap<Vec<u8>, Vec<u8>> = crate::persistence::with_kept_keys(
        oxpinyin_data::table_entries::pinyin_index_entries(&rows),
        keys,
    )
    .into_iter()
    .collect();
    records
        .into_iter()
        .filter(|(key, value)| {
            derived
                .get(key)
                .map_or(!value.is_empty(), |held| held != value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use oxpinyin_data::table_entries::exact_compare2;
    use proptest::prelude::*;

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

    #[test]
    fn packed_comparison_uses_phrase_field_precedence() {
        let examples = [
            // The second initial outranks the first syllable's other fields.
            ([ChewingKey::new(3, 3, 31, 7), key(2)], [key(3), key(3)]),
            // The first middle/final pair outranks the second pair.
            (
                [ChewingKey::new(3, 1, 2, 7), ChewingKey::new(3, 3, 31, 7)],
                [ChewingKey::new(3, 1, 3, 0), ChewingKey::new(3, 0, 0, 0)],
            ),
            // The second middle/final pair outranks the first tone.
            (
                [ChewingKey::new(3, 1, 1, 7), ChewingKey::new(3, 0, 0, 7)],
                [key(3), ChewingKey::new(3, 1, 1, 0)],
            ),
            // Tone order is lexicographic only after the other fields tie.
            (
                [ChewingKey::new(3, 1, 1, 0), ChewingKey::new(3, 1, 1, 7)],
                [ChewingKey::new(3, 1, 1, 1), ChewingKey::new(3, 1, 1, 0)],
            ),
        ];
        for (lhs, rhs) in examples {
            assert!(compare_record(&encode_item(1, &lhs), &rhs).is_lt());
            assert!(compare_record(&encode_item(1, &rhs), &lhs).is_gt());
        }
    }

    proptest! {
        #[test]
        fn packed_comparison_preserves_every_field_precedence(
            left in any::<[u16; MAX_PHRASE_LENGTH]>(),
            right in any::<[u16; MAX_PHRASE_LENGTH]>(),
            len in 1..=MAX_PHRASE_LENGTH,
        ) {
            let lhs: Vec<_> = left[..len].iter().copied().map(ChewingKey::from_packed).collect();
            let rhs: Vec<_> = right[..len].iter().copied().map(ChewingKey::from_packed).collect();
            let mut packed = encode_item(u32::MAX, &lhs);
            // The stored key's spare bit and record's alignment bytes have
            // no place in the comparison.
            for high in packed[5..4 + 2 * len].iter_mut().step_by(2) {
                *high |= 0x80;
            }
            packed[4 + 2 * len..].fill(0xff);
            prop_assert_eq!(compare_record(&packed, &rhs), exact_compare2(&lhs, &rhs));
        }

        #[test]
        fn unpartitioned_ranges_are_bounded_and_preserve_stored_order(
            row in prop::collection::vec(any::<u16>(), 0..64),
            probe in any::<u16>(),
            len in 1..=MAX_PHRASE_LENGTH,
        ) {
            let mut bytes: Vec<_> = row.iter().enumerate().flat_map(|(n, word)| {
                encode_item(u32::try_from(n).expect("small fixture"),
                            &vec![ChewingKey::from_packed(*word); len])
            }).collect();
            bytes.extend_from_slice(&[0xff; 3]);
            let before = bytes.clone();
            let (first, last) = Entry::new(&mut bytes, len)
                .equal_range(&vec![ChewingKey::from_packed(probe); len]);
            // No position or membership is promised on unpartitioned
            // input. Searching the upper suffix still bounds the range.
            prop_assert!(first <= last && last <= row.len());
            prop_assert_eq!(bytes, before);
        }
    }

    #[test]
    fn every_stride_finds_equal_keys_and_keeps_the_partial_tail() {
        for len in 1..=MAX_PHRASE_LENGTH {
            let mut rows: Vec<_> = (1..=12)
                .map(|n| {
                    let mut keys = vec![key(3); len];
                    keys[len - 1] = ChewingKey::new(n % 4, n % 3, n, n % 5);
                    keys
                })
                .collect();
            rows.sort_by(|a, b| exact_compare2(a, b));
            // Equal keys with distinct tokens must span the full range.
            rows.insert(4, rows[4].clone());
            let pristine: Vec<_> = rows
                .iter()
                .enumerate()
                .flat_map(|(n, keys)| encode_item(u32::try_from(n).expect("small fixture"), keys))
                .collect();
            for tail_len in 0..item2_stride(len) {
                let mut bytes = pristine.clone();
                bytes.extend(std::iter::repeat_n(0xa5, tail_len));
                let before = bytes.clone();
                let entry = Entry::new(&mut bytes, len);
                let below = vec![ChewingKey::new(0, 0, 0, 0); len];
                let above = vec![ChewingKey::new(31, 3, 31, 7); len];
                assert!(exact_compare2(&below, &rows[0]).is_lt());
                assert!(exact_compare2(&above, &rows[rows.len() - 1]).is_gt());
                for probe in rows.iter().chain([&below, &above]) {
                    let first = rows
                        .iter()
                        .position(|keys| !exact_compare2(keys, probe).is_lt())
                        .unwrap_or(rows.len());
                    let last = first
                        + rows[first..]
                            .iter()
                            .take_while(|keys| exact_compare2(keys, probe).is_eq())
                            .count();
                    assert_eq!(entry.equal_range(probe), (first, last), "length {len}");
                }
                assert_eq!(*entry.bytes, before);
            }
        }
    }

    #[test]
    fn every_stride_handles_empty_records_and_retains_trailing_bytes_on_edits() {
        for len in 1..=MAX_PHRASE_LENGTH {
            let keys = vec![key(3); len];
            for tail_len in 0..item2_stride(len) {
                let tail = vec![0xa5; tail_len];
                let mut bytes = tail.clone();
                let mut entry = Entry::new(&mut bytes, len);
                assert_eq!(entry.equal_range(&keys), (0, 0));
                assert!(!entry.remove_index(&keys, 7));
                assert_eq!(*entry.bytes, tail);
                assert!(entry.add_index(&keys, 7));
                let mut expected = encode_item(7, &keys);
                expected.extend_from_slice(&tail);
                assert_eq!(*entry.bytes, expected);
                assert!(!entry.add_index(&keys, 7));
                assert!(entry.remove_index(&keys, 7));
                assert_eq!(*entry.bytes, tail);
            }
        }
    }

    #[test]
    fn a_search_loads_only_a_hit_on_a_non_empty_record() {
        let a = pair(2, 2);
        let mut keys = IndexKeys::default();
        keys.pinyin.insert(encode_complete_key(&a));
        keys.note_search(&a, || false);
        assert_eq!(keys.slots[2], Slot::Empty);
        keys.note_search(&a, || true);
        assert_eq!(keys.slots[2], Slot::Record(encode_complete_key(&a)));
        keys.note_search(&pair(9, 9), || true);
        assert_eq!(keys.slots[2], Slot::Record(encode_complete_key(&a)));
    }

    #[test]
    fn a_suggestion_empties_every_longer_length_it_visits() {
        let prefix = vec![key(2)];
        let mut keys = IndexKeys::default();
        let long = [key(2), key(3), key(4)];
        keys.note_add(&long, 1, false);
        keys.slots[2] = Slot::Bytes(vec![1; 8]);
        keys.slots[3] = Slot::Bytes(vec![1; 12]);
        keys.slots[4] = Slot::Bytes(vec![1; 12]);
        keys.note_suggestion(&prefix);
        assert_eq!(keys.slots[2], Slot::Empty);
        assert_eq!(keys.slots[3], Slot::Empty);
        assert_eq!(keys.slots[4], Slot::Bytes(vec![1; 12]));
    }

    #[test]
    fn the_walk_gives_the_first_record_of_a_length_the_entry_s_tail() {
        let (a, b, c) = (pair(1, 1), pair(2, 2), pair(3, 3));
        let (ka, kb, kc) = (
            encode_complete_key(&a),
            encode_complete_key(&b),
            encode_complete_key(&c),
        );
        // `b` holds two items and a hit loaded it; `a` and `c` hold one.
        let before: Entries = vec![
            (ka.clone(), record(&[(1, &a)])),
            (kb.clone(), record(&[(2, &b), (3, &b)])),
            (kc.clone(), record(&[(4, &c)])),
        ];
        let after = before.clone();
        let mut keys = IndexKeys::default();
        keys.slots[2] = Slot::Record(kb.clone());
        keys.mask_walk(&before, &after, 0xffff_ffff, 0x00ff_ffff);
        // `a` keeps the second item of `b`, `b` is rewritten whole (so it
        // is the record its readings derive) and `c` keeps the second item
        // of `b` behind its own front.
        assert_eq!(keys.tails.get(&ka), Some(&record(&[(1, &a), (3, &b)])));
        assert_eq!(keys.tails.get(&kb), None);
        assert_eq!(keys.tails.get(&kc), Some(&record(&[(4, &c), (3, &b)])));
        assert_eq!(
            keys.slots[2],
            Slot::Bytes(record(&[(4, &c), (3, &b)])),
            "the entry ends holding the last record's chunk"
        );
    }

    #[test]
    fn an_empty_entry_leaves_the_derived_records_alone() {
        let (a, b) = (pair(1, 1), pair(2, 2));
        let before: Entries = vec![
            (encode_complete_key(&a), record(&[(1, &a)])),
            (encode_complete_key(&b), record(&[(2, &b)])),
        ];
        let mut keys = IndexKeys::default();
        keys.mask_walk(&before, &before, 0xffff_ffff, 0x00ff_ffff);
        assert!(keys.tails.is_empty());
    }

    #[test]
    fn a_masked_item_leaves_a_record_that_the_readings_derive_untouched() {
        let (a, b) = (pair(1, 1), pair(2, 2));
        let ka = encode_complete_key(&a);
        let before: Entries = vec![
            (ka.clone(), record(&[(1, &a), (5, &a)])),
            (encode_complete_key(&b), record(&[(2, &b)])),
        ];
        let after: Entries = vec![
            (ka, record(&[(1, &a)])),
            (encode_complete_key(&b), record(&[(2, &b)])),
        ];
        let mut keys = IndexKeys::default();
        keys.mask_walk(&before, &after, 0xffff_ffff, 5);
        assert!(keys.tails.is_empty());
    }

    #[test]
    fn an_add_to_an_existing_record_loads_it_and_a_new_record_does_not() {
        let a = pair(2, 2);
        let mut keys = IndexKeys::default();
        keys.note_add(&a, 1, false);
        assert_eq!(keys.slots[2], Slot::Empty);
        assert!(keys.holds(&encode_complete_key(&a)));
        assert!(keys.holds(&encode_complete_key(&a)[..2]));
        keys.note_add(&a, 2, false);
        assert_eq!(keys.slots[2], Slot::Record(encode_complete_key(&a)));
    }
}
