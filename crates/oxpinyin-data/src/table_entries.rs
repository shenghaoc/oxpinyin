//! The two index DBMs' **entry-set** builders — the prefix-closure half
//! of the `ChewingLargeTable2` / `PhraseLargeTable3` writer paths.
//!
//! The row layouts themselves (the `PinyinIndexItem2<L>` record, the
//! key encodings, the stride) have one written copy in
//! [`crate::row_format`], shared with the runtime's readers; this module
//! owns what that module deliberately does not — assembling a whole
//! DBM's `(key, value)` rows from semantic records: every row lands in
//! both key spaces (incomplete and complete for the pinyin index), every
//! proper prefix of every stored key exists as an empty-value
//! continuation marker, and record order within a value follows
//! `pinyin_exact_compare2` with token ascending for identical keys
//! (`ChewingTableEntry::add_index`'s equal_range insert before the first
//! greater token).
//!
//! Rows come out sorted by ascending key bytes; KC TreeDB and tkrzw
//! TreeDBM both order byte-lexically, so the sorted writer order is also
//! the container's physical order. `oxpinyin-datagen` writes the system
//! tables with this and `oxpinyin-user`'s persistence writes the user
//! `user_pinyin_index.bin` / `user_phrase_index.bin` — a user table entry
//! passes through the same `add_index` upstream, so the record shapes
//! are identical.

use std::collections::BTreeMap;

use oxpinyin_core::ChewingKey;

use crate::row_format::phrase_index::{encode_tokens, encode_ucs4_key_scalars};
use crate::row_format::pinyin_index::{
    PinyinIndexItem, encode_complete_key, encode_incomplete_key, encode_item,
};

/// One DBM file's rows: `(key, value)` pairs in ascending key order.
pub type Entries = Vec<(Vec<u8>, Vec<u8>)>;

/// `pinyin_exact_compare2` (`pinyin_phrase3.h:33`): all initials across
/// syllables first, then middle/final per syllable, then tone per
/// syllable. This is the comparator the value arrays are sorted by
/// (`ChewingTableEntry::add_index`'s `phrase_exact_less_than2`
/// equal_range).
fn exact_compare2(lhs: &[ChewingKey], rhs: &[ChewingKey]) -> std::cmp::Ordering {
    let len = lhs.len();
    debug_assert_eq!(len, rhs.len());
    for i in 0..len {
        match lhs[i].initial.cmp(&rhs[i].initial) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    for i in 0..len {
        match lhs[i].middle.cmp(&rhs[i].middle) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
        match lhs[i].final_.cmp(&rhs[i].final_) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    for i in 0..len {
        match lhs[i].tone.cmp(&rhs[i].tone) {
            std::cmp::Ordering::Equal => {}
            other => return other,
        }
    }
    std::cmp::Ordering::Equal
}

/// One keyspace's accumulated rows: packed key bytes → the records in
/// arrival order.
type SpaceMap = BTreeMap<Vec<u8>, Vec<PinyinIndexItem>>;

/// The pinyin index rows (`pinyin_index.bin`, `addon_pinyin_index.bin`,
/// and the user `user_pinyin_index.bin`).
///
/// Every parsed row lands in **both** key spaces, upstream
/// `ChewingLargeTable2::add_index`'s two `add_index_internal` calls:
/// the incomplete (initial-only) keyspace and the complete (tone-zeroed)
/// keyspace. In each space the DBM holds one key per distinct syllable
/// sequence; its value is the space-sorted `PinyinIndexItem2` records of
/// every row with that sequence. Every proper prefix of every stored key
/// exists as an empty-value `SEARCH_CONTINUED` marker — the recursive
/// prefix fill in `add_index_internal` leaves exactly the prefix closure
/// behind (a prefix that is itself a stored key carries its records;
/// markers are never removed).
#[must_use]
pub fn pinyin_index_entries(rows: &[PinyinIndexItem]) -> Entries {
    // key bytes → records; a BTreeMap emits ascending key-byte order.
    let mut spaces: [SpaceMap; 2] = [BTreeMap::new(), BTreeMap::new()];

    for row in rows {
        // (keyspace index, DBM key): incomplete first, then complete —
        // upstream's add_index order, though the two spaces never share
        // a file entry.
        let dbm_keys = [
            encode_incomplete_key(&row.keys),
            encode_complete_key(&row.keys),
        ];
        for (space, dbm_key) in dbm_keys.into_iter().enumerate() {
            // Prefix markers: every proper prefix of this key exists in
            // the space (empty if never a stored key itself).
            let n = row.keys.len();
            for prefix in 1..n {
                spaces[space]
                    .entry(truncate_packed(&dbm_key, prefix))
                    .or_default();
            }
            spaces[space]
                .entry(dbm_key)
                .or_default()
                .push(PinyinIndexItem {
                    token: row.token,
                    keys: row.keys.clone(),
                });
        }
    }

    let mut entries: Entries = Vec::new();
    // One map across both spaces, keyed by raw key bytes. The two
    // spaces are disjoint for every real syllable — the incomplete space
    // zeroes middle and final, and no pinyin syllable has an initial
    // with neither — but a `ChewingKey` with a zero middle *and* final
    // would name the same bytes in both spaces, and the DBM cannot hold
    // two values for one key. Merging by key makes that collision a
    // union instead of a silent last-write-wins, and never fires on real
    // data.
    let mut merged: BTreeMap<Vec<u8>, Vec<PinyinIndexItem>> = BTreeMap::new();
    for space in spaces {
        for (key, records) in space {
            merged.entry(key).or_default().extend(records);
        }
    }
    for (key, mut records) in merged {
        records.sort_by(|a, b| exact_compare2(&a.keys, &b.keys).then(a.token.cmp(&b.token)));
        let mut value = Vec::with_capacity(
            records.len()
                * crate::row_format::pinyin_index::item2_stride(
                    records.first().map_or(0, |row| row.keys.len()),
                ),
        );
        for row in &records {
            value.extend_from_slice(&encode_item(row.token, &row.keys));
        }
        entries.push((key, value));
    }
    entries
}

/// Truncates a packed key to its first `syllables` syllables (each packed
/// syllable is 2 bytes) — a prefix marker's key.
fn truncate_packed(key: &[u8], syllables: usize) -> Vec<u8> {
    key[..2 * syllables].to_vec()
}

/// The phrase index rows (`phrase_index.bin`, `addon_phrase_index.bin`,
/// and the user `user_phrase_index.bin`).
///
/// Upstream `PhraseLargeTable3::load_text` + `PhraseTableEntry::add_index`
/// + the recursive prefix fill in `PhraseLargeTable3::add_index`:
///
/// * key — the phrase text as raw UCS-4 (`g_utf8_to_ucs4`), 4 LE bytes
///   per character;
/// * value — the phrase's tokens as ascending `u32` values
///   (`PhraseTableEntry::add_index` inserts before the first greater
///   token; an identical token is a no-op);
/// * every proper UCS-4 prefix of every phrase exists as an empty-value
///   continuation marker.
#[must_use]
pub fn phrase_index_entries(rows: &[(Vec<u32>, u32)]) -> Entries {
    let mut map: BTreeMap<Vec<u8>, Vec<u32>> = BTreeMap::new();
    for (phrase, token) in rows {
        let key = encode_ucs4_key_scalars(phrase);
        for prefix in 1..phrase.len() {
            map.entry(key[..4 * prefix].to_vec()).or_default();
        }
        let tokens = map.entry(key).or_default();
        match tokens.binary_search(token) {
            Ok(_) => {} // ERROR_INSERT_ITEM_EXISTS — ignored upstream.
            Err(position) => tokens.insert(position, *token),
        }
    }
    map.into_iter()
        .map(|(key, tokens)| (key, encode_tokens(&tokens)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(initial: u8, middle: u8, final_: u8) -> ChewingKey {
        ChewingKey::new(initial, middle, final_, 0)
    }

    fn row(token: u32, keys: &[ChewingKey]) -> PinyinIndexItem {
        PinyinIndexItem {
            token,
            keys: keys.to_vec(),
        }
    }

    #[test]
    fn pinyin_index_two_spaces_with_markers() {
        // One two-syllable row: "zhong guo" shape — distinct elements.
        let rows = vec![row(0x0100_0001, &[key(23, 0, 1), key(7, 0, 13)])];
        let entries = pinyin_index_entries(&rows);
        // Both spaces: lengths 1 (marker) and 2 (real) → 4 keys.
        assert_eq!(entries.len(), 4);
        // Incomplete length-1 key: initial-only first syllable.
        let inc1 = encode_incomplete_key(&[key(23, 0, 0)]);
        let inc2 = encode_incomplete_key(&[key(23, 0, 0), key(7, 0, 0)]);
        let comp2 = encode_complete_key(&[key(23, 0, 1), key(7, 0, 13)]);
        let by_key: BTreeMap<Vec<u8>, Vec<u8>> = entries.into_iter().collect();
        assert!(by_key[&inc1].is_empty(), "length-1 key is a marker");
        assert_eq!(by_key[&inc2].len(), 8, "L=2 records stride 8");
        assert_eq!(by_key[&comp2].len(), 8);
        // The complete-space length-1 prefix is the tone-zeroed first
        // syllable — a marker unless another row stores it.
        let comp1 = encode_complete_key(&[key(23, 0, 1)]);
        assert!(by_key[&comp1].is_empty());
    }

    #[test]
    fn pinyin_index_records_sort_by_exact_compare2_then_token() {
        // Two rows sharing a complete key: record order is exact_compare2
        // (all equal here) then token ascending.
        let rows = vec![
            row(0x0100_0009, &[key(5, 0, 3)]),
            row(0x0100_0002, &[key(5, 0, 3)]),
        ];
        let entries = pinyin_index_entries(&rows);
        let full_key = encode_complete_key(&[key(5, 0, 3)]);
        let (_, value) = entries.iter().find(|(k, _)| *k == full_key).expect("key");
        assert_eq!(value.len(), 16, "two L=1 records at stride 8");
        let first = u32::from_le_bytes(value[0..4].try_into().unwrap());
        let second = u32::from_le_bytes(value[8..12].try_into().unwrap());
        assert_eq!((first, second), (0x0100_0002, 0x0100_0009));
    }

    #[test]
    fn phrase_index_tokens_ascending_with_prefix_markers() {
        // 你好 (two chars) with two tokens inserted out of order, plus a
        // row reusing the first character as a full phrase.
        let rows = vec![
            (vec![0x4f60, 0x597d], 0x0100_0009_u32),
            (vec![0x4f60, 0x597d], 0x0100_0002),
            (vec![0x4f60, 0x597d], 0x0100_0002), // duplicate token: no-op
            (vec![0x4f60], 0x0100_0005),
        ];
        let entries = phrase_index_entries(&rows);
        let by_key: BTreeMap<Vec<u8>, Vec<u8>> = entries.into_iter().collect();
        let ni: Vec<u8> = 0x4f60_u32.to_le_bytes().to_vec();
        let nihao: Vec<u8> = [0x4f60_u32, 0x597d]
            .iter()
            .flat_map(|c| c.to_le_bytes())
            .collect();
        // 你 is both a prefix marker target and a real phrase: real value.
        assert_eq!(by_key[&ni], 0x0100_0005_u32.to_le_bytes().to_vec());
        assert_eq!(by_key[&nihao].len(), 8);
        let t0 = u32::from_le_bytes(by_key[&nihao][0..4].try_into().unwrap());
        let t1 = u32::from_le_bytes(by_key[&nihao][4..8].try_into().unwrap());
        assert_eq!((t0, t1), (0x0100_0002, 0x0100_0009));
        assert_eq!(by_key.len(), 2);
    }
}
