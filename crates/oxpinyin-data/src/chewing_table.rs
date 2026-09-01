//! Direct reader for libpinyin's `pinyin_index.bin` DBM — the Rust
//! equivalent of `ChewingLargeTable2`.
//!
//! libpinyin stores the pinyin index as a backend DBM (KC TreeDB or Tkrzw
//! TreeDBM) with two key spaces sharing one file:
//!
//! - **Complete index:** key = packed `ChewingKey[L]` with every tone
//!   zeroed, value = `PinyinIndexItem2<L>[]` with the original tones.
//! - **Incomplete (initial) index:** key = packed initial-only
//!   `ChewingKey[L]`, value = `PinyinIndexItem2<L>[]`.
//!
//! Every shorter prefix of a stored key has an **empty-value marker**
//! entry — that is `SEARCH_CONTINUED`: a probe for a prefix answers "yes"
//! when the DBM contains that key at all (empty or not).
//!
//! See `docs/findings/libpinyin-system-data-formats-2026-09-01.md` §1.3 and
//! `chewing_large_table2_tkrzwdb.cpp:133-296`.

use oxpinyin_core::ChewingKey;

use crate::dict::DictError;

// ── PinyinIndexItem2 stride ──────────────────────────────────────

/// The C++ `sizeof(PinyinIndexItem2<L>)` with tail padding to 4-byte
/// alignment. Field sum is `4 + 2*L`; C++ rounds up to the next multiple
/// of 4 (the `u32 token` field's alignment).
///
/// ```text
/// L=1: 4+2 = 6 → 8
/// L=2: 4+4 = 8 → 8
/// L=3: 4+6 = 10 → 12
/// L=4: 4+8 = 12 → 12
/// ```
#[must_use]
pub(crate) const fn item2_stride(phrase_length: usize) -> usize {
    let raw = 4 + 2 * phrase_length;
    (raw + 3) & !3
}

// ── Key encoding ─────────────────────────────────────────────────

/// Packs a `ChewingKey` slice into the DBM key for the **complete** index:
/// every tone zeroed, each key as 2 LE bytes.
///
/// `chewing_large_table2_tkrzwdb.cpp:221-232` (`search`): zeroes every
/// key's tone, then encodes the array as the lookup key.
#[must_use]
pub(crate) fn encode_complete_key(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for key in keys {
        let zeroed = ChewingKey::new(key.initial, key.middle, key.final_, 0);
        buf.extend_from_slice(&zeroed.to_packed().to_le_bytes());
    }
    buf
}

/// Packs a `ChewingKey` slice into the DBM key for the **incomplete**
/// (initial-only) index: each key reduced to its `m_initial` only
/// (middle, final, tone all zero), as 2 LE bytes.
///
/// `pinyin_phrase3.h:160-177`: the two key spaces coexist in one DBM;
/// the incomplete key space uses only the initial bits.
#[must_use]
pub(crate) fn encode_incomplete_key(keys: &[ChewingKey]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(keys.len() * 2);
    for key in keys {
        let initial_only = ChewingKey::new(key.initial, 0, 0, 0);
        buf.extend_from_slice(&initial_only.to_packed().to_le_bytes());
    }
    buf
}

// ── Value decoding ───────────────────────────────────────────────

/// One decoded `PinyinIndexItem2<L>`: a phrase token and its stored
/// pronunciation keys (with original tones preserved).
#[derive(Clone, Debug)]
pub(crate) struct PinyinIndexItem {
    pub(crate) token: u32,
    pub(crate) keys: Vec<ChewingKey>,
}

/// Decodes a DBM value into `PinyinIndexItem2<L>` records.
///
/// The value is a packed array of C++ structs with stride
/// [`item2_stride`]`(phrase_length)`. Each record contains:
/// - `u32 token` (LE, offset 0)
/// - `ChewingKey keys[phrase_length]` (2 bytes each, offset 4)
/// - padding to the stride
///
/// Returns an empty Vec for an empty value (a prefix marker).
///
/// # Errors
///
/// Returns `DictError::Parse` if the value length is not a multiple of
/// the stride, or if any record is too short to contain its fields.
pub(crate) fn decode_items(
    value: &[u8],
    phrase_length: usize,
) -> Result<Vec<PinyinIndexItem>, DictError> {
    if value.is_empty() {
        return Ok(Vec::new());
    }
    if phrase_length == 0 {
        return Err(DictError::Parse(
            "phrase_length must be at least 1".to_owned(),
        ));
    }
    let stride = item2_stride(phrase_length);
    if !value.len().is_multiple_of(stride) {
        return Err(DictError::Parse(format!(
            "pinyin index value length {} is not a multiple of stride {} (L={})",
            value.len(),
            stride,
            phrase_length,
        )));
    }
    let count = value.len() / stride;
    let mut items = Vec::with_capacity(count);
    for i in 0..count {
        let base = i * stride;
        let token = u32::from_le_bytes([
            value[base],
            value[base + 1],
            value[base + 2],
            value[base + 3],
        ]);
        let mut keys = Vec::with_capacity(phrase_length);
        for j in 0..phrase_length {
            let key_offset = base + 4 + j * 2;
            if key_offset + 2 > value.len() {
                return Err(DictError::Parse(format!(
                    "record {i} truncated at key {j} (offset {key_offset}, value len {})",
                    value.len(),
                )));
            }
            let packed = u16::from_le_bytes([value[key_offset], value[key_offset + 1]]);
            keys.push(ChewingKey::from_packed(packed));
        }
        items.push(PinyinIndexItem { token, keys });
    }
    Ok(items)
}

/// Encodes a single `PinyinIndexItem2<L>` record into its C++ ABI form.
pub(crate) fn encode_item(item: &PinyinIndexItem) -> Vec<u8> {
    let phrase_length = item.keys.len();
    let stride = item2_stride(phrase_length);
    let mut buf = vec![0u8; stride];
    buf[..4].copy_from_slice(&item.token.to_le_bytes());
    for (j, key) in item.keys.iter().enumerate() {
        let packed = key.to_packed().to_le_bytes();
        buf[4 + j * 2] = packed[0];
        buf[4 + j * 2 + 1] = packed[1];
    }
    buf
}

/// Encodes a slice of `PinyinIndexItem` records into a DBM value.
pub(crate) fn encode_items(items: &[PinyinIndexItem]) -> Vec<u8> {
    let mut buf = Vec::new();
    for item in items {
        buf.extend_from_slice(&encode_item(item));
    }
    buf
}

// ── ChewingTable ─────────────────────────────────────────────────

/// An abstraction over the DBM access method, so the ChewingTable can
/// read both libpinyin's raw DBM files (KC/Tkrzw with no table framing)
/// and oxpinyin's store-backed files (redb/LMDB with table framing).
pub(crate) trait ChewingDbm {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError>;
}

/// Wraps a [`RawReadStore`](oxpinyin_store::RawReadStore) for raw
/// (unframed) DBM access — the mode libpinyin's pinyin_index.bin needs.
///
/// KC and Tkrzw backends skip table framing and hand the key straight
/// to the underlying library, matching libpinyin's flat keyspace. redb
/// and LMDB delegate to the well-known `"data"` table.
pub(crate) struct RawChewingDbm<S> {
    store: S,
}

impl<S> RawChewingDbm<S> {
    pub(crate) fn new(store: S) -> Self {
        Self { store }
    }
}

impl<S: oxpinyin_store::RawReadStore + Send + Sync> ChewingDbm for RawChewingDbm<S> {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError> {
        self.store
            .get_raw(key)
            .map_err(|e| DictError::Table(e.into()))
    }
}

/// `SEARCH_OK | SEARCH_CONTINUED` as a bitset, matching libpinyin's
/// `SearchResult` enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SearchResult(u8);

impl SearchResult {
    pub(crate) const NONE: Self = Self(0x00);
    pub(crate) const OK: Self = Self(0x01);
    pub(crate) const CONTINUED: Self = Self(0x02);

    #[must_use]
    pub(crate) const fn has_ok(self) -> bool {
        self.0 & 0x01 != 0
    }

    #[must_use]
    pub(crate) const fn has_continued(self) -> bool {
        self.0 & 0x02 != 0
    }

    #[must_use]
    pub(crate) const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// The Rust equivalent of `ChewingLargeTable2`: a lazy, read-only view
/// over a pinyin-index DBM.
///
/// Does not materialize the entire index at open time. Lookups are
/// point reads against the DBM backend.
pub(crate) struct ChewingTable {
    dbm: Box<dyn ChewingDbm + Send + Sync>,
}

impl ChewingTable {
    pub(crate) fn new(dbm: Box<dyn ChewingDbm + Send + Sync>) -> Self {
        Self { dbm }
    }

    /// Looks up `keys` in the **complete** index (tone-zeroed).
    ///
    /// Port of `ChewingLargeTable2::search_internal`
    /// (`chewing_large_table2_tkrzwdb.cpp:133-162`):
    ///
    /// - Key not found → `SEARCH_NONE`
    /// - Key found, empty value → `SEARCH_CONTINUED` (prefix marker)
    /// - Key found, non-empty, tone matches → `SEARCH_OK | SEARCH_CONTINUED`
    /// - Key found, non-empty, no tone match → `SEARCH_CONTINUED`
    ///
    /// `SEARCH_CONTINUED` is set whenever the key exists. The prefix-
    /// marker scheme guarantees that a key at length L exists only when
    /// a real entry at length >= L exists.
    pub(crate) fn search(
        &self,
        keys: &[ChewingKey],
    ) -> Result<(SearchResult, Vec<PinyinIndexItem>), DictError> {
        if keys.is_empty() {
            return Ok((SearchResult::NONE, Vec::new()));
        }
        self.search_internal(&encode_complete_key(keys), keys)
    }

    /// Looks up `keys` in the **incomplete** (initial-only) index.
    pub(crate) fn search_incomplete(
        &self,
        keys: &[ChewingKey],
    ) -> Result<(SearchResult, Vec<PinyinIndexItem>), DictError> {
        if keys.is_empty() {
            return Ok((SearchResult::NONE, Vec::new()));
        }
        self.search_internal(&encode_incomplete_key(keys), keys)
    }

    fn search_internal(
        &self,
        encoded: &[u8],
        query_keys: &[ChewingKey],
    ) -> Result<(SearchResult, Vec<PinyinIndexItem>), DictError> {
        let value = match self.dbm.get(encoded)? {
            Some(v) => v,
            None => return Ok((SearchResult::NONE, Vec::new())),
        };

        let mut result = SearchResult::CONTINUED;

        if value.is_empty() {
            return Ok((result, Vec::new()));
        }

        let phrase_length = query_keys.len();
        let all_items = decode_items(&value, phrase_length)?;

        let items: Vec<PinyinIndexItem> = if query_keys.iter().all(|k| k.tone == 0) {
            all_items
        } else {
            all_items
                .into_iter()
                .filter(|item| tones_match(query_keys, &item.keys))
                .collect()
        };

        if !items.is_empty() {
            result = result.union(SearchResult::OK);
        }

        Ok((result, items))
    }
}

/// Checks whether the stored tones in `stored` match the queried tones
/// in `query`. A zero tone in the query is a wildcard (matches any).
fn tones_match(query: &[ChewingKey], stored: &[ChewingKey]) -> bool {
    if query.len() != stored.len() {
        return false;
    }
    for (q, s) in query.iter().zip(stored.iter()) {
        if q.tone != 0 && q.tone != s.tone {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn item2_stride_matches_upstream_sizeof() {
        assert_eq!(item2_stride(1), 8, "L=1: 4+2=6 padded to 8");
        assert_eq!(item2_stride(2), 8, "L=2: 4+4=8 no padding");
        assert_eq!(item2_stride(3), 12, "L=3: 4+6=10 padded to 12");
        assert_eq!(item2_stride(4), 12, "L=4: 4+8=12 no padding");
        assert_eq!(item2_stride(5), 16, "L=5: 4+10=14 padded to 16");
        assert_eq!(item2_stride(6), 16, "L=6: 4+12=16 no padding");
        assert_eq!(item2_stride(16), 36, "L=16: max libpinyin phrase length");
    }

    #[test]
    fn encode_complete_key_zeroes_tone() {
        let keys = [ChewingKey::new(1, 0, 2, 3)];
        let encoded = encode_complete_key(&keys);
        assert_eq!(encoded.len(), 2);
        let expected = ChewingKey::new(1, 0, 2, 0).to_packed().to_le_bytes();
        assert_eq!(encoded, expected);
    }

    #[test]
    fn encode_incomplete_key_keeps_only_initial() {
        let keys = [ChewingKey::new(5, 1, 3, 2)];
        let encoded = encode_incomplete_key(&keys);
        assert_eq!(encoded.len(), 2);
        let expected = ChewingKey::new(5, 0, 0, 0).to_packed().to_le_bytes();
        assert_eq!(encoded, expected);
    }

    #[test]
    fn encode_multi_syllable_key() {
        let keys = [ChewingKey::new(1, 0, 2, 0), ChewingKey::new(3, 1, 4, 0)];
        let encoded = encode_complete_key(&keys);
        assert_eq!(encoded.len(), 4);
        let k0 = ChewingKey::new(1, 0, 2, 0).to_packed().to_le_bytes();
        let k1 = ChewingKey::new(3, 1, 4, 0).to_packed().to_le_bytes();
        assert_eq!(&encoded[0..2], &k0);
        assert_eq!(&encoded[2..4], &k1);
    }

    #[test]
    fn round_trip_item_encoding() {
        let item = PinyinIndexItem {
            token: 0x01000042,
            keys: vec![ChewingKey::new(2, 0, 3, 1)],
        };
        let encoded = encode_item(&item);
        assert_eq!(encoded.len(), item2_stride(1));
        let decoded = decode_items(&encoded, 1).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].token, 0x01000042);
        assert_eq!(decoded[0].keys.len(), 1);
        assert_eq!(decoded[0].keys[0], ChewingKey::new(2, 0, 3, 1));
    }

    #[test]
    fn round_trip_multi_record_value() {
        let items = vec![
            PinyinIndexItem {
                token: 0x01000001,
                keys: vec![ChewingKey::new(1, 0, 2, 1)],
            },
            PinyinIndexItem {
                token: 0x01000002,
                keys: vec![ChewingKey::new(1, 0, 2, 3)],
            },
        ];
        let encoded = encode_items(&items);
        assert_eq!(encoded.len(), item2_stride(1) * 2);
        let decoded = decode_items(&encoded, 1).unwrap();
        assert_eq!(decoded.len(), 2);
        assert_eq!(decoded[0].token, 0x01000001);
        assert_eq!(decoded[1].token, 0x01000002);
    }

    #[test]
    fn round_trip_multi_key_item() {
        let item = PinyinIndexItem {
            token: 0x01000099,
            keys: vec![
                ChewingKey::new(14, 0, 7, 3), // ni3
                ChewingKey::new(8, 0, 2, 3),  // hao3
            ],
        };
        let encoded = encode_item(&item);
        assert_eq!(encoded.len(), item2_stride(2));
        let decoded = decode_items(&encoded, 2).unwrap();
        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].token, 0x01000099);
        assert_eq!(decoded[0].keys[0], ChewingKey::new(14, 0, 7, 3));
        assert_eq!(decoded[0].keys[1], ChewingKey::new(8, 0, 2, 3));
    }

    #[test]
    fn decode_empty_value_is_prefix_marker() {
        let decoded = decode_items(&[], 1).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn decode_rejects_invalid_length() {
        let result = decode_items(&[0; 7], 1);
        assert!(result.is_err());
    }

    #[test]
    fn decode_rejects_zero_phrase_length() {
        let result = decode_items(&[0; 4], 0);
        assert!(result.is_err());
    }

    #[test]
    fn tones_match_wildcard() {
        let query = [ChewingKey::new(1, 0, 2, 0)];
        let stored = [ChewingKey::new(1, 0, 2, 3)];
        assert!(tones_match(&query, &stored));
    }

    #[test]
    fn tones_match_exact() {
        let query = [ChewingKey::new(1, 0, 2, 3)];
        let stored = [ChewingKey::new(1, 0, 2, 3)];
        assert!(tones_match(&query, &stored));
    }

    #[test]
    fn tones_mismatch() {
        let query = [ChewingKey::new(1, 0, 2, 3)];
        let stored = [ChewingKey::new(1, 0, 2, 1)];
        assert!(!tones_match(&query, &stored));
    }

    #[test]
    fn search_result_bitset() {
        let none = SearchResult::NONE;
        assert!(!none.has_ok());
        assert!(!none.has_continued());

        let ok = SearchResult::OK;
        assert!(ok.has_ok());
        assert!(!ok.has_continued());

        let both = SearchResult::OK.union(SearchResult::CONTINUED);
        assert!(both.has_ok());
        assert!(both.has_continued());
    }

    // ── In-memory DBM for unit testing ───────────────────────────

    use std::collections::BTreeMap;
    use std::sync::Mutex;

    struct MemoryDbm {
        data: Mutex<BTreeMap<Vec<u8>, Vec<u8>>>,
    }

    impl MemoryDbm {
        fn new() -> Self {
            Self {
                data: Mutex::new(BTreeMap::new()),
            }
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) {
            self.data.lock().unwrap().insert(key, value);
        }
    }

    impl ChewingDbm for MemoryDbm {
        fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError> {
            Ok(self.data.lock().unwrap().get(key).cloned())
        }
    }

    fn table_with_ba() -> ChewingTable {
        let dbm = MemoryDbm::new();
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let ba_item = PinyinIndexItem {
            token: 0x01000001,
            keys: vec![ba.with_tone(1)],
        };
        let key = encode_complete_key(&[ba]);
        let value = encode_items(&[ba_item]);
        dbm.put(key, value);
        let ikey = encode_incomplete_key(&[ba]);
        let ivalue = encode_items(&[PinyinIndexItem {
            token: 0x01000001,
            keys: vec![ChewingKey::new(ba.initial, 0, 0, 0)],
        }]);
        dbm.put(ikey, ivalue);
        ChewingTable::new(Box::new(dbm))
    }

    #[test]
    fn search_finds_single_syllable() {
        let table = table_with_ba();
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let (result, items) = table.search(&[ba]).unwrap();
        assert!(result.has_ok());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].token, 0x01000001);
    }

    #[test]
    fn search_filters_by_tone() {
        let dbm = MemoryDbm::new();
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let ba1 = PinyinIndexItem {
            token: 0x01000001,
            keys: vec![ba.with_tone(1)],
        };
        let ba3 = PinyinIndexItem {
            token: 0x01000002,
            keys: vec![ba.with_tone(3)],
        };
        let key = encode_complete_key(&[ba]);
        dbm.put(key, encode_items(&[ba1, ba3]));
        let table = ChewingTable::new(Box::new(dbm));

        let (result, items) = table.search(&[ba.with_tone(1)]).unwrap();
        assert!(result.has_ok());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].token, 0x01000001);

        let (result, items) = table.search(&[ba.with_tone(3)]).unwrap();
        assert!(result.has_ok());
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].token, 0x01000002);

        let (result, items) = table.search(&[ba]).unwrap();
        assert!(result.has_ok());
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn search_miss_returns_none() {
        let table = table_with_ba();
        let zhong = ChewingKey::from_pinyin("zhong").unwrap();
        let (result, items) = table.search(&[zhong]).unwrap();
        assert!(!result.has_ok());
        assert!(!result.has_continued());
        assert!(items.is_empty());
    }

    #[test]
    fn search_prefix_marker_returns_continued() {
        let dbm = MemoryDbm::new();
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let hao = ChewingKey::from_pinyin("hao").unwrap();

        let key2 = encode_complete_key(&[ni, hao]);
        let item = PinyinIndexItem {
            token: 0x01000099,
            keys: vec![ni.with_tone(3), hao.with_tone(3)],
        };
        dbm.put(key2, encode_items(&[item]));
        let key1 = encode_complete_key(&[ni]);
        dbm.put(key1, Vec::new());

        let table = ChewingTable::new(Box::new(dbm));

        let (result, items) = table.search(&[ni]).unwrap();
        assert!(!result.has_ok());
        assert!(result.has_continued());
        assert!(items.is_empty());

        let (result, items) = table.search(&[ni, hao]).unwrap();
        assert!(result.has_ok());
        assert!(result.has_continued());
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn search_empty_keys_returns_none() {
        let table = table_with_ba();
        let (result, items) = table.search(&[]).unwrap();
        assert!(!result.has_ok());
        assert!(items.is_empty());
    }

    #[test]
    fn incomplete_search_uses_initial_only_key() {
        let table = table_with_ba();
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let (result, items) = table.search_incomplete(&[ba]).unwrap();
        assert!(result.has_ok());
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn malformed_value_does_not_panic() {
        let dbm = MemoryDbm::new();
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let key = encode_complete_key(&[ba]);
        dbm.put(key, vec![0xFF; 5]);
        let table = ChewingTable::new(Box::new(dbm));
        let result = table.search(&[ba]);
        assert!(result.is_err());
    }

    #[test]
    fn truncated_value_does_not_panic() {
        let dbm = MemoryDbm::new();
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let hao = ChewingKey::from_pinyin("hao").unwrap();
        let key = encode_complete_key(&[ni, hao]);
        dbm.put(key, vec![0x01, 0x00, 0x00, 0x00, 0x0E, 0x00]);
        let table = ChewingTable::new(Box::new(dbm));
        let result = table.search(&[ni, hao]);
        assert!(result.is_err());
    }
}
