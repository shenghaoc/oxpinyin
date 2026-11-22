//! Builds native index fixture rows.
//! [`crate::data::row_format`] defines the byte encodings.

// Fixture entry-set builders below use ChewingLargeTable2 and PhraseLargeTable3 prefix
// closure, and ChewingTableEntry::add_index orders identical keys by token.

#[cfg(test)]
use std::collections::BTreeMap;

use crate::parser::ChewingKey;

#[cfg(test)]
use crate::data::phrase_library::CHEWING_KEY_SIZE;
#[cfg(test)]
use crate::data::row_format::phrase_index::{
    encode_ucs4_key_scalars, test_encode_tokens as encode_tokens,
};
#[cfg(test)]
use crate::data::row_format::pinyin_index::{
    PinyinIndexItem, encode_complete_key, encode_incomplete_key, encode_item,
};

/// One DBM file's rows: `(key, value)` pairs in ascending key order.
#[cfg(test)]
pub type Entries = Vec<(Vec<u8>, Vec<u8>)>;

// pinyin_exact_compare2, pinyin_phrase3.h:33–66.
/// One keyspace's accumulated rows: packed key bytes → the records in
/// arrival order.
#[cfg(test)]
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
#[cfg(test)]
pub(crate) fn pinyin_index_entries(rows: &[PinyinIndexItem]) -> Entries {
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
        records.sort_by_cached_key(|row| {
            let initials: Vec<_> = row.keys.iter().map(|key| key.initial).collect();
            let finals: Vec<_> = row
                .keys
                .iter()
                .map(|key| (key.middle, key.final_))
                .collect();
            let tones: Vec<_> = row.keys.iter().map(|key| key.tone).collect();
            (initials, finals, tones, row.token)
        });
        let mut value = Vec::with_capacity(
            records.len()
                * crate::data::row_format::pinyin_index::item2_stride(
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
#[cfg(test)]
fn truncate_packed(key: &[u8], syllables: usize) -> Vec<u8> {
    key[..CHEWING_KEY_SIZE * syllables].to_vec()
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
#[cfg(test)]
pub(crate) fn phrase_index_entries(rows: &[(Vec<u32>, u32)]) -> Entries {
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
mod tests;
