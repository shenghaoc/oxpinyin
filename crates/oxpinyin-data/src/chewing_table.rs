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
#[cfg(test)]
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
#[cfg(test)]
pub(crate) fn encode_items(items: &[PinyinIndexItem]) -> Vec<u8> {
    let mut buf = Vec::new();
    for item in items {
        buf.extend_from_slice(&encode_item(item));
    }
    buf
}

// ── ChewingTable ─────────────────────────────────────────────────

/// The callback a [`ChewingDbm::walk`] hands each `(key, value)` row to.
pub(crate) type RowVisitor<'a> = dyn FnMut(&[u8], &[u8]) -> Result<(), DictError> + 'a;

/// The callback [`ChewingTable::walk_extensions`] hands each non-empty
/// extension row's `(syllable count, records)` to; `Ok(true)` stops the
/// walk.
pub(crate) type ExtensionVisitor<'a> =
    dyn FnMut(usize, &[PinyinIndexItem]) -> Result<bool, DictError> + 'a;

/// An abstraction over the DBM access method, so the ChewingTable can
/// read both libpinyin's raw DBM files (KC/Tkrzw with no table framing)
/// and oxpinyin's store-backed files (redb/LMDB with table framing).
pub(crate) trait ChewingDbm {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, DictError>;

    /// Every row whose key lies in `[lo, hi)` (`hi = None` — to the end),
    /// in ascending key-byte order — the cursor walk upstream's
    /// `search_suggestion` performs (`cursor->jump` + `step`).
    fn walk(
        &self,
        lo: &[u8],
        hi: Option<&[u8]>,
        visit: &mut RowVisitor<'_>,
    ) -> Result<(), DictError>;
}

/// The smallest key-byte string above every extension of `prefix` — the
/// exclusive upper bound of the range `[prefix, …)` that holds exactly the
/// keys starting with `prefix`. `None` when no such bound exists (every
/// byte is `0xFF`), which callers read as "to the end".
#[must_use]
pub(crate) fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
    let last = prefix.iter().rposition(|&byte| byte != 0xFF)?;
    let mut bound = prefix[..=last].to_vec();
    bound[last] = bound[last].wrapping_add(1);
    Some(bound)
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

    fn walk(
        &self,
        lo: &[u8],
        hi: Option<&[u8]>,
        visit: &mut RowVisitor<'_>,
    ) -> Result<(), DictError> {
        use std::ops::Bound;
        // The store's visitor speaks `StoreError`; collect the bounded
        // range first (every caller bounds it to one key's extensions)
        // and hand the rows over outside the store's callback.
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
        self.store
            .range_raw(
                Bound::Included(lo),
                hi.map_or(Bound::Unbounded, Bound::Excluded),
                &mut |key, value| {
                    rows.push((key.to_vec(), value.to_vec()));
                    Ok(())
                },
            )
            .map_err(|e| DictError::Table(e.into()))?;
        for (key, value) in &rows {
            visit(key, value)?;
        }
        Ok(())
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

    /// `ChewingLargeTable2::search` (`chewing_large_table2_kyotodb.cpp`)
    /// with `ChewingTableEntry::search`'s record filter
    /// (`chewing_large_table2.h`):
    ///
    /// * the DBM key is the query's **incomplete** index (initials only)
    ///   when any query syllable is incomplete
    ///   (`contains_incomplete_pinyin`), else its tone-zeroed
    ///   **complete** index;
    /// * key absent → `SEARCH_NONE`; present → `SEARCH_CONTINUED`;
    /// * a non-empty value's `PinyinIndexItem2` records are kept when
    ///   `pinyin_compare_with_tones(query, stored) == 0`
    ///   ([`keys_match`]): initials equal, middle/final equal unless
    ///   either side is incomplete, tones equal unless either side is
    ///   zero — so a tone-less query accepts every tone and an incomplete
    ///   syllable accepts every final;
    /// * the surviving records, empty when the key is absent, an empty
    ///   marker, or no record matches. Upstream additionally requires the
    ///   record's library to be loaded; the dictionary layer applies that
    ///   when it resolves tokens. Use [`Self::key_exists`] for the
    ///   `SEARCH_CONTINUED` prefix probe, which needs no record decode.
    pub(crate) fn search(&self, keys: &[ChewingKey]) -> Result<Vec<PinyinIndexItem>, DictError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        let Some(value) = self.dbm.get(&index_key(keys))? else {
            return Ok(Vec::new());
        };
        if value.is_empty() {
            return Ok(Vec::new());
        }
        Ok(decode_items(&value, keys.len())?
            .into_iter()
            .filter(|item| keys_match(keys, &item.keys))
            .collect())
    }

    /// Whether the query's index key exists at all — `SEARCH_CONTINUED`
    /// without the record decode, for the widen probe.
    pub(crate) fn key_exists(&self, keys: &[ChewingKey]) -> Result<bool, DictError> {
        if keys.is_empty() {
            return Ok(false);
        }
        Ok(self.dbm.get(&index_key(keys))?.is_some())
    }

    /// Every row whose key extends the query's index key (the key itself
    /// included), in ascending key order — the rows a longer phrase with
    /// this pinyin prefix can live under. Each row is handed over as
    /// `(syllable count, records)`; empty markers are skipped.
    ///
    /// This is the widen probe's visibility walk: upstream's
    /// `SEARCH_CONTINUED` says only that *some* longer key exists, and a
    /// caller that must know whether a *visible* phrase extends the
    /// prefix has to read the extensions.
    pub(crate) fn walk_extensions(
        &self,
        keys: &[ChewingKey],
        visit: &mut ExtensionVisitor<'_>,
    ) -> Result<bool, DictError> {
        if keys.is_empty() {
            return Ok(false);
        }
        let encoded = index_key(keys);
        let upper = prefix_upper_bound(&encoded);
        let mut found = false;
        self.dbm
            .walk(&encoded, upper.as_deref(), &mut |key, value| {
                if found || value.is_empty() || !key.len().is_multiple_of(2) {
                    return Ok(());
                }
                let phrase_length = key.len() / 2;
                let items = decode_items(value, phrase_length)?;
                if visit(phrase_length, &items)? {
                    found = true;
                }
                Ok(())
            })?;
        Ok(found)
    }
}

/// `contains_incomplete_pinyin` (`pinyin_phrase3.h:146`): a syllable with
/// neither middle nor final is an initial-only (incomplete) key.
#[must_use]
pub(crate) fn contains_incomplete(keys: &[ChewingKey]) -> bool {
    keys.iter().any(|key| key.middle == 0 && key.final_ == 0)
}

/// The DBM key `ChewingLargeTable2::search` computes for a query.
#[must_use]
pub(crate) fn index_key(keys: &[ChewingKey]) -> Vec<u8> {
    if contains_incomplete(keys) {
        encode_incomplete_key(keys)
    } else {
        encode_complete_key(keys)
    }
}

/// `pinyin_compare_with_tones(query, stored, len) == 0`
/// (`pinyin_phrase3.h:68-115`), syllable by syllable:
///
/// * `pinyin_compare_initial3`: initials must be equal;
/// * `pinyin_compare_middle_and_final3`: equal middle and final, or
///   either side has neither (an incomplete syllable matches any);
/// * `pinyin_compare_tone3`: equal tones, or either side is zero.
///
/// Upstream compares all initials, then all middle/finals, then all
/// tones, and returns at the first difference; equality is order
/// independent, so this checks per syllable.
#[must_use]
pub(crate) fn keys_match(query: &[ChewingKey], stored: &[ChewingKey]) -> bool {
    query.len() == stored.len() && prefix_keys_match(query, stored)
}

/// [`keys_match`] over the first `query.len()` syllables of `stored` —
/// `prefix_compare_with_tones` (`PrefixLessThanWithTones`,
/// `chewing_large_table2.h`), the comparison the suggestion path applies
/// to longer records.
#[must_use]
pub(crate) fn prefix_keys_match(query: &[ChewingKey], stored: &[ChewingKey]) -> bool {
    if stored.len() < query.len() {
        return false;
    }
    query.iter().zip(stored).all(|(q, s)| {
        q.initial == s.initial
            && ((q.middle == s.middle && q.final_ == s.final_)
                || (q.middle == 0 && q.final_ == 0)
                || (s.middle == 0 && s.final_ == 0))
            && (q.tone == s.tone || q.tone == 0 || s.tone == 0)
    })
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

        fn walk(
            &self,
            lo: &[u8],
            hi: Option<&[u8]>,
            visit: &mut RowVisitor<'_>,
        ) -> Result<(), DictError> {
            let data = self.data.lock().unwrap();
            for (key, value) in data.range(lo.to_vec()..) {
                if hi.is_some_and(|hi| key.as_slice() >= hi) {
                    break;
                }
                visit(key, value)?;
            }
            Ok(())
        }
    }

    #[test]
    fn prefix_upper_bound_is_the_next_sibling() {
        assert_eq!(prefix_upper_bound(&[1, 2]), Some(vec![1, 3]));
        assert_eq!(prefix_upper_bound(&[1, 0xFF]), Some(vec![2]));
        assert_eq!(prefix_upper_bound(&[0xFF, 0xFF]), None);
        assert_eq!(prefix_upper_bound(&[]), None);
    }

    #[test]
    fn keys_match_follows_the_three_upstream_comparisons() {
        let ni3 = ChewingKey::from_pinyin("ni").unwrap().with_tone(3);
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let n = ChewingKey::new(ni.initial, 0, 0, 0);
        let na = ChewingKey::from_pinyin("na").unwrap();
        assert!(keys_match(&[ni], &[ni3]), "zero tone matches any tone");
        assert!(keys_match(&[ni3], &[ni]), "either side zero");
        assert!(!keys_match(&[ni3.with_tone(1)], &[ni3]), "tones differ");
        assert!(
            keys_match(&[n], &[ni3]),
            "incomplete query matches any final"
        );
        assert!(keys_match(&[ni3], &[n]), "incomplete stored matches too");
        assert!(!keys_match(&[na], &[ni3]), "finals differ");
        assert!(!keys_match(&[ni, ni], &[ni3]), "length differs");
        assert!(prefix_keys_match(&[ni], &[ni3, na]));
        assert!(!prefix_keys_match(&[ni, ni], &[ni3]));
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
        let items = table.search(&[ba]).unwrap();
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

        let items = table.search(&[ba.with_tone(1)]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].token, 0x01000001);

        let items = table.search(&[ba.with_tone(3)]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].token, 0x01000002);

        let items = table.search(&[ba]).unwrap();
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn search_miss_returns_none() {
        let table = table_with_ba();
        let zhong = ChewingKey::from_pinyin("zhong").unwrap();
        let items = table.search(&[zhong]).unwrap();
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

        let items = table.search(&[ni]).unwrap();
        assert!(items.is_empty());

        let items = table.search(&[ni, hao]).unwrap();
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn search_empty_keys_returns_none() {
        let table = table_with_ba();
        let items = table.search(&[]).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn an_incomplete_query_uses_the_initial_only_key_and_filters_by_the_query() {
        // Upstream stores the full keys in both key spaces; the incomplete
        // space's records are told apart by `pinyin_compare_with_tones`.
        let dbm = MemoryDbm::new();
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let bo = ChewingKey::from_pinyin("bo").unwrap();
        let b = ChewingKey::new(ba.initial, 0, 0, 0);
        dbm.put(
            encode_incomplete_key(&[b]),
            encode_items(&[
                PinyinIndexItem {
                    token: 0x01000001,
                    keys: vec![ba.with_tone(1)],
                },
                PinyinIndexItem {
                    token: 0x01000002,
                    keys: vec![bo.with_tone(2)],
                },
            ]),
        );
        let table = ChewingTable::new(Box::new(dbm));

        // The initial alone: both records.
        let items = table.search(&[b]).unwrap();
        assert_eq!(items.len(), 2);

        // A complete query goes to the complete space, which this fixture
        // does not carry.
        let items = table.search(&[ba]).unwrap();
        assert!(items.is_empty());
    }

    #[test]
    fn a_mixed_query_filters_the_complete_syllables_inside_the_incomplete_space() {
        let dbm = MemoryDbm::new();
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let na = ChewingKey::from_pinyin("na").unwrap();
        let hao = ChewingKey::from_pinyin("hao").unwrap();
        let h = ChewingKey::new(hao.initial, 0, 0, 0);
        let n = ChewingKey::new(ni.initial, 0, 0, 0);
        dbm.put(
            encode_incomplete_key(&[n, h]),
            encode_items(&[
                PinyinIndexItem {
                    token: 0x01000099,
                    keys: vec![ni.with_tone(3), hao.with_tone(3)],
                },
                PinyinIndexItem {
                    token: 0x01000098,
                    keys: vec![na.with_tone(4), hao.with_tone(3)],
                },
            ]),
        );
        let table = ChewingTable::new(Box::new(dbm));
        // "ni" + "h": the incomplete-space key `n h`, records filtered to
        // those whose first syllable is `ni`.
        let items = table.search(&[ni, h]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].token, 0x01000099);
    }

    #[test]
    fn walk_extensions_visits_the_longer_keys_in_the_same_space() {
        let dbm = MemoryDbm::new();
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let hao = ChewingKey::from_pinyin("hao").unwrap();
        let men = ChewingKey::from_pinyin("men").unwrap();
        dbm.put(encode_complete_key(&[ni]), Vec::new());
        dbm.put(
            encode_complete_key(&[ni, hao]),
            encode_items(&[PinyinIndexItem {
                token: 0x01000099,
                keys: vec![ni.with_tone(3), hao.with_tone(3)],
            }]),
        );
        dbm.put(
            encode_complete_key(&[ni, men]),
            encode_items(&[PinyinIndexItem {
                token: 0x02000001,
                keys: vec![ni.with_tone(3), men.with_tone(0)],
            }]),
        );
        // A neighbour outside the prefix range.
        dbm.put(
            encode_complete_key(&[hao]),
            encode_items(&[PinyinIndexItem {
                token: 0x01000011,
                keys: vec![hao.with_tone(3)],
            }]),
        );
        let table = ChewingTable::new(Box::new(dbm));
        let mut seen = Vec::new();
        let found = table
            .walk_extensions(&[ni], &mut |len, items| {
                seen.push((len, items[0].token));
                Ok(false)
            })
            .unwrap();
        assert!(!found);
        assert_eq!(seen.len(), 2, "two non-empty extensions, marker skipped");
        assert!(seen.iter().all(|(len, _)| *len == 2));
        // Early exit on the first accepted row.
        let found = table
            .walk_extensions(&[ni], &mut |_, items| Ok(items[0].token >> 24 == 2))
            .unwrap();
        assert!(found);
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
