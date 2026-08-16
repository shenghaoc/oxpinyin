//! System dictionary backed by the exported pinyin index and phrase index.
//!
//! Implements [`oxpinyin_core::Dictionary`] over the tables that
//! `oxpinyin-migrate export` derives from the pinned oracle's public ABI
//! (`docs/findings/data-layer-export.md`). The index is keyed by the
//! pinyin spelling itself — syllables joined by `'` — so a lookup for
//! `[ni, hao]` is a single get on `ni'hao`; there is no per-syllable
//! binary encoder and no compound binary key. Entries come back in the
//! stored order, which the exporter froze as frequency-descending.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use oxpinyin_core::{
    Completeness, Dictionary, INCOMPLETE_PINYIN_KEYS, PhraseEntry, PhraseToken, SyllableKey,
};

use crate::table::{LookupTable, TableError};

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
    pinyin_index: LookupTable,
    phrase_index: LookupTable,
    /// Aggregated phrase frequencies across all pinyin keys, for unigram LM.
    unigrams: BTreeMap<u32, u64>,
    unigram_total: u64,
    /// Every pinyin-index key, sorted: the `SEARCH_CONTINUED` prefix probe
    /// binary-searches this.
    pinyin_keys: Box<[String]>,
    /// Every pinyin-index key projected to its initial sequence, sorted:
    /// the probe the pin uses when the searched sequence holds an
    /// initial-only key. Vowel-initial syllables project to a `0` sentinel.
    initial_keys: Box<[String]>,
}

impl SystemDictionary {
    /// Opens the system dictionary from the two exported table files.
    pub fn open(pinyin_index_path: &Path, phrase_index_path: &Path) -> Result<Self, DictError> {
        let pinyin_index = LookupTable::open(pinyin_index_path)?;
        let phrase_index = LookupTable::open(phrase_index_path)?;
        let (unigrams, unigram_total) = build_unigram_map(&pinyin_index)?;
        let (pinyin_keys, initial_keys) = build_prefix_tables(&pinyin_index)?;
        Ok(Self {
            pinyin_index,
            phrase_index,
            unigrams,
            unigram_total,
            pinyin_keys,
            initial_keys,
        })
    }

    /// Number of pinyin keys in the index.
    pub fn key_count(&self) -> Result<u64, DictError> {
        Ok(self.pinyin_index.len()?)
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
        let key_bytes = token.to_le_bytes();
        let Some(text) = self.phrase_index.get(&key_bytes)? else {
            return Ok(None);
        };
        String::from_utf8(text).map(Some).map_err(|_| {
            DictError::Parse(format!("phrase text for token {token:#010x} is not UTF-8"))
        })
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
        for (key, value) in self.pinyin_index.iter()? {
            let pinyin = String::from_utf8(key)
                .map_err(|_| DictError::Parse("pinyin index key is not UTF-8".to_owned()))?;
            for (candidate, freq) in parse_index_records(&value)? {
                if candidate == token {
                    out.push((pinyin.clone(), u64::from(freq)));
                }
            }
        }
        Ok(out)
    }

    /// The frozen index key for a syllable sequence: texts joined by `'`.
    fn index_key(syllables: &[SyllableKey]) -> String {
        let mut key = String::new();
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
    fn initial_key(syllables: &[SyllableKey]) -> String {
        let mut key = String::new();
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
}

impl Dictionary for SystemDictionary {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        if syllables.is_empty() {
            return Ok(Vec::new());
        }
        let key = Self::index_key(syllables);
        let Some(raw) = self.pinyin_index.get(key.as_bytes())? else {
            return Ok(Vec::new());
        };

        let mut entries = Vec::new();
        for (token, _freq) in parse_index_records(&raw)? {
            // Token → text through phrase_index (the reverse half is
            // `phrase_text`). The full export resolves every token; a mini
            // fixture may omit some, and those records contribute no
            // candidate rather than failing the lookup.
            if let Some(text) = self.phrase_text(token)? {
                entries.push(PhraseEntry::new(PhraseToken::new(token), text));
            }
        }
        Ok(entries)
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
            Ok(prefix_probe(&self.pinyin_keys, &Self::index_key(syllables)))
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

/// The initial of a syllable spelling: the longest initial-only key that is
/// a prefix of it, or `None` for a vowel-initial syllable.
fn syllable_initial(text: &str) -> Option<&'static str> {
    INCOMPLETE_PINYIN_KEYS
        .iter()
        .filter(|key| text.starts_with(**key))
        .max_by_key(|key| key.len())
        .copied()
}

/// The two sorted key lists the prefix probes binary-search: every
/// pinyin-index key, and every key projected to its initial sequence.
type PrefixTables = (Box<[String]>, Box<[String]>);

/// Builds the two sorted key lists the prefix probes binary-search.
fn build_prefix_tables(index: &LookupTable) -> Result<PrefixTables, DictError> {
    let mut pinyin_keys: Vec<String> = Vec::new();
    let mut initial_keys: Vec<String> = Vec::new();

    for (key, _value) in index.iter()? {
        let pinyin = String::from_utf8(key)
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
        pinyin_keys.push(pinyin);
        initial_keys.push(initial);
    }

    pinyin_keys.sort_unstable();
    pinyin_keys.dedup();
    initial_keys.sort_unstable();
    initial_keys.dedup();
    Ok((
        pinyin_keys.into_boxed_slice(),
        initial_keys.into_boxed_slice(),
    ))
}

/// Parses an index value as `{token: u32 LE, freq: u32 LE}` records.
fn parse_index_records(data: &[u8]) -> Result<Vec<(u32, u32)>, DictError> {
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

/// Aggregates `{token → Σ freq}` over every pinyin-index value.
fn build_unigram_map(pinyin_index: &LookupTable) -> Result<(BTreeMap<u32, u64>, u64), DictError> {
    let mut map: BTreeMap<u32, u64> = BTreeMap::new();
    let mut total: u64 = 0;
    for (_, value) in pinyin_index.iter()? {
        for (token, freq) in parse_index_records(&value)? {
            *map.entry(token).or_default() += u64::from(freq);
            total = total.saturating_add(u64::from(freq));
        }
    }
    Ok((map, total))
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
        for (_key, value) in dict.pinyin_index.iter().unwrap() {
            for (token, _freq) in parse_index_records(&value).unwrap() {
                reachable.insert(token);
            }
        }

        for (key, _text) in dict.phrase_index.iter().unwrap() {
            assert_eq!(key.len(), 4, "phrase_index keys are 4-byte tokens");
            let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
            assert!(
                reachable.contains(&token),
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
}
