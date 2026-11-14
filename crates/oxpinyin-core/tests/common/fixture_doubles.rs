//! Fixture-backed [`Dictionary`] and [`LanguageModel`].
//!
//! These are what the decoder is developed and tested against: no installed
//! oracle, no W3 dependency, and portable everywhere. The data is
//! frozen in `fixtures/w4/`, and `docs/testing/fixture-adapters.md` records
//! its provenance — phrase text is captured pinned-oracle output, key
//! sequences and weights are authored, and no model-archive probability data
//! is present.
//!
//! Parsing takes `&str`, not a path: `oxpinyin-core` performs no I/O. Callers
//! `include_str!` the fixtures or read them themselves.
//!
//! This is one source file and no crate. Each test target that needs the doubles
//! includes it with `#[path]`, so it implements the traits of the one
//! `oxpinyin-core` that target links and no shared test crate can close a
//! dependency cycle through core. Not every target uses every item.
#![allow(dead_code)]

use core::fmt;
use std::collections::BTreeMap;

use oxpinyin_core::{Dictionary, LanguageModel, PhraseEntry, PhraseText, PhraseToken, SyllableKey};

/// Why a fixture could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FixtureError {
    /// A record is missing a required field.
    MissingField {
        /// One-based line number.
        line: usize,
        /// Field name.
        field: &'static str,
    },
    /// A field that must be a number is not one.
    InvalidNumber {
        /// One-based line number.
        line: usize,
        /// Field name.
        field: &'static str,
        /// The offending text.
        value: String,
    },
    /// A key sequence names something that is not a frozen pinyin key.
    UnknownKey {
        /// One-based line number.
        line: usize,
        /// The offending text.
        key: String,
    },
    /// Two records claim the same token.
    DuplicateToken {
        /// One-based line number.
        line: usize,
        /// The repeated token.
        token: u32,
    },
    /// A bigram names a token the vocabulary does not define.
    UnknownToken {
        /// One-based line number.
        line: usize,
        /// The undefined token.
        token: u32,
    },
}

impl fmt::Display for FixtureError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField { line, field } => {
                write!(formatter, "line {line}: missing field {field}")
            }
            Self::InvalidNumber { line, field, value } => {
                write!(
                    formatter,
                    "line {line}: field {field} is not a number: {value:?}"
                )
            }
            Self::UnknownKey { line, key } => {
                write!(formatter, "line {line}: {key:?} is not a pinyin key")
            }
            Self::DuplicateToken { line, token } => {
                write!(formatter, "line {line}: token {token} is already defined")
            }
            Self::UnknownToken { line, token } => {
                write!(
                    formatter,
                    "line {line}: token {token} is not in the vocabulary"
                )
            }
        }
    }
}

impl std::error::Error for FixtureError {}

/// One parsed vocabulary record.
#[derive(Clone, Debug)]
struct VocabRecord {
    token: PhraseToken,
    keys: Vec<SyllableKey>,
    text: String,
    unigram: u64,
}

/// Reads `fixtures/w4/mini-vocab.txt`.
///
/// Blank lines, lines starting with `#`, and a trailing field starting with
/// `#` are comments.
fn parse_vocab(text: &str) -> Result<Vec<VocabRecord>, FixtureError> {
    let mut records = Vec::new();
    let mut tokens = BTreeMap::new();

    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let Some(fields) = record_fields(raw) else {
            continue;
        };

        let token = number(&fields, "token", line)?;
        let token = u32::try_from(token).map_err(|_| FixtureError::InvalidNumber {
            line,
            field: "token",
            value: token.to_string(),
        })?;
        if tokens.insert(token, line).is_some() {
            return Err(FixtureError::DuplicateToken { line, token });
        }

        let keys = field(&fields, "keys", line)?;
        let keys = keys
            .split(',')
            .map(|key| {
                SyllableKey::from_text(key).ok_or_else(|| FixtureError::UnknownKey {
                    line,
                    key: key.to_owned(),
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        records.push(VocabRecord {
            token: PhraseToken::new(token),
            keys,
            text: field(&fields, "text", line)?.to_owned(),
            unigram: number(&fields, "unigram", line)?,
        });
    }

    Ok(records)
}

/// Splits a fixture line into `key=value` fields, or `None` for a comment.
fn record_fields(raw: &str) -> Option<Vec<(&str, &str)>> {
    let trimmed = raw.trim_end_matches(['\r', ' ']);
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    let fields: Vec<(&str, &str)> = trimmed
        .split('\t')
        .take_while(|field| !field.trim_start().starts_with('#'))
        .filter_map(|field| field.split_once('='))
        .collect();
    if fields.is_empty() {
        None
    } else {
        Some(fields)
    }
}

fn field<'a>(
    fields: &[(&'a str, &'a str)],
    name: &'static str,
    line: usize,
) -> Result<&'a str, FixtureError> {
    fields
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
        .ok_or(FixtureError::MissingField { line, field: name })
}

fn number(fields: &[(&str, &str)], name: &'static str, line: usize) -> Result<u64, FixtureError> {
    let value = field(fields, name, line)?;
    value.parse().map_err(|_| FixtureError::InvalidNumber {
        line,
        field: name,
        value: value.to_owned(),
    })
}

/// An in-memory dictionary read from a fixture.
///
/// Entries keep the fixture's order within a key sequence, which is the
/// captured candidate order, so lookups are deterministic and reproduce the
/// pin's relative ranking where the capture states one.
#[derive(Clone, Debug, Default)]
pub struct FixtureDictionary {
    entries: BTreeMap<Vec<SyllableKey>, Vec<PhraseEntry>>,
    /// Each token's phrase item: one pronunciation per record that spells
    /// it, toneless, with frequency 1.
    items: BTreeMap<PhraseToken, oxpinyin_core::PronunciationRows>,
    count: usize,
}

impl FixtureDictionary {
    /// Reads a vocabulary fixture.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError`] for a malformed record.
    pub fn parse(text: &str) -> Result<Self, FixtureError> {
        let records = parse_vocab(text)?;
        let mut entries: BTreeMap<Vec<SyllableKey>, Vec<PhraseEntry>> = BTreeMap::new();
        let mut items: BTreeMap<PhraseToken, oxpinyin_core::PronunciationRows> = BTreeMap::new();
        let count = records.len();

        for record in records {
            items
                .entry(record.token)
                .or_default()
                .push((record.keys.iter().map(|key| key.chewing_key()).collect(), 1));
            entries
                .entry(record.keys)
                .or_default()
                .push(PhraseEntry::new(record.token, record.text));
        }

        Ok(Self {
            entries,
            items,
            count,
        })
    }

    /// Number of phrases held.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    /// Whether the dictionary holds nothing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Length in keys of the longest phrase held.
    ///
    /// The decoder uses this to bound how far ahead it looks.
    #[must_use]
    pub fn max_keys(&self) -> usize {
        self.entries.keys().map(Vec::len).max().unwrap_or(0)
    }
}

impl Dictionary for FixtureDictionary {
    type Error = FixtureError;
    type PathMemo = ();
    type Syllable = SyllableKey;

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, FixtureError> {
        Ok(self.entries.get(syllables).cloned().unwrap_or_default())
    }

    /// The §9 suggestion walk over the fixture's own table: every entry
    /// whose key vector strictly extends `syllables` (prefix match), in
    /// fixture order — the capture order the dictionary itself keeps.
    fn suggest_extension_tokens(
        &self,
        syllables: &[SyllableKey],
    ) -> Result<Vec<PhraseToken>, FixtureError> {
        Ok(self
            .entries
            .iter()
            .filter(|(keys, _)| keys.len() > syllables.len() && keys.starts_with(syllables))
            .flat_map(|(_, entries)| entries.iter().map(|entry| entry.token()))
            .collect())
    }

    fn phrase_item_pronunciations(
        &self,
        token: PhraseToken,
    ) -> Result<oxpinyin_core::PhraseItemRead, FixtureError> {
        Ok(self.items.get(&token).cloned().map_or(
            oxpinyin_core::PhraseItemRead::NoItem,
            oxpinyin_core::PhraseItemRead::Item,
        ))
    }

    fn phrase_text_for_token(&self, token: u32) -> Option<PhraseText> {
        let token = PhraseToken::new(token);
        self.entries
            .values()
            .flatten()
            .find(|entry| entry.token() == token)
            .map(|entry| entry.text().clone())
    }
}

/// An in-memory unigram model read from fixtures.
#[derive(Clone, Debug, Default)]
pub struct FixtureLanguageModel {
    unigrams: BTreeMap<PhraseToken, u64>,
    unigram_total: u64,
}

impl FixtureLanguageModel {
    /// Reads a vocabulary fixture for unigrams and checks every bigram
    /// record against it.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError`] for a malformed record, or for a bigram naming
    /// a token the vocabulary does not define.
    pub fn parse(vocab: &str, bigrams: &str) -> Result<Self, FixtureError> {
        let mut unigrams = BTreeMap::new();
        let mut unigram_total = 0_u64;
        for record in parse_vocab(vocab)? {
            unigram_total = unigram_total.saturating_add(record.unigram);
            unigrams.insert(record.token, record.unigram);
        }

        for (index, raw) in bigrams.lines().enumerate() {
            let line = index + 1;
            let Some(fields) = record_fields(raw) else {
                continue;
            };

            token_field(&fields, "prev", line, &unigrams)?;
            token_field(&fields, "next", line, &unigrams)?;
            number(&fields, "count", line)?;
        }

        Ok(Self {
            unigrams,
            unigram_total,
        })
    }

    /// Number of unigrams held.
    #[must_use]
    pub fn len(&self) -> usize {
        self.unigrams.len()
    }

    /// Whether the model holds nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.unigrams.is_empty()
    }
}

impl LanguageModel for FixtureLanguageModel {
    type Error = FixtureError;
    type Token = PhraseToken;
}

/// A [`FixtureLanguageModel`] wrapper that answers the unigram queries, so
/// the engine's window scan and frequency ranking see a frequency table.
///
/// The fixture's authored unigram counts become the frequency table: each
/// token's [`LanguageModel::unigram_freq`] is its fixture count and the total
/// is the fixture's sum. This is enough for the engine to exercise the pinned
/// construction — the three-key order, the expanding-window scan and the
/// amplified-frequency ranking — without a full model archive.
///
/// [`FixtureLanguageModel`] itself answers neither query, so every candidate
/// ranks at frequency zero under it.
#[derive(Clone, Debug)]
pub struct FrequencyFixtureModel {
    inner: FixtureLanguageModel,
}

impl FrequencyFixtureModel {
    /// Wraps an existing [`FixtureLanguageModel`].
    #[must_use]
    pub fn new(inner: FixtureLanguageModel) -> Self {
        Self { inner }
    }

    /// Reads the same vocabulary and bigram fixtures as
    /// [`FixtureLanguageModel::parse`], returning a model that reports real
    /// unigrams.
    ///
    /// # Errors
    ///
    /// Returns [`FixtureError`] for a malformed record.
    pub fn parse(vocab: &str, bigrams: &str) -> Result<Self, FixtureError> {
        FixtureLanguageModel::parse(vocab, bigrams).map(Self::new)
    }
}

impl LanguageModel for FrequencyFixtureModel {
    type Error = FixtureError;
    type Token = PhraseToken;

    fn unigram_freq(&self, token: &PhraseToken) -> Result<Option<u64>, FixtureError> {
        Ok(self.inner.unigrams.get(token).copied())
    }

    fn unigram_total(&self) -> Result<Option<u64>, FixtureError> {
        Ok(Some(self.inner.unigram_total))
    }
}

fn token_field(
    fields: &[(&str, &str)],
    name: &'static str,
    line: usize,
    known: &BTreeMap<PhraseToken, u64>,
) -> Result<PhraseToken, FixtureError> {
    let raw = number(fields, name, line)?;
    let value = u32::try_from(raw).map_err(|_| FixtureError::InvalidNumber {
        line,
        field: name,
        value: raw.to_string(),
    })?;
    let token = PhraseToken::new(value);
    if known.contains_key(&token) {
        Ok(token)
    } else {
        Err(FixtureError::UnknownToken { line, token: value })
    }
}
