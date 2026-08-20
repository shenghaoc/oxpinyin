//! System dictionary backed by the exported pinyin index and phrase index.
//!
//! Implements [`oxpinyin_core::Dictionary`] over the tables that
//! `oxpinyin-migrate export` derives from the pinned oracle's public ABI
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
use std::ops::Bound::{Included, Unbounded};
use std::path::Path;
use std::sync::OnceLock;

use compact_str::CompactString;
use oxpinyin_core::{
    Completeness, Dictionary, PhraseEntry, PhraseToken, SyllableKey, syllable_initial,
};

use crate::table::{self, TableError};

type PinyinIndex = BTreeMap<Box<str>, Box<[PhraseEntry]>>;
type PhraseIndex = BTreeMap<u32, CompactString>;

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
/// `phrase_index.redb` from `oxpinyin-migrate export`.
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
        Ok(self.pinyin_index.len() as u64)
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
        Ok(self.phrase_index.get(&token).map(|text| text.to_string()))
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
        for (pinyin, records) in &self.pinyin_index {
            for entry in records.iter() {
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
        out.sort_by_key(|(token, _)| *token);
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
        if let Some(hits) = self.pinyin_index.get(key.as_str()) {
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

/// The `SEARCH_CONTINUED` probe over a sorted key list: does any stored key
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

/// The `SEARCH_CONTINUED` probe over the typed pinyin index.
fn pinyin_prefix_exists(index: &PinyinIndex, joined: &str) -> bool {
    if index.contains_key(joined) {
        return true;
    }
    index
        .range::<str, _>((Included(joined), Unbounded))
        .next()
        .is_some_and(|(key, _)| {
            key.starts_with(joined) && key.as_bytes().get(joined.len()) == Some(&b'\'')
        })
}

struct PinyinDerived {
    pinyin_index: PinyinIndex,
    unigrams: BTreeMap<u32, u64>,
    unigram_total: u64,
    initial_keys: Box<[String]>,
}

fn load_pinyin_index(path: &Path, phrase_index: &PhraseIndex) -> Result<PinyinDerived, DictError> {
    let mut raw: BTreeMap<Box<str>, Box<[(u32, u32)]>> = BTreeMap::new();
    let mut unigrams: BTreeMap<u32, u64> = BTreeMap::new();
    let mut unigram_total: u64 = 0;
    let mut initial_keys: Vec<String> = Vec::new();

    table::for_each_row(path, |key, value| {
        let pinyin = std::str::from_utf8(key)
            .map_err(|_| DictError::Parse("pinyin index key is not UTF-8".to_owned()))?;
        let mut initial = String::new();
        for (position, syllable) in pinyin.split('\'').enumerate() {
            if position > 0 {
                initial.push('\'');
            }
            match syllable_initial(syllable) {
                Some(prefix) => initial.push_str(prefix),
                None => initial.push('0'),
            }
        }
        initial_keys.push(initial);

        let records = parse_index_records(value)?;
        for &(token, freq) in records.iter() {
            let count = unigrams.entry(token).or_default();
            *count = count.saturating_add(u64::from(freq));
            unigram_total = unigram_total.saturating_add(u64::from(freq));
        }
        raw.insert(Box::from(pinyin), records);
        Ok::<(), DictError>(())
    })?;

    initial_keys.sort_unstable();
    initial_keys.dedup();
    // Totals are the sum over every pronunciation; resolve after the
    // aggregate pass so each hit carries the item's final unigram.
    let pinyin_index = raw
        .into_iter()
        .map(|(key, records)| (key, resolve_hits(&records, phrase_index, &unigrams)))
        .collect();
    Ok(PinyinDerived {
        pinyin_index,
        unigrams,
        unigram_total,
        initial_keys: initial_keys.into_boxed_slice(),
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
    unigrams: &BTreeMap<u32, u64>,
) -> Box<[PhraseEntry]> {
    records
        .iter()
        .filter_map(|&(token, freq)| {
            phrase_index.get(&token).map(|text| {
                let total = unigrams.get(&token).copied().unwrap_or(0);
                PhraseEntry::new(PhraseToken::new(token), text.clone())
                    .with_pronunciation_possibility(u64::from(freq), total)
            })
        })
        .collect()
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
        map.insert(token, CompactString::from(text));
        Ok::<(), DictError>(())
    })?;
    Ok(map)
}

/// Parses an index value as `{token: u32 LE, freq: u32 LE}` records.
fn parse_index_records(data: &[u8]) -> Result<Box<[(u32, u32)]>, DictError> {
    if !data.len().is_multiple_of(8) {
        return Err(DictError::Parse(format!(
            "index value length {} is not a multiple of 8",
            data.len()
        )));
    }
    Ok(data
        .chunks_exact(8)
        .map(|chunk| {
            (
                u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
            )
        })
        .collect())
}

/// Builds the exact-text → tokens reverse map from the typed phrase index.
fn build_text_tokens(phrase_index: &PhraseIndex) -> Result<BTreeMap<String, Vec<u32>>, DictError> {
    let mut map: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for (&token, text) in phrase_index {
        map.entry(text.to_string()).or_default().push(token);
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
    fn mini_fixture_opens() {
        assert_eq!(dict().key_count().unwrap(), 10);
    }

    #[test]
    fn single_syllable_is_frequency_ranked() {
        let entries = dict().lookup(&[key("ni")]).unwrap();
        assert!(!entries.is_empty());
        // 你 dominates the pin's ni column; the exporter froze
        // frequency-descending order.
        assert_eq!(entries[0].text(), "你");
    }

    #[test]
    fn multi_syllable_lookup_is_one_string_key() {
        let entries = dict().lookup(&[key("ni"), key("hao")]).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].text(), "你好");

        let entries = dict().lookup(&[key("zhong"), key("guo")]).unwrap();
        assert!(entries.iter().any(|entry| entry.text() == "中国"));
    }

    #[test]
    fn apostrophe_keeps_xian_and_xi_an_apart() {
        let xian = dict().lookup(&[key("xian")]).unwrap();
        assert!(xian.iter().any(|entry| entry.text() == "现"));
        assert!(!xian.iter().any(|entry| entry.text() == "西安"));

        let xi_an = dict().lookup(&[key("xi"), key("an")]).unwrap();
        assert!(xi_an.iter().any(|entry| entry.text() == "西安"));
        assert!(!xi_an.iter().any(|entry| entry.text() == "现"));
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
        for records in dict.pinyin_index.values() {
            for entry in records.iter() {
                reachable.insert(entry.token().value());
            }
        }

        for token in dict.phrase_index.keys() {
            assert!(
                reachable.contains(token),
                "phrase token {token:#010x} is in phrase_index but no pinyin_index entry references it"
            );
        }
    }

    #[test]
    fn unknown_sequence_is_empty_not_an_error() {
        let entries = dict().lookup(&[key("zhuang"), key("zhuang")]).unwrap();
        assert!(entries.is_empty());
        let entries = dict().lookup(&[]).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn lookup_into_matches_lookup_and_keeps_capacity() {
        let dict = dict();
        let via_lookup = dict.lookup(&[key("ni")]).unwrap();
        let mut buf = Vec::with_capacity(via_lookup.len().max(1));
        dict.lookup_into(&[key("ni")], &mut buf).unwrap();
        assert_eq!(buf, via_lookup);
        let cap = buf.capacity();
        dict.lookup_into(&[key("ni"), key("hao")], &mut buf)
            .unwrap();
        assert_eq!(buf, dict.lookup(&[key("ni"), key("hao")]).unwrap());
        assert!(buf.capacity() >= cap);
    }

    #[test]
    fn phrase_text_and_pronunciations_reverse_the_index() {
        let dict = dict();
        let entries = dict.lookup(&[key("ni")]).unwrap();
        let lead = &entries[0];
        assert_eq!(
            dict.phrase_text(lead.token().value()).unwrap().as_deref(),
            Some(lead.text())
        );
        // The lead's pronunciation list contains the lookup key itself.
        let pronunciations = dict.pronunciations(lead.token().value()).unwrap();
        assert!(
            pronunciations.iter().any(|(pinyin, _)| pinyin == "ni"),
            "lead token must carry the ni reading"
        );
        // Unknown tokens reverse to nothing, not to an error.
        assert!(dict.phrase_text(u32::MAX).unwrap().is_none());
        assert!(dict.pronunciations(u32::MAX).unwrap().is_empty());
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
}
