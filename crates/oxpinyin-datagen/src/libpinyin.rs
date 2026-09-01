//! libpinyin-schema row builders for the KC/Tkrzw drop-in DBMs.
//!
//! The two index DBMs (`pinyin_index.bin`, `phrase_index.bin`) are the
//! byte-level output of upstream's `ChewingLargeTable2::load_text` /
//! `PhraseLargeTable3::load_text` writer paths
//! (`docs/findings/pinyin-dbm-format-2026-09-01.md`,
//! `docs/findings/phrase-dbm-format-2026-09-01.md`). This module turns the
//! semantic records the [`crate::system`] compile produces into the exact
//! `(key, value)` rows those writers leave behind, prefix markers included.
//!
//! Rows come out sorted by ascending key bytes (the `Entries` contract);
//! KC TreeDB and Tkrzw TreeDBM both order byte-lexically, so the sorted
//! writer order is also the container's physical order.
//!
//! Only the KC/Tkrzw producers emit this schema. redb and LMDB keep the
//! native oxpinyin schema — no drop-in requirement exists for them
//! (`docs/findings/datagen-compat-2026-09-01.md`).

use std::collections::BTreeMap;

use oxpinyin_core::ChewingKey;

use crate::Entries;

/// `sizeof(PinyinIndexItem2<L>)` — the `u32` token field's alignment
/// rounds the `4 + 2L` field sum up to the next multiple of 4
/// (`docs/findings/pinyin-dbm-format-2026-09-01.md` §5).
#[must_use]
pub const fn item2_stride(phrase_length: usize) -> usize {
    (4 + 2 * phrase_length + 3) & !3
}

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

/// One `.table` row's parsed pronunciation, the input unit of both index
/// writers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedRow {
    /// The row's token.
    pub token: u32,
    /// The parsed syllable keys with their tones (`USE_TONE` semantics —
    /// `PinyinDirectParser2::parse_one_key`). The model20 tables carry no
    /// tone digits, so these are tone-zero in practice; the field keeps
    /// the parse faithful to upstream for toned models.
    pub keys: Vec<ChewingKey>,
}

/// The packed two-byte LE form of one key.
fn pack(key: ChewingKey) -> [u8; 2] {
    key.to_packed().to_le_bytes()
}

/// `compute_incomplete_chewing_index` (`pinyin_phrase3.h:171`): keep only
/// each syllable's initial (middle, final, tone zero).
fn incomplete_key(keys: &[ChewingKey]) -> Vec<u8> {
    keys.iter()
        .flat_map(|k| pack(ChewingKey::new(k.initial, 0, 0, 0)))
        .collect()
}

/// `compute_chewing_index` (`pinyin_phrase3.h:160`): tone every syllable
/// to zero, keep the rest.
fn complete_key(keys: &[ChewingKey]) -> Vec<u8> {
    keys.iter()
        .flat_map(|k| pack(ChewingKey::new(k.initial, k.middle, k.final_, 0)))
        .collect()
}

/// One keyspace's accumulated rows: packed key bytes → `(stored keys,
/// token)` records in arrival order.
type SpaceMap = BTreeMap<Vec<u8>, Vec<(Vec<ChewingKey>, u32)>>;

/// Serialises one `PinyinIndexItem2<L>` record: token, then the stored
/// keys (their original tones), then zero padding to the stride.
fn encode_item(token: u32, keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = vec![0_u8; item2_stride(keys.len())];
    buf[..4].copy_from_slice(&token.to_le_bytes());
    for (i, key) in keys.iter().enumerate() {
        let bytes = pack(*key);
        buf[4 + 2 * i..6 + 2 * i].copy_from_slice(&bytes);
    }
    buf
}

/// The pinyin index rows (`pinyin_index.bin` / `addon_pinyin_index.bin`).
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
///
/// Record order within a value is `pinyin_exact_compare2` with token
/// ascending for identical keys (`ChewingTableEntry::add_index`'s
/// equal_range insert before the first greater token).
#[must_use]
pub fn pinyin_index_entries(rows: &[ParsedRow]) -> Entries {
    // key bytes → records; a BTreeMap emits ascending key-byte order.
    let mut spaces: [SpaceMap; 2] = [BTreeMap::new(), BTreeMap::new()];

    for row in rows {
        let keys_with_tone = row.keys.clone();
        // (keyspace index, DBM key): incomplete first, then complete —
        // upstream's add_index order, though the two spaces never share
        // a file entry.
        let dbm_keys = [incomplete_key(&row.keys), complete_key(&row.keys)];
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
                .push((keys_with_tone.clone(), row.token));
        }
    }

    let mut entries: Entries = Vec::new();
    for space in &spaces {
        for (key, mut records) in space.clone().into_iter() {
            records.sort_by(|a, b| exact_compare2(&a.0, &b.0).then(a.1.cmp(&b.1)));
            let mut value = Vec::with_capacity(
                records.len() * item2_stride(records.first().map_or(0, |(k, _)| k.len())),
            );
            for (keys, token) in &records {
                value.extend_from_slice(&encode_item(*token, keys));
            }
            entries.push((key, value));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

/// Truncates a packed key to its first `syllables` syllables (each packed
/// syllable is 2 bytes) — a prefix marker's key.
fn truncate_packed(key: &[u8], syllables: usize) -> Vec<u8> {
    key[..2 * syllables].to_vec()
}

/// The phrase index rows (`phrase_index.bin` / `addon_phrase_index.bin`).
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
        let key: Vec<u8> = phrase.iter().flat_map(|c| c.to_le_bytes()).collect();
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
        .map(|(key, tokens)| {
            let value: Vec<u8> = tokens.iter().flat_map(|t| t.to_le_bytes()).collect();
            (key, value)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(initial: u8, middle: u8, final_: u8) -> ChewingKey {
        ChewingKey::new(initial, middle, final_, 0)
    }

    #[test]
    fn stride_formula() {
        assert_eq!(item2_stride(1), 8);
        assert_eq!(item2_stride(2), 8);
        assert_eq!(item2_stride(3), 12);
        assert_eq!(item2_stride(4), 12);
        assert_eq!(item2_stride(5), 16);
        assert_eq!(item2_stride(16), 36);
    }

    #[test]
    fn pinyin_index_two_spaces_with_markers() {
        // One two-syllable row: "zhong guo" shape — distinct elements.
        let rows = vec![ParsedRow {
            token: 0x0100_0001,
            keys: vec![key(23, 0, 1), key(7, 0, 13)],
        }];
        let entries = pinyin_index_entries(&rows);
        // Both spaces: lengths 1 (marker) and 2 (real) → 4 keys.
        assert_eq!(entries.len(), 4);
        // Incomplete length-1 key: initial-only first syllable.
        let inc1 = key(23, 0, 0).to_packed().to_le_bytes().to_vec();
        let inc2: Vec<u8> = [key(23, 0, 0), key(7, 0, 0)]
            .iter()
            .flat_map(|k| k.to_packed().to_le_bytes())
            .collect();
        let comp2: Vec<u8> = [key(23, 0, 1), key(7, 0, 13)]
            .iter()
            .flat_map(|k| k.to_packed().to_le_bytes())
            .collect();
        let by_key: BTreeMap<Vec<u8>, Vec<u8>> = entries.into_iter().collect();
        assert!(by_key[&inc1].is_empty(), "length-1 key is a marker");
        assert_eq!(by_key[&inc2].len(), 8, "L=2 records stride 8");
        assert_eq!(by_key[&comp2].len(), 8);
        // The complete-space length-1 prefix is the tone-zeroed first
        // syllable — a marker unless another row stores it.
        let comp1 = key(23, 0, 1).to_packed().to_le_bytes().to_vec();
        assert!(by_key[&comp1].is_empty());
    }

    #[test]
    fn pinyin_index_records_sort_by_exact_compare2_then_token() {
        // Two rows sharing a complete key: record order is exact_compare2
        // (all equal here) then token ascending.
        let rows = vec![
            ParsedRow {
                token: 0x0100_0009,
                keys: vec![key(5, 0, 3)],
            },
            ParsedRow {
                token: 0x0100_0002,
                keys: vec![key(5, 0, 3)],
            },
        ];
        let entries = pinyin_index_entries(&rows);
        let full_key = key(5, 0, 3).to_packed().to_le_bytes().to_vec();
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
