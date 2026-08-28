//! System dictionary backed by the exported pinyin index and phrase index.
//!
//! Implements [`oxpinyin_core::Dictionary`] over the tables
//! committed under `fixtures/w3/` (frozen; no longer regenerated in-tree)
//! (`docs/findings/data-layer-export.md`). The index is keyed by the
//! pinyin spelling itself — syllables joined by `'` — so a lookup for
//! `[ni, hao]` is a single get on `ni'hao`; there is no per-syllable
//! binary encoder and no compound binary key. Entries come back in the
//! stored order, which the exporter froze as frequency-descending.
//!
//! Values are the resolved [`PhraseEntry`] slice so `fill_lookup` copies
//! hits instead of walking `phrase_index` / `unigrams` per record.

use std::collections::BTreeMap;
use std::fmt;
use std::ops::Range;
use std::path::Path;
use std::sync::OnceLock;

use compact_str::CompactString;
use oxpinyin_core::{
    Completeness, Dictionary, PhraseEntry, PhraseToken, SyllableKey, syllable_initial,
};

use crate::table::{self, LeByteKey, TableError};

/// The pinyin index as a sorted vector map plus one shared entry arena.
///
/// `rows` holds `(key, entries range)` pairs in ascending key order — redb
/// walks its B-tree in that order, so the load appends rows instead of
/// paying `BTreeMap::insert`'s O(log n) string compares per row (the
/// #129/#132 audit). All [`PhraseEntry`] hits live contiguously in
/// `entries`: one allocation for the whole index instead of one boxed
/// slice per key, and a key's hits are still one contiguous slice for
/// `fill_lookup`'s `extend_from_slice`.
#[derive(Default)]
struct PinyinIndex {
    /// `(pinyin key, hits range into `entries`)`, ascending key order.
    rows: Vec<(Box<str>, Range<usize>)>,
    /// Every resolved hit of every row, concatenated in row order.
    entries: Vec<PhraseEntry>,
}

impl PinyinIndex {
    /// Exact-key get: the hits slice of `key`, or `None`.
    fn hits(&self, key: &str) -> Option<&[PhraseEntry]> {
        self.rows
            .binary_search_by(|(stored, _)| stored.as_ref().cmp(key))
            .ok()
            .map(|position| &self.entries[self.rows[position].1.clone()])
    }
}

/// The phrase index as a sorted vector map: `(token, text)` pairs in
/// ascending [`LeByteKey`] order — the order redb's walk already yields
/// for the 4-byte little-endian keys the exporter wrote — searched by
/// binary search. The append replaces `BTreeMap::insert`'s O(log n)
/// per-row walk; iteration stays ascending in walk order.
type PhraseIndex = Vec<(LeByteKey, CompactString)>;

/// Error conditions for system dictionary lookups.
#[derive(Debug)]
pub enum DictError {
    /// A table-level error (I/O, redb, etc.).
    Table(TableError),
    /// Value bytes did not parse as `{token, freq}` records.
    Parse(String),
}

impl fmt::Display for DictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for DictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
            Self::Parse(_) => None,
        }
    }
}

impl From<TableError> for DictError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

/// The system dictionary, backed by `pinyin_index.redb` and
/// `phrase_index.redb` committed under `fixtures/w3/`.
pub struct SystemDictionary {
    /// Pinyin spelling → `{token, freq}` records, stored order.
    pinyin_index: PinyinIndex,
    /// Phrase token → UTF-8 text.
    phrase_index: PhraseIndex,
    /// Aggregated phrase frequencies across all pinyin keys, for unigram LM.
    unigrams: BTreeMap<u32, u64>,
    unigram_total: u64,
    /// Every pinyin-index key projected to its initial sequence, sorted:
    /// the probe the pin uses when the searched sequence holds an
    /// initial-only key. Vowel-initial syllables project to a `0` sentinel.
    initial_keys: Box<[String]>,
    /// Reverse phrase-table map: exact UTF-8 text → tokens (nibble then id).
    /// Built on first `tokens_for_text` / `suggest_after`; decode does not
    /// need it, so `open` leaves it empty.
    text_tokens: OnceLock<BTreeMap<String, Vec<u32>>>,
}

impl SystemDictionary {
    /// Opens the system dictionary from the two exported table files.
    pub fn open(pinyin_index_path: &Path, phrase_index_path: &Path) -> Result<Self, DictError> {
        let phrase_index = load_phrase_index(phrase_index_path)?;
        let derived = load_pinyin_index(pinyin_index_path, &phrase_index)?;
        Ok(Self {
            pinyin_index: derived.pinyin_index,
            phrase_index,
            unigrams: derived.unigrams,
            unigram_total: derived.unigram_total,
            initial_keys: derived.initial_keys,
            text_tokens: OnceLock::new(),
        })
    }

    /// Number of pinyin keys in the index.
    pub fn key_count(&self) -> Result<u64, DictError> {
        Ok(self.pinyin_index.rows.len() as u64)
    }

    /// Total of all phrase frequencies observed in the pinyin index.
    #[must_use]
    pub const fn unigram_total(&self) -> u64 {
        self.unigram_total
    }

    /// Frequency of `token` aggregated across all pinyin keys.
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        self.unigrams.get(&token).copied()
    }

    /// Unigram map for wiring into a [`crate::BigramLanguageModel`].
    #[must_use]
    pub const fn unigram_map(&self) -> &BTreeMap<u32, u64> {
        &self.unigrams
    }

    /// Phrase text for `token` from the exported phrase index, if present.
    ///
    /// This is the reverse half of [`SystemDictionary::lookup`] and backs the
    /// W6-T7 bigram export's text rendering for system tokens
    /// (`docs/findings/user-store.md` §9).
    pub fn phrase_text(&self, token: u32) -> Result<Option<String>, DictError> {
        Ok(phrase_lookup(&self.phrase_index, token).map(str::to_owned))
    }

    /// Every pinyin-index spelling recorded for `token`, with its frequency,
    /// in pinyin-index key order.
    ///
    /// These are the phrase item's pronunciations in the upstream model (the
    /// pinyin table holds one key sequence per pronunciation), so this is the
    /// rendering surface the W6-T7 bigram export needs for system tokens.
    /// The scan is O(index) — an export-time cost, not a decode-path cost.
    pub fn pronunciations(&self, token: u32) -> Result<Vec<(String, u64)>, DictError> {
        let mut out = Vec::new();
        for (pinyin, range) in &self.pinyin_index.rows {
            for entry in &self.pinyin_index.entries[range.clone()] {
                if entry.token().value() == token {
                    let freq = entry
                        .pronunciation_possibility()
                        .map(|(matched, _)| matched)
                        .unwrap_or(0);
                    out.push((pinyin.to_string(), freq));
                }
            }
        }
        Ok(out)
    }

    /// Tokens whose phrase text is exactly `text`.
    #[must_use]
    pub fn tokens_for_text(&self, text: &str) -> &[u32] {
        self.reverse_text_tokens()
            .ok()
            .and_then(|map| map.get(text).map(Vec::as_slice))
            .unwrap_or(&[])
    }

    /// Tokens whose phrase text starts with `prefix` and is longer, when
    /// `prefix` itself is a stored phrase (`PhraseLargeTable3::search_suggestion`).
    ///
    /// Rows come out in the DEFINED prediction order (`upstream-divergences.md`,
    /// "Predicted-candidate tie order"): the `BTreeMap`'s text-ascending
    /// walk, token-ascending within one text. The pin's order is its DBM's
    /// physical bucket walk — backend-dependent and semantically arbitrary —
    /// so oxpinyin deliberately diverges on positions with this defined,
    /// build-stable order instead.
    #[must_use]
    pub fn suggest_after(&self, prefix: &str) -> Vec<(u32, String)> {
        let Ok(map) = self.reverse_text_tokens() else {
            return Vec::new();
        };
        if prefix.is_empty() || !map.contains_key(prefix) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (text, tokens) in map.range(prefix.to_owned()..) {
            if !text.starts_with(prefix) {
                break;
            }
            if text == prefix {
                continue;
            }
            for token in tokens {
                out.push((*token, text.clone()));
            }
        }
        out
    }

    /// The reverse phrase map, built once on first predict/import use.
    fn reverse_text_tokens(&self) -> Result<&BTreeMap<String, Vec<u32>>, DictError> {
        if let Some(map) = self.text_tokens.get() {
            return Ok(map);
        }
        let map = build_text_tokens(&self.phrase_index)?;
        Ok(self.text_tokens.get_or_init(|| map))
    }

    /// The frozen index key for a syllable sequence: texts joined by `'`.
    fn index_key(syllables: &[SyllableKey]) -> CompactString {
        let mut key = CompactString::const_new("");
        for (position, syllable) in syllables.iter().enumerate() {
            if position > 0 {
                key.push('\'');
            }
            key.push_str(syllable.text());
        }
        key
    }

    /// Projects a sequence to the initial form the pin's incomplete-index
    /// probe uses: a complete key contributes its initial, an initial-only
    /// key its own spelling, joined by `'` with `0` for vowel-initial keys.
    fn initial_key(syllables: &[SyllableKey]) -> CompactString {
        let mut key = CompactString::const_new("");
        for (position, syllable) in syllables.iter().enumerate() {
            if position > 0 {
                key.push('\'');
            }
            match syllable_initial(syllable.text()) {
                Some(initial) => key.push_str(initial),
                None => key.push('0'),
            }
        }
        key
    }

    fn fill_lookup(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        out.clear();
        if syllables.is_empty() {
            return Ok(());
        }
        let key = Self::index_key(syllables);
        if let Some(hits) = self.pinyin_index.hits(key.as_str()) {
            out.extend_from_slice(hits);
        }
        Ok(())
    }
}

impl Dictionary for SystemDictionary {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        self.fill_lookup(syllables, &mut entries)?;
        Ok(entries)
    }

    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        self.fill_lookup(syllables, out)
    }

    fn lookup_addon_into(
        &self,
        _syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        out.clear();
        Ok(())
    }

    fn phrase_index_item_count(&self) -> Result<u64, Self::Error> {
        Ok(self.unigram_map().len() as u64)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
        if syllables.is_empty() {
            return Ok(true);
        }
        if syllables
            .iter()
            .any(|key| key.completeness() == Completeness::Partial)
        {
            Ok(prefix_probe(
                &self.initial_keys,
                &Self::initial_key(syllables),
            ))
        } else {
            Ok(pinyin_prefix_exists(
                &self.pinyin_index,
                &Self::index_key(syllables),
            ))
        }
    }
}

/// The `SEARCH_CONTINUED` probe over the sorted key list: does any stored key
/// equal `joined`, or extend it at a syllable boundary (`joined` + `'`)?
fn prefix_probe(sorted: &[String], joined: &str) -> bool {
    match sorted.binary_search_by(|candidate| candidate.as_str().cmp(joined)) {
        Ok(_) => true,
        Err(index) => {
            sorted
                .get(index)
                .is_some_and(|candidate| candidate.starts_with(joined))
                && sorted[index].as_bytes().get(joined.len()) == Some(&b'\'')
        }
    }
}

/// The `SEARCH_CONTINUED` probe over the sorted vector map.
fn pinyin_prefix_exists(index: &PinyinIndex, joined: &str) -> bool {
    // First key >= joined; the exact and the boundary-extension hits can
    // only be there, as on the tree's `contains_key` + `range` pair.
    let first_at_or_after = index.rows.partition_point(|(key, _)| key.as_ref() < joined);
    index.rows.get(first_at_or_after).is_some_and(|(key, _)| {
        key.as_ref() == joined
            || (key.starts_with(joined) && key.as_bytes().get(joined.len()) == Some(&b'\''))
    })
}

/// Exact-token get on the phrase-index vector map.
fn phrase_lookup(index: &[(LeByteKey, CompactString)], token: u32) -> Option<&str> {
    let needle = LeByteKey::new(token);
    index
        .binary_search_by(|(stored, _)| stored.cmp(&needle))
        .ok()
        .map(|position| index[position].1.as_str())
}

struct PinyinDerived {
    pinyin_index: PinyinIndex,
    unigrams: BTreeMap<u32, u64>,
    unigram_total: u64,
    initial_keys: Box<[String]>,
}

/// Load-time row staging: pinyin key → range into one flat
/// `{token, freq}` record vec, appended in the walk order `for_each_row`
/// yields, before `resolve_hits` fans the records out into the arena.
type RawPinyinRows = Vec<(Box<str>, Range<usize>)>;

fn load_pinyin_index(path: &Path, phrase_index: &PhraseIndex) -> Result<PinyinDerived, DictError> {
    let mut raw: RawPinyinRows = Vec::new();
    let mut flat_records: Vec<(u32, u32)> = Vec::new();
    let mut unigrams: BTreeMap<u32, u64> = BTreeMap::new();
    let mut unigram_total: u64 = 0;
    // Initial keys stage as packed u128s (order-exact, sort integers) and
    // decode to strings only once, deduped; a key too long to pack falls
    // back to the string path and re-sorts the whole list — the export's
    // longest key is 14 syllables against a 25-slot pack.
    let alphabet = crate::initials::InitialAlphabet::new();
    let mut initial_keys: Vec<u128> = Vec::new();
    let mut oversized_initials: Vec<String> = Vec::new();

    table::for_each_row(path, |key, value| {
        let pinyin = std::str::from_utf8(key)
            .map_err(|_| DictError::Parse("pinyin index key is not UTF-8".to_owned()))?;
        match alphabet.pack(pinyin) {
            Some(packed) => initial_keys.push(packed),
            None => oversized_initials.push(alphabet.project(pinyin)),
        }

        if !value.len().is_multiple_of(8) {
            return Err(DictError::Parse(format!(
                "index value length {} is not a multiple of 8",
                value.len()
            )));
        }
        let start = flat_records.len();
        for chunk in value.chunks_exact(8) {
            let token = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
            let freq = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
            flat_records.push((token, freq));
            let count = unigrams.entry(token).or_default();
            *count = count.saturating_add(u64::from(freq));
            unigram_total = unigram_total.saturating_add(u64::from(freq));
        }
        // The row walk is ascending, so this is an append; the order check
        // below repairs (and keeps the last row per key, as insert did) if a
        // table ever arrives any other way.
        raw.push((Box::from(pinyin), start..flat_records.len()));
        Ok::<(), DictError>(())
    })?;
    table::ensure_sorted_unique(&mut raw);

    initial_keys.sort_unstable();
    initial_keys.dedup();
    let mut initial_list: Vec<String> = initial_keys
        .iter()
        .map(|&packed| alphabet.unpack(packed))
        .collect();
    if !oversized_initials.is_empty() {
        initial_list.extend(oversized_initials);
        initial_list.sort_unstable();
        initial_list.dedup();
    }
    let initial_keys = initial_list.into_boxed_slice();

    // Totals are the sum over every pronunciation; resolve after the
    // aggregate pass so each hit carries the item's final unigram, and
    // resolve into one arena so a key's hits stay a contiguous slice.
    // Totals resolve from a sorted snapshot: the map's in-order walk is
    // O(n) to build and a binary search per record beats a tree walk.
    let totals: Vec<(u32, u64)> = unigrams
        .iter()
        .map(|(&token, &count)| (token, count))
        .collect();
    let mut entries: Vec<PhraseEntry> = Vec::with_capacity(flat_records.len());
    let mut rows: Vec<(Box<str>, Range<usize>)> = Vec::with_capacity(raw.len());
    for (key, range) in raw {
        let start = entries.len();
        resolve_hits(&flat_records[range], phrase_index, &totals, &mut entries);
        rows.push((key, start..entries.len()));
    }
    Ok(PinyinDerived {
        pinyin_index: PinyinIndex { rows, entries },
        unigrams,
        unigram_total,
        initial_keys,
    })
}

/// Token → text through `phrase_index` (the reverse half is `phrase_text`).
/// The full export resolves every token; a mini fixture may omit some,
/// and those records contribute no candidate rather than failing open.
///
/// W14: the record's frequency is the matched pronunciation's share and
/// the aggregated unigram map holds the item's pronunciation total —
/// matched/total is upstream's `get_pronunciation_possibility` polyphone
/// discount. The candidate scan never reads it.
fn resolve_hits(
    records: &[(u32, u32)],
    phrase_index: &PhraseIndex,
    totals: &[(u32, u64)],
    entries: &mut Vec<PhraseEntry>,
) {
    entries.extend(records.iter().filter_map(|&(token, freq)| {
        phrase_lookup(phrase_index, token).map(|text| {
            let total = totals
                .binary_search_by_key(&token, |&(token, _)| token)
                .ok()
                .map(|position| totals[position].1)
                .unwrap_or(0);
            PhraseEntry::new(PhraseToken::new(token), text)
                .with_pronunciation_possibility(u64::from(freq), total)
        })
    }));
}

fn load_phrase_index(path: &Path) -> Result<PhraseIndex, DictError> {
    let mut map = PhraseIndex::new();
    table::for_each_row(path, |key, value| {
        if key.len() != 4 {
            return Ok::<(), DictError>(());
        }
        let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        let text = std::str::from_utf8(value).map_err(|_| {
            DictError::Parse(format!("phrase text for token {token:#010x} is not UTF-8"))
        })?;
        map.push((LeByteKey::new(token), CompactString::from(text)));
        Ok::<(), DictError>(())
    })?;
    table::ensure_sorted_unique(&mut map);
    Ok(map)
}

/// Builds the exact-text → tokens reverse map from the typed phrase index.
fn build_text_tokens(phrase_index: &PhraseIndex) -> Result<BTreeMap<String, Vec<u32>>, DictError> {
    let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (token, text) in phrase_index {
        map.entry(text.to_string()).or_default().push(token.token());
    }
    for tokens in map.values_mut() {
        tokens.sort_unstable();
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures_dir() -> std::path::PathBuf {
        let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
        std::path::PathBuf::from(manifest)
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures")
            .join("w3")
    }

    fn dict() -> SystemDictionary {
        SystemDictionary::open(
            &fixtures_dir().join("pinyin_index.redb"),
            &fixtures_dir().join("phrase_index.redb"),
        )
        .unwrap()
    }

    fn key(text: &str) -> SyllableKey {
        SyllableKey::from_text(text).expect("frozen syllable")
    }
    #[test]
    fn every_phrase_token_is_reachable_from_the_pinyin_index() {
        // Export invariant: a token in phrase_index but referenced by no
        // pinyin_index entry is unreachable by lookup, and its frequency never
        // enters the aggregated unigram map — so its unigram silently costs
        // UNKNOWN_COST. Every phrase token must appear in at least one
        // pinyin_index record.
        use std::collections::BTreeSet;

        let dict = dict();
        let mut reachable: BTreeSet<u32> = BTreeSet::new();
        for range in dict.pinyin_index.rows.iter().map(|(_, range)| range) {
            for entry in &dict.pinyin_index.entries[range.clone()] {
                reachable.insert(entry.token().value());
            }
        }

        for token in dict.phrase_index.iter().map(|(key, _)| key.token()) {
            assert!(
                reachable.contains(&token),
                "phrase token {token:#010x} is in phrase_index but no pinyin_index entry references it"
            );
        }
    }

    #[test]
    fn reverse_map_is_lazy_and_finds_fixture_text() {
        let dict = dict();
        assert!(
            dict.text_tokens.get().is_none(),
            "decode-only open must not build the reverse map"
        );
        let entries = dict.lookup(&[key("ni")]).unwrap();
        let lead = &entries[0];
        let tokens = dict.tokens_for_text(lead.text());
        assert!(
            tokens.contains(&lead.token().value()),
            "lazy reverse map must resolve the looked-up phrase"
        );
        assert!(dict.text_tokens.get().is_some());
    }

    #[test]
    fn loaded_index_is_sorted_and_unique() {
        // The sorted vector map is only correct with ascending unique keys;
        // redb's walk provides them, and this pins that the load keeps them.
        let dict = dict();
        assert!(
            dict.pinyin_index
                .rows
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0),
            "pinyin index keys must be strictly ascending after open"
        );
    }

    #[test]
    fn hits_binary_searches_the_loaded_order() {
        let dict = dict();
        let hits = dict.pinyin_index.hits("ni'hao").expect("fixture key");
        assert!(hits.iter().any(|entry| entry.text() == "你好"));
        assert!(dict.pinyin_index.hits("no'such").is_none());
    }
}
