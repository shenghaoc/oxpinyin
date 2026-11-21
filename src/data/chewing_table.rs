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

use crate::data::phrase_library::CHEWING_KEY_SIZE;
use crate::parser::ChewingKey;
use crate::vocab::MAX_PHRASE_LENGTH;

use crate::data::DictError;
// The key and record layouts this reader shares with
// `gen_binary_files`'s writer: one written copy, so a probe here and an
// emission there cannot drift (`crate::data::row_format`).
#[cfg(test)]
use crate::data::row_format::pinyin_index::encode_items;
pub use crate::data::row_format::pinyin_index::{
    PinyinIndexItem, decode_items, encode_complete_key, encode_incomplete_key,
};

// ── ChewingTable ─────────────────────────────────────────────────

/// The DBM access method behind the tables ([`ChewingTable`],
/// [`crate::data::phrase_table::PhraseTable`], [`crate::data::BigramTable`], [`crate::data::PunctTable`]):
/// each table is generic over it, so the one implementation of a table's
/// logic runs over the production [`DefaultDbm`] and over the in-memory,
/// counting and failing test doubles alike.
pub trait ChewingDbm {
    /// Whether the system table failed to open.
    fn attachment_failed(&self) -> bool {
        false
    }

    /// Closes the native attachment at facade fini. Later reads are misses.
    fn finish_attachment(&mut self) {}

    /// Whether [`Self::finish_attachment`] has closed the native object.
    #[cfg(test)]
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

    /// Read one phrase-table row with the shape of
    /// `PhraseLargeTable3::search`: a failed read is a miss
    /// (`phrase_large_table3_tkrzwdb.cpp:136-139`,
    /// `phrase_large_table3_bdb.cpp:196-198`,
    /// `phrase_large_table3_kyotodb.cpp:174-182` at 074a2219). Kyoto
    /// Cabinet aborts on a failed body read after a successful check
    /// (`:187`). The default is [`Self::get`].
    fn get_phrase_row(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
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

/// Wraps a [`RawReadStore`](crate::store::RawReadStore) for raw
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
    Unopened,
    /// The open file after facade fini: the handle is dropped, which closes
    /// it and releases its lock. An instance or iterator that outlives the
    /// context reads misses here.
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
            DbmState::Unopened | DbmState::Closed => None,
        }
    }
}

/// The production [`ChewingDbm`]: the compiled-in backend read through
/// its raw (unframed) keyspace.
pub type DefaultDbm = RawChewingDbm<crate::store::DefaultStore>;

impl DefaultDbm {
    pub(crate) fn attach(path: &std::path::Path, hash: bool) -> Result<Self, DictError> {
        use crate::store::SystemAttachment;
        Ok(
            match SystemAttachment::open(path, hash).map_err(|e| DictError::Table(e.into()))? {
                SystemAttachment::Open(store) => Self::new(store),
                SystemAttachment::Unopened => Self {
                    state: DbmState::Unopened,
                },
            },
        )
    }
}

/// A point read whose failure the pin answers as a miss: the value is
/// cleared and the key reads as absent. The one failure that stays an error
/// is a body read after a good size check, where the pin asserts.
fn failed_read_is_a_miss(
    read: Result<bool, crate::store::StoreError>,
    value: &mut Vec<u8>,
) -> Result<bool, DictError> {
    match read {
        Err(error @ crate::store::StoreError::ReadAfterCheck) => {
            Err(DictError::Table(error.into()))
        }
        Err(
            crate::store::StoreError::Io(_)
            | crate::store::StoreError::Backend(_)
            | crate::store::StoreError::ReadOnly
            | crate::store::StoreError::InvalidInput(_)
            | crate::store::StoreError::ShortArrayHeader,
        ) => {
            value.clear();
            Ok(false)
        }
        Ok(found) => Ok(found),
    }
}

impl<S: crate::store::RawReadStore> ChewingDbm for RawChewingDbm<S> {
    fn attachment_failed(&self) -> bool {
        matches!(self.state, DbmState::Unopened)
    }

    fn finish_attachment(&mut self) {
        // A failed open still controls the phrase-prefix walk after fini.
        if matches!(self.state, DbmState::Open(_)) {
            self.state = DbmState::Closed;
        }
    }

    #[cfg(test)]
    fn attachment_finished(&self) -> bool {
        !matches!(self.state, DbmState::Open(_))
    }

    fn get(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        match &self.state {
            DbmState::Open(store) => store
                .get_raw(key, value)
                .map_err(|e| DictError::Table(e.into())),
            DbmState::Unopened | DbmState::Closed => {
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
            DbmState::Unopened | DbmState::Closed => {
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
                // path through either table search
                // (`chewing_large_table2_kyotodb.cpp:170-174`,
                // `phrase_large_table3_kyotodb.cpp:187` at 074a2219).
                Err(crate::store::StoreError::ReadAfterCheck) => std::process::abort(),
                read => failed_read_is_a_miss(read, value),
            },
            DbmState::Unopened | DbmState::Closed => self.get(key, value),
        }
    }

    fn get_phrase_row(&self, key: &[u8], value: &mut Vec<u8>) -> Result<bool, DictError> {
        // Both tables use the same checked read on Kyoto Cabinet, including
        // the abort after a successful check (`phrase_large_table3_kyotodb.cpp:
        // 174-187` at 074a2219).
        self.get_pinyin_row(key, value)
    }

    fn contains(&self, key: &[u8]) -> Result<bool, DictError> {
        match &self.state {
            // A failed existence read is a miss: the pin tests
            // `Get(key, nullptr).IsOK()` and returns
            // (`chewing_large_table2_tkrzwdb.cpp:511`,
            // `phrase_large_table3_tkrzwdb.cpp:166` at 074a2219), as its
            // Kyoto Cabinet `check` and Berkeley DB cursor do.
            DbmState::Open(store) => Ok(store.contains_raw(key).unwrap_or(false)),
            DbmState::Unopened | DbmState::Closed => Ok(false),
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
        // A cursor step that fails ends the walk with the rows read so far:
        // the pin returns on a failed `Jump` or `Next` and leaves its loop
        // on a failed cursor `Get`
        // (`chewing_large_table2_tkrzwdb.cpp:516-524`,
        // `phrase_large_table3_tkrzwdb.cpp:171-178` at 074a2219).
        scan.or(Ok(()))
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

    #[cfg(test)]
    pub(crate) fn attachment_failed(&self) -> bool {
        self.dbm.attachment_failed()
    }

    pub(crate) fn finish_attachment(&mut self) {
        self.dbm.finish_attachment();
    }

    #[cfg(test)]
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
    ///   when it resolves tokens.
    pub(crate) fn search(&self, keys: &[ChewingKey]) -> Result<Vec<u32>, DictError> {
        self.search_continued(keys, &mut Vec::new())
            .map(|(_, tokens)| tokens)
    }

    /// [`Self::search`] with one DBM get into `value`, the caller's
    /// buffer: `visit` receives each surviving token in stored order, and
    /// the answer is whether the query's index key exists, an empty marker
    /// included. That is the `SEARCH_CONTINUED` bit the same get sets
    /// (`chewing_large_table2_bdb.cpp:180-187`,
    /// `chewing_large_table2_tkrzwdb.cpp:146-152`,
    /// `chewing_large_table2_kyotodb.cpp:160-166` at 074a2219); a failed
    /// read is a miss and sets nothing.
    pub(crate) fn search_into(
        &self,
        keys: &[ChewingKey],
        value: &mut Vec<u8>,
        visit: impl FnMut(u32),
    ) -> Result<bool, DictError> {
        if keys.is_empty() {
            return Ok(false);
        }
        let present = with_index_key(keys, |key| self.dbm.get_pinyin_row(key, value))?;
        if present {
            visit_matching_items(keys, value, visit)?;
        }
        Ok(present)
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
        let mut tokens = Vec::new();
        let present = self.search_into(keys, value, |token| tokens.push(token))?;
        Ok((present, tokens))
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
    /// [`MAX_PHRASE_LENGTH`] words, the most a `ChewingLargeTable2` index
    /// key can hold (the pin instantiates its entries for lengths 1 through
    /// `MAX_PHRASE_LENGTH`) — the key the pin's
    /// `ChewingLargeTable2::search_suggestion` reads with `phrase_length =
    /// key.size / sizeof(ChewingKey)` (so a trailing odd byte counts as no
    /// word) and whose `switch` has no case for it: `default: abort()`
    /// (`chewing_large_table2_bdb.cpp:282`). Only the keys are read, never
    /// the values, and the scan stops at the first such key.
    #[cfg(test)]
    pub(crate) fn has_overlong_extension(&self, keys: &[ChewingKey]) -> Result<bool, DictError> {
        if keys.is_empty() {
            return Ok(false);
        }
        let encoded = index_key(keys);
        let upper = prefix_upper_bound(&encoded);
        let mut found = false;
        self.dbm.walk(&encoded, upper.as_deref(), |key, _value| {
            found = key.len() / CHEWING_KEY_SIZE > MAX_PHRASE_LENGTH;
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
        if !self.key_exists(keys)? {
            return Ok(false);
        }
        let encoded = index_key(keys);
        let upper = prefix_upper_bound(&encoded);
        let mut overlong = false;
        self.dbm.walk(&encoded, upper.as_deref(), |key, value| {
            overlong = key.len() / CHEWING_KEY_SIZE > MAX_PHRASE_LENGTH;
            if overlong {
                return Ok(true);
            }
            if value.is_empty() || !key.len().is_multiple_of(CHEWING_KEY_SIZE) {
                return Ok(false);
            }
            let length = key.len() / CHEWING_KEY_SIZE;
            let items = decode_items(value, length)?;
            visit(length, &items)
        })?;
        Ok(overlong)
    }
}

/// `contains_incomplete_pinyin` (`pinyin_phrase3.h:146`): a syllable with
/// neither middle nor final is an initial-only (incomplete) key.
#[must_use]
pub fn contains_incomplete(keys: &[ChewingKey]) -> bool {
    keys.iter().any(|key| key.middle == 0 && key.final_ == 0)
}

// ChewingLargeTable2::search, chewing_large_table2.cpp:161–174.
/// Encodes a query as a DBM key. If any input key is initial-only, every key in the
/// sequence is reduced to its initial. Otherwise the complete-key encoding is used.
#[must_use]
pub fn index_key(keys: &[ChewingKey]) -> Vec<u8> {
    if contains_incomplete(keys) {
        encode_incomplete_key(keys)
    } else {
        encode_complete_key(keys)
    }
}

/// [`index_key`] passed to `read` from a stack buffer. The pin's search
/// keeps its index in a `ChewingKey[MAX_PHRASE_LENGTH]` array
/// (`chewing_large_table2.cpp:161-172` at 074a2219); a longer query, which
/// that search never takes, is encoded on the heap.
fn with_index_key<R>(keys: &[ChewingKey], read: impl FnOnce(&[u8]) -> R) -> R {
    if keys.len() > MAX_PHRASE_LENGTH {
        return read(&index_key(keys));
    }
    let incomplete = contains_incomplete(keys);
    let mut buf = [0_u8; CHEWING_KEY_SIZE * MAX_PHRASE_LENGTH];
    let (words, _) = buf.as_chunks_mut::<CHEWING_KEY_SIZE>();
    for (word, key) in words.iter_mut().zip(keys) {
        let indexed = if incomplete {
            ChewingKey::new(key.initial, 0, 0, 0)
        } else {
            ChewingKey::new(key.initial, key.middle, key.final_, 0)
        };
        *word = indexed.to_packed().to_le_bytes();
    }
    read(&buf[..CHEWING_KEY_SIZE * keys.len()])
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

// ChewingLargeTable2::search, chewing_large_table2.cpp:161–174; SEARCH_CONTINUED is
// independent of the filtered tokens.
/// Returns whether a row was present for a nonempty query and its matching tokens in
/// stored order. A present empty prefix marker or a row with no matching readings still
/// reports presence.
///
/// # Errors
///
/// Propagates a row-decoding error.
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
    let mut tokens = Vec::new();
    visit_matching_items(keys, value, |token| tokens.push(token))?;
    Ok((true, tokens))
}

fn visit_matching_items(
    keys: &[ChewingKey],
    value: &[u8],
    mut visit: impl FnMut(u32),
) -> Result<(), DictError> {
    if value.is_empty() {
        return Ok(());
    }
    for item in crate::data::row_format::pinyin_index::item_views(value, keys.len())? {
        if keys
            .iter()
            .zip(item.keys())
            .all(|(query, stored)| query.matches_with_tones(stored))
        {
            visit(item.token);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
