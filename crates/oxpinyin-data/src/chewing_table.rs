//! Direct reader for libpinyin's `pinyin_index.bin` DBM — the Rust
//! equivalent of `ChewingLargeTable2`.
//!
//! libpinyin stores the pinyin index as a backend DBM (KC `TreeDB` or Tkrzw
//! `TreeDBM`) with two key spaces sharing one file:
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
//! See `docs/findings/data-formats.md` §4 and
//! `chewing_large_table2_tkrzwdb.cpp:133-296`.

use oxpinyin_core::ChewingKey;

use crate::dict::DictError;
// The key and record layouts this reader shares with
// `gen_binary_files`'s writer: one written copy, so a probe here and an
// emission there cannot drift (`crate::row_format`).
#[cfg(test)]
use crate::row_format::pinyin_index::encode_items;
pub use crate::row_format::pinyin_index::{
    PinyinIndexItem, decode_items, encode_complete_key, encode_incomplete_key,
};

// ── ChewingTable ─────────────────────────────────────────────────

/// The DBM access method behind the tables ([`ChewingTable`],
/// [`crate::PhraseTable`], [`crate::BigramTable`], [`crate::PunctTable`]):
/// each table is generic over it, so the one implementation of a table's
/// logic runs over the production [`DefaultDbm`] and over the in-memory,
/// counting and failing test doubles alike.
pub trait ChewingDbm {
    /// Whether this is the retained object of a failed system attach.
    fn attachment_failed(&self) -> bool {
        false
    }

    /// Performs the failed suggestion's native first call, returning
    /// whether the failed attachment handled the operation.
    fn failed_suggestion(&self, _key: &[u8]) -> bool {
        false
    }

    /// Closes the native attachment once, at facade fini: an open file or
    /// a retained failed object (`pinyin.cpp:1205-1215`, `zhuyin.cpp:744-748`
    /// at 074a2219). Every later read is a miss with no native call.
    fn finish_attachment(&mut self) {}

    /// Whether [`Self::finish_attachment`] has closed the native object.
    fn attachment_finished(&self) -> bool {
        false
    }

    /// Point read into `value`, a buffer the caller owns: `value` is
    /// cleared, then holds the stored bytes on a hit. Answers whether the
    /// key was found. A caller that passes one buffer to successive reads
    /// allocates only when a value outgrows it.
    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError>;

    /// Read one bigram row with the pin's checked-read shape. The default
    /// is [`Self::get`]; a backend whose library checks the size and then
    /// reads the body overrides it.
    fn get_bigram(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        self.get(key, value)
    }

    /// Read one pinyin-table row with the shape of
    /// `ChewingLargeTable2::search_internal`: every backend answers a failed
    /// first read as a miss (`chewing_large_table2_tkrzwdb.cpp:146-149`,
    /// `chewing_large_table2_bdb.cpp:182-184`,
    /// `chewing_large_table2_kyotodb.cpp:160-163` at 074a2219). Kyoto
    /// Cabinet's body read after a good size check aborts, as the pin's
    /// `check_result` does (`:170-174`). The default is [`Self::get`].
    fn get_pinyin_row(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        self.get(key, value)
    }

    /// Whether `key` exists, an empty marker included, with no value bytes
    /// copied — the `SEARCH_CONTINUED` probe and the suggestion walk's
    /// opening check (`m_db->check`, `phrase_large_table3_kyotodb.cpp:206`
    /// at 074a2219).
    fn contains(&self, key: &[u8]) -> Result<bool, DictError>;

    /// Every row whose key lies in `[lo, hi)` (`hi = None` — to the end),
    /// in ascending key-byte order — the cursor walk upstream's
    /// `search_suggestion` performs (`cursor->jump` + `step`).
    ///
    /// A visitor return of `Ok(true)` stops the walk: implementations
    /// must terminate the underlying scan there (not merely skip the
    /// remaining rows), and the walk returns `Ok(())`. A visitor `Err`
    /// ends the walk and comes back as itself.
    fn walk(
        &self,
        lo: &[u8],
        hi: Option<&[u8]>,
        visit: impl FnMut(&[u8], &[u8]) -> Result<bool, DictError>,
    ) -> Result<(), DictError>;
}

/// The smallest key-byte string above every extension of `prefix` — the
/// exclusive upper bound of the range `[prefix, …)` that holds exactly the
/// keys starting with `prefix`. `None` when no such bound exists (every
/// byte is `0xFF`), which callers read as "to the end".
#[must_use]
pub fn prefix_upper_bound(prefix: &[u8]) -> Option<Vec<u8>> {
    let last = prefix.iter().rposition(|&byte| byte != 0xFF)?;
    let mut bound = prefix[..=last].to_vec();
    bound[last] = bound[last].wrapping_add(1);
    Some(bound)
}

/// Wraps a [`RawReadStore`](oxpinyin_store::RawReadStore) for raw
/// (unframed) DBM access — the mode libpinyin's `pinyin_index.bin` needs.
///
/// KC and Tkrzw backends skip table framing and hand the key straight
/// to the underlying library, matching libpinyin's flat keyspace.
/// delegates to the well-known `"data"` table.
pub struct RawChewingDbm<S> {
    state: DbmState<S>,
}

enum DbmState<S> {
    Open(S),
    Unopened(oxpinyin_store::UnopenedSystemDbm),
    /// The open file after facade fini: the handle is dropped, which closes
    /// it and releases its lock. An instance or iterator that outlives the
    /// context reads misses here (compatibility row 42).
    Closed,
}

impl<S> RawChewingDbm<S> {
    pub(crate) const fn new(store: S) -> Self {
        Self {
            state: DbmState::Open(store),
        }
    }

    fn store(&self) -> Option<&S> {
        match &self.state {
            DbmState::Open(store) => Some(store),
            DbmState::Unopened(_) | DbmState::Closed => None,
        }
    }
}

/// The production [`ChewingDbm`]: the compiled-in backend read through
/// its raw (unframed) keyspace.
pub type DefaultDbm = RawChewingDbm<oxpinyin_store::DefaultStore>;

impl DefaultDbm {
    pub(crate) fn attach(path: &std::path::Path, hash: bool) -> Result<Self, DictError> {
        use oxpinyin_store::SystemAttachment;
        Ok(
            match SystemAttachment::open(path, hash).map_err(|e| DictError::Table(e.into()))? {
                SystemAttachment::Open(store) => Self::new(store),
                SystemAttachment::Unopened(db) => Self {
                    state: DbmState::Unopened(db),
                },
            },
        )
    }
}

/// A point read whose failure the pin answers as a miss: the value is
/// cleared and the key reads as absent. The one failure that stays an error
/// is a body read after a good size check, where the pin asserts.
fn failed_read_is_a_miss(
    read: Result<bool, oxpinyin_store::StoreError>,
    value: &mut Vec<u8>,
) -> Result<bool, DictError> {
    match read {
        // A failed record copy is a pin assert too (`tkrzwdb_utils.h:65`,
        // `:72`; `bdb_utils.h:61`, `:67`); a point read does not copy,
        // so it never reaches here.
        Err(
            error @ (oxpinyin_store::StoreError::ReadAfterCheck
            | oxpinyin_store::StoreError::FatalCopy),
        ) => Err(DictError::Table(error.into())),
        Err(
            oxpinyin_store::StoreError::Io(_)
            | oxpinyin_store::StoreError::Backend(_)
            | oxpinyin_store::StoreError::ReadOnly
            | oxpinyin_store::StoreError::InvalidInput(_)
            | oxpinyin_store::StoreError::ShortArrayHeader,
        ) => {
            value.clear();
            Ok(false)
        }
        Ok(found) => Ok(found),
    }
}

impl<S: oxpinyin_store::RawReadStore> ChewingDbm for RawChewingDbm<S> {
    fn attachment_failed(&self) -> bool {
        matches!(self.state, DbmState::Unopened(_))
    }

    fn failed_suggestion(&self, key: &[u8]) -> bool {
        match &self.state {
            DbmState::Open(_) | DbmState::Closed => {
                let _ = key;
                false
            }
            DbmState::Unopened(db) => {
                db.suggest(key);
                true
            }
        }
    }

    fn finish_attachment(&mut self) {
        match &self.state {
            // Dropping the store closes the native file.
            DbmState::Open(_) => self.state = DbmState::Closed,
            DbmState::Unopened(db) => db.finish(),
            DbmState::Closed => {}
        }
    }

    fn attachment_finished(&self) -> bool {
        match &self.state {
            DbmState::Open(_) => false,
            DbmState::Unopened(db) => db.is_finished(),
            DbmState::Closed => true,
        }
    }

    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        match &self.state {
            DbmState::Open(store) => store
                .get_raw(key, value)
                .map_err(|e| DictError::Table(e.into())),
            DbmState::Unopened(db) => {
                value.clear();
                db.read(key);
                Ok(false)
            }
            DbmState::Closed => {
                value.clear();
                Ok(false)
            }
        }
    }

    /// `Bigram::load` over the system gram: every backend answers a failed
    /// read with false and a NULL gram, and `search_bigram2` ignores it
    /// (`ngram_bdb.cpp:140-142`, `ngram_kyotodb.cpp:131-134`,
    /// `ngram_tkrzwdb.cpp:111-114`, `phrase_lookup.cpp:268-269` at
    /// 074a2219). The one failure that is not a miss is Kyoto Cabinet's
    /// body read after a good size check, the pin's `check_result` assert
    /// (`ngram_kyotodb.cpp:138`).
    fn get_bigram(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        match &self.state {
            DbmState::Open(store) => failed_read_is_a_miss(store.get_bigram_raw(key, value), value),
            DbmState::Unopened(db) => {
                value.clear();
                db.read(key);
                Ok(false)
            }
            DbmState::Closed => {
                value.clear();
                Ok(false)
            }
        }
    }

    fn get_pinyin_row(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        match &self.state {
            DbmState::Open(store) => match store.get_pinyin_row_raw(key, value) {
                // Kyoto Cabinet's body read after a good size check:
                // `check_result(vsiz == m_db->get(...))`, an assert on every
                // path through `search_internal`
                // (`chewing_large_table2_kyotodb.cpp:170-174` at 074a2219).
                Err(oxpinyin_store::StoreError::ReadAfterCheck) => std::process::abort(),
                read => failed_read_is_a_miss(read, value),
            },
            DbmState::Unopened(_) | DbmState::Closed => self.get(key, value),
        }
    }

    fn contains(&self, key: &[u8]) -> Result<bool, DictError> {
        match &self.state {
            DbmState::Open(store) => store
                .contains_raw(key)
                .map_err(|e| DictError::Table(e.into())),
            DbmState::Unopened(db) => {
                db.read(key);
                Ok(false)
            }
            DbmState::Closed => Ok(false),
        }
    }

    fn walk(
        &self,
        lo: &[u8],
        hi: Option<&[u8]>,
        mut visit: impl FnMut(&[u8], &[u8]) -> Result<bool, DictError>,
    ) -> Result<(), DictError> {
        use std::ops::{Bound, ControlFlow};
        let Some(store) = self.store() else {
            // Auxiliary Rust scans do not correspond to a pin read.
            // Suggestions dispatch their native first call separately.
            return Ok(());
        };
        // The store's visitor speaks `StoreError` and answers
        // continue/break, while the row visitor speaks `DictError` and
        // asks to stop: a stop is a `Break`, which ends the store scan
        // right there with no error; a visitor failure is parked here,
        // also ends the scan with a `Break`, and is returned as itself
        // once the scan is over. Rows are visited as the store hands
        // them over — no collect-then-replay buffer, and nothing past
        // the stop is scanned.
        let mut failure: Option<DictError> = None;
        let scan = store.scan_raw(
            Bound::Included(lo),
            hi.map_or(Bound::Unbounded, Bound::Excluded),
            |key, value| match visit(key, value) {
                Ok(true) => Ok(ControlFlow::Break(())),
                Ok(false) => Ok(ControlFlow::Continue(())),
                Err(e) => {
                    failure = Some(e);
                    Ok(ControlFlow::Break(()))
                }
            },
        );
        if let Some(e) = failure {
            return Err(e);
        }
        scan.map_err(|e| DictError::Table(e.into()))
    }
}

/// The Rust equivalent of `ChewingLargeTable2`: a lazy, read-only view
/// over a pinyin-index DBM.
///
/// Does not materialize the entire index at open time. Lookups are
/// point reads against the DBM backend.
pub struct ChewingTable<D = DefaultDbm> {
    dbm: D,
}

impl<D: ChewingDbm> ChewingTable<D> {
    pub(crate) const fn new(dbm: D) -> Self {
        Self { dbm }
    }

    pub(crate) fn attachment_failed(&self) -> bool {
        self.dbm.attachment_failed()
    }

    pub(crate) fn finish_attachment(&mut self) {
        self.dbm.finish_attachment();
    }

    pub(crate) fn attachment_finished(&self) -> bool {
        self.dbm.attachment_finished()
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
    ///   ([`ChewingKey::matches_with_tones`]): initials equal, middle/final equal unless
    ///   either side is incomplete, tones equal unless either side is
    ///   zero — so a tone-less query accepts every tone and an incomplete
    ///   syllable accepts every final;
    /// * the surviving tokens, empty when the key is absent, an empty
    ///   marker, or no record matches. Upstream additionally requires the
    ///   record's library to be loaded; the dictionary layer applies that
    ///   when it resolves tokens. Use [`Self::key_exists`] for the
    ///   `SEARCH_CONTINUED` prefix probe, which needs no record decode.
    pub(crate) fn search(&self, keys: &[ChewingKey]) -> Result<Vec<u32>, DictError> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.search_continued(keys, &mut Vec::new())?.1)
    }

    /// [`Self::search`] and [`Self::key_exists`] from one DBM get: whether
    /// the query's index key exists (an empty marker counts), and the
    /// surviving records' tokens, in stored order. The index key zeroes tones, so a toned query and
    /// its tone-less spelling name the same row.
    ///
    /// The value is read into `value`, the caller's buffer, which the
    /// decode loop passes to every search of a guess.
    pub(crate) fn search_continued(
        &self,
        keys: &[ChewingKey],
        value: &mut Vec<u8>,
    ) -> Result<(bool, Vec<u32>), DictError> {
        if keys.is_empty() {
            return Ok((false, Vec::new()));
        }
        let present = self.dbm.get_pinyin_row(&index_key(keys), value)?;
        search_row(keys, present.then_some(value.as_slice()))
    }

    /// Whether the query's index key exists at all — `SEARCH_CONTINUED`
    /// without the record decode or a value copy, for the widen probe.
    pub(crate) fn key_exists(&self, keys: &[ChewingKey]) -> Result<bool, DictError> {
        if keys.is_empty() {
            return Ok(false);
        }
        self.dbm.contains(&index_key(keys))
    }

    /// Whether some row extending the query's index key is past
    /// [`MAX_INDEX_KEY_WORDS`] words — the key the pin's
    /// `ChewingLargeTable2::search_suggestion` reads with `phrase_length =
    /// key.size / sizeof(ChewingKey)` (so a trailing odd byte counts as no
    /// word) and whose `switch` has no case for it: `default: abort()`
    /// (`chewing_large_table2_bdb.cpp:282`). Only the keys are read, never
    /// the values, and the scan stops at the first such key.
    pub(crate) fn has_overlong_extension(&self, keys: &[ChewingKey]) -> Result<bool, DictError> {
        if keys.is_empty() {
            return Ok(false);
        }
        let encoded = index_key(keys);
        let upper = prefix_upper_bound(&encoded);
        let mut found = false;
        self.dbm.walk(&encoded, upper.as_deref(), |key, _value| {
            found = key.len() / 2 > MAX_INDEX_KEY_WORDS;
            Ok(found)
        })?;
        Ok(found)
    }

    /// The suggestion lookup and range walk, checking raw key lengths
    /// before empty/odd rows are skipped or their values decoded. Returns
    /// `true` at the pin's overlong-key abort, stopping the store scan.
    pub(crate) fn walk_suggestions(
        &self,
        keys: &[ChewingKey],
        mut visit: impl FnMut(usize, &[PinyinIndexItem]) -> Result<bool, DictError>,
    ) -> Result<bool, DictError> {
        if self.dbm.attachment_failed() && self.dbm.failed_suggestion(&index_key(keys)) {
            return Ok(false);
        }
        if !self.key_exists(keys)? {
            return Ok(false);
        }
        let encoded = index_key(keys);
        let upper = prefix_upper_bound(&encoded);
        let mut overlong = false;
        self.dbm.walk(&encoded, upper.as_deref(), |key, value| {
            overlong = key.len() / 2 > MAX_INDEX_KEY_WORDS;
            if overlong {
                return Ok(true);
            }
            if value.is_empty() || !key.len().is_multiple_of(2) {
                return Ok(false);
            }
            let length = key.len() / 2;
            let items = decode_items(value, length)?;
            visit(length, &items)
        })?;
        Ok(overlong)
    }
}

/// The most words a `ChewingLargeTable2` index key can hold: the pin
/// instantiates its entries for lengths 1 through `MAX_PHRASE_LENGTH` (16).
const MAX_INDEX_KEY_WORDS: usize = 16;

/// `contains_incomplete_pinyin` (`pinyin_phrase3.h:146`): a syllable with
/// neither middle nor final is an initial-only (incomplete) key.
#[must_use]
pub fn contains_incomplete(keys: &[ChewingKey]) -> bool {
    keys.iter().any(|key| key.middle == 0 && key.final_ == 0)
}

/// The DBM key `ChewingLargeTable2::search` computes for a query.
#[must_use]
pub fn index_key(keys: &[ChewingKey]) -> Vec<u8> {
    if contains_incomplete(keys) {
        encode_incomplete_key(keys)
    } else {
        encode_complete_key(keys)
    }
}

/// `pinyin_compare_with_tones(query, stored, len) == 0`
/// (`pinyin_phrase3.h:108-144`, [`ChewingKey::matches_with_tones`] at every
/// position) over the first `query.len()` syllables of `stored` —
/// `prefix_compare_with_tones` (`PrefixLessThanWithTones`,
/// `chewing_large_table2.h`), the comparison the suggestion path applies
/// to longer records.
#[must_use]
pub fn prefix_keys_match(query: &[ChewingKey], stored: &[ChewingKey]) -> bool {
    if stored.len() < query.len() {
        return false;
    }
    query
        .iter()
        .zip(stored)
        .all(|(q, s)| q.matches_with_tones(*s))
}

/// Filter the exact row that ChewingLargeTable2::search found, retaining stored
/// order and SEARCH_CONTINUED even for an empty prefix marker. The whole row
/// is validated before any record is compared; the matching records' tokens
/// are collected and their keys are compared in place. Shared by the
/// production reader and the standalone in-memory test programs.
///
/// # Errors
/// Returns DictError for a malformed item array.
pub fn search_row(
    keys: &[ChewingKey],
    value: Option<&[u8]>,
) -> Result<(bool, Vec<u32>), DictError> {
    if keys.is_empty() {
        return Ok((false, Vec::new()));
    }
    let Some(value) = value else {
        return Ok((false, Vec::new()));
    };
    if value.is_empty() {
        return Ok((true, Vec::new()));
    }
    let tokens = crate::row_format::pinyin_index::item_views(value, keys.len())?
        .filter(|item| {
            keys.iter()
                .zip(item.keys())
                .all(|(query, stored)| query.matches_with_tones(stored))
        })
        .map(|item| item.token)
        .collect();
    Ok((true, tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
            value.clear();
            let data = self.data.lock().unwrap();
            let stored = data.get(key);
            if let Some(stored) = stored {
                value.extend_from_slice(stored);
            }
            Ok(stored.is_some())
        }

        fn contains(&self, key: &[u8]) -> Result<bool, DictError> {
            Ok(self.data.lock().unwrap().contains_key(key))
        }

        fn walk(
            &self,
            lo: &[u8],
            hi: Option<&[u8]>,
            mut visit: impl FnMut(&[u8], &[u8]) -> Result<bool, DictError>,
        ) -> Result<(), DictError> {
            let data = self.data.lock().unwrap();
            for (key, value) in data.range(lo.to_vec()..) {
                if hi.is_some_and(|hi| key.as_slice() >= hi) {
                    break;
                }
                if visit(key, value)? {
                    break;
                }
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
    fn the_row_filter_follows_the_three_upstream_comparisons() {
        let ni3 = ChewingKey::from_pinyin("ni").unwrap().with_tone(3);
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let n = ChewingKey::new(ni.initial, 0, 0, 0);
        let na = ChewingKey::from_pinyin("na").unwrap();
        // Whether a one-record row of `stored` survives the filter for `query`.
        let keys_match = |query: &[ChewingKey], stored: &[ChewingKey]| {
            let row = encode_items(&[PinyinIndexItem {
                token: 7,
                keys: stored.to_vec(),
            }]);
            search_row(query, Some(&row)).unwrap() == (true, vec![7])
        };
        assert!(keys_match(&[ni], &[ni3]), "zero tone matches any tone");
        assert!(keys_match(&[ni3], &[ni]), "either side zero");
        assert!(!keys_match(&[ni3.with_tone(1)], &[ni3]), "tones differ");
        assert!(
            keys_match(&[n], &[ni3]),
            "incomplete query matches any final"
        );
        assert!(keys_match(&[ni3], &[n]), "incomplete stored matches too");
        assert!(!keys_match(&[na], &[ni3]), "finals differ");
        assert!(prefix_keys_match(&[ni], &[ni3, na]));
        assert!(!prefix_keys_match(&[ni, ni], &[ni3]));
    }

    fn table_with_ba() -> ChewingTable<MemoryDbm> {
        let dbm = MemoryDbm::new();
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let ba_item = PinyinIndexItem {
            token: 0x0100_0001,
            keys: vec![ba.with_tone(1)],
        };
        let key = encode_complete_key(&[ba]);
        let value = encode_items(&[ba_item]);
        dbm.put(key, value);
        let ikey = encode_incomplete_key(&[ba]);
        let ivalue = encode_items(&[PinyinIndexItem {
            token: 0x0100_0001,
            keys: vec![ChewingKey::new(ba.initial, 0, 0, 0)],
        }]);
        dbm.put(ikey, ivalue);
        ChewingTable::new(dbm)
    }

    #[test]
    fn search_preserves_matching_token_order_and_duplicates_at_every_stride() {
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        for length in 1..=16 {
            let query = vec![ba.with_tone(1); length];
            let mut value = Vec::new();
            for (token, tone) in [(9, 3), (7, 1), (3, 0), (7, 1)] {
                let keys = vec![ba.with_tone(tone); length];
                let mut record = crate::row_format::pinyin_index::encode_item(token, &keys);
                record[4 + 2 * length..].fill(0xff);
                value.extend_from_slice(&record);
            }
            let dbm = MemoryDbm::new();
            dbm.put(index_key(&query), value);
            let table = ChewingTable::new(dbm);
            assert_eq!(table.search(&query).unwrap(), [7, 3, 7], "L={length}");
        }
    }

    #[test]
    fn search_refuses_a_truncated_row_before_filtering() {
        let ba = ChewingKey::from_pinyin("ba").unwrap();
        let query = [ba.with_tone(1)];
        let mut value = crate::row_format::pinyin_index::encode_item(9, &[ba.with_tone(3)]);
        value.push(0);
        let dbm = MemoryDbm::new();
        dbm.put(index_key(&query), value);
        assert!(ChewingTable::new(dbm).search(&query).is_err());
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
            token: 0x0100_0099,
            keys: vec![ni.with_tone(3), hao.with_tone(3)],
        };
        dbm.put(key2, encode_items(&[item]));
        let key1 = encode_complete_key(&[ni]);
        dbm.put(key1, Vec::new());

        let table = ChewingTable::new(dbm);

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
                    token: 0x0100_0001,
                    keys: vec![ba.with_tone(1)],
                },
                PinyinIndexItem {
                    token: 0x0100_0002,
                    keys: vec![bo.with_tone(2)],
                },
            ]),
        );
        let table = ChewingTable::new(dbm);

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
                    token: 0x0100_0099,
                    keys: vec![ni.with_tone(3), hao.with_tone(3)],
                },
                PinyinIndexItem {
                    token: 0x0100_0098,
                    keys: vec![na.with_tone(4), hao.with_tone(3)],
                },
            ]),
        );
        let table = ChewingTable::new(dbm);
        // "ni" + "h": the incomplete-space key `n h`, records filtered to
        // those whose first syllable is `ni`.
        let items = table.search(&[ni, h]).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0], 0x0100_0099);
    }

    #[test]
    fn has_overlong_extension_reads_keys_past_sixteen_words_under_the_query() {
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let hao = ChewingKey::from_pinyin("hao").unwrap();
        let table_with = |rows: &[Vec<u8>]| {
            let dbm = MemoryDbm::new();
            dbm.put(encode_complete_key(&[ni]), Vec::new());
            for row in rows {
                dbm.put(row.clone(), Vec::new());
            }
            ChewingTable::new(dbm)
        };
        // Sixteen words is the longest key the pin's switch instantiates.
        let sixteen = encode_complete_key(&[ni; 16]);
        assert!(
            !table_with(std::slice::from_ref(&sixteen))
                .has_overlong_extension(&[ni])
                .unwrap()
        );
        // Seventeen words under the query: the pin's `default: abort()`.
        let seventeen = encode_complete_key(&[ni; 17]);
        assert!(
            table_with(std::slice::from_ref(&seventeen))
                .has_overlong_extension(&[ni])
                .unwrap()
        );
        // A trailing odd byte still reads as seventeen words (`size / 2`).
        let mut odd = seventeen.clone();
        odd.push(7);
        assert!(table_with(&[odd]).has_overlong_extension(&[ni]).unwrap());
        // Eight-and-a-half words is not past sixteen.
        let mut short_odd = encode_complete_key(&[ni; 8]);
        short_odd.push(7);
        assert!(
            !table_with(&[short_odd])
                .has_overlong_extension(&[ni])
                .unwrap()
        );
        // A seventeen-word key under another prefix is not an extension.
        let elsewhere = encode_complete_key(&[hao; 17]);
        assert!(
            !table_with(&[elsewhere])
                .has_overlong_extension(&[ni])
                .unwrap()
        );
        // No query, nothing to extend.
        assert!(
            !table_with(&[seventeen])
                .has_overlong_extension(&[])
                .unwrap()
        );
    }

    #[test]
    fn suggestion_walk_matches_the_separate_probes() {
        let ni = ChewingKey::from_pinyin("ni").unwrap();
        let hao = ChewingKey::from_pinyin("hao").unwrap();
        let ni_hao = || {
            (
                encode_complete_key(&[ni, hao]),
                encode_items(&[PinyinIndexItem {
                    token: 0x0100_0099,
                    keys: vec![ni.with_tone(3), hao.with_tone(3)],
                }]),
            )
        };
        let overlong = || {
            let mut odd = encode_complete_key(&[ni; 17]);
            odd.push(7);
            (odd, Vec::new())
        };
        let marker = || (encode_complete_key(&[ni]), Vec::new());
        let cases: [Vec<(Vec<u8>, Vec<u8>)>; 5] = [
            vec![marker()],
            vec![marker(), ni_hao()],
            vec![ni_hao()],
            vec![marker(), overlong()],
            vec![marker(), ni_hao(), overlong()],
        ];
        for (case, rows) in cases.into_iter().enumerate() {
            let dbm = MemoryDbm::new();
            for (key, value) in rows {
                dbm.put(key, value);
            }
            let table = ChewingTable::new(dbm);
            let mut fused = Vec::new();
            let fused_overlong = table
                .walk_suggestions(&[ni], |len, items| {
                    fused.push((len, items.to_vec()));
                    Ok(false)
                })
                .unwrap();
            // The uncached path: the existence probe and the key-length scan,
            // each on its own.
            let exists = table.key_exists(&[ni]).unwrap();
            let separate_overlong = exists && table.has_overlong_extension(&[ni]).unwrap();
            assert_eq!(fused_overlong, separate_overlong, "case {case}");
            if !separate_overlong {
                // The rows a suggestion walk visits: the `ni'hao` record when
                // the query's own key exists, nothing otherwise.
                let expected = if case == 1 {
                    vec![(
                        2,
                        vec![PinyinIndexItem {
                            token: 0x0100_0099,
                            keys: vec![ni.with_tone(3), hao.with_tone(3)],
                        }],
                    )]
                } else {
                    Vec::new()
                };
                assert_eq!(fused, expected, "case {case}");
            }
        }
    }

    #[test]
    fn raw_walk_stops_the_store_scan_and_keeps_errors_distinct() {
        use oxpinyin_store::{DefaultStore, RawReadStore as _, WriteStore as _, WriteTxn as _};
        let dir = std::env::temp_dir().join(format!(
            "oxpinyin-raw-walk-stop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("walk_stop.{}", oxpinyin_store::DEFAULT_STORE_EXT));
        let store = DefaultStore::create(&path).unwrap();
        store
            .write(|txn| {
                for i in 0_u8..8 {
                    txn.put_raw(&[i], &[i])?;
                }
                Ok(())
            })
            .unwrap();
        drop(store);
        let dbm = RawChewingDbm::new(DefaultStore::open_read_only(&path).unwrap());

        // A stop after two rows: only two rows are visited and the walk
        // succeeds — the store scan is aborted, not run to the end.
        let mut seen = Vec::new();
        dbm.walk(&[2], Some(&[8]), |key, _| {
            seen.push(key[0]);
            Ok(seen.len() == 2)
        })
        .unwrap();
        assert_eq!(seen, vec![2, 3]);

        // No stop: every row of the range.
        let mut count = 0;
        dbm.walk(&[2], Some(&[8]), |_, _| {
            count += 1;
            Ok(false)
        })
        .unwrap();
        assert_eq!(count, 6);

        // A visitor error comes back as itself, not as the sentinel that
        // aborted the store scan.
        let err = dbm
            .walk(&[2], Some(&[8]), |_, _| {
                Err(DictError::Parse("boom".to_owned()))
            })
            .unwrap_err();
        assert!(
            matches!(err, DictError::Parse(ref m) if m == "boom"),
            "{err:?}"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
