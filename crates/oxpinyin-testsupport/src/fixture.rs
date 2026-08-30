// No deps beyond oxpinyin-core; bounded by fixture file size. Originally
// `oxpinyin-core/src/fixture.rs`; moved to this test-support crate so the
// production build graph carries no test doubles. See fixture-adapters.md.

//! Fixture-backed [`Dictionary`] and [`LanguageModel`].
//!
//! These are what the decoder is developed and tested against: no installed
//! oracle, no redb, no W3 dependency, and portable everywhere. The data is
//! frozen in `fixtures/w4/`, and `docs/testing/fixture-adapters.md` records
//! its provenance — phrase text is captured pinned-oracle output, key
//! sequences and weights are authored, and no model-archive probability data
//! is present.
//!
//! Parsing takes `&str`, not a path: `oxpinyin-core` performs no I/O. Callers
//! `include_str!` the fixtures or read them themselves.
//!
//! The doubles live in their own crate, which test crates take as an
//! ordinary (dev-)dependency. A cargo feature on a shipping crate — the
//! previous alternative — would let `cargo test --workspace` skip these
//! tests silently whenever nothing happened to enable it.

use core::fmt;
use std::collections::BTreeMap;

use oxpinyin_core::cost::{self, UNKNOWN_COST};
use oxpinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};

/// Weight of the bigram term in the interpolated estimate, as a fraction.
///
/// One half: an authored, deliberately neutral value for this **portable test
/// double**, whose data is captured/authored, not the real model. The
/// shipping decoder (`oxpinyin_data::lm`) instead reads λ from the model's
/// `table.conf` (`0.312699` for the pinned model, `data-formats.md` §3); the
/// fixture keeps a neutral λ because its authored mini-bigram is not the
/// pinned model's distribution, so tracking the real λ here would buy no
/// parity and only re-calibrate the fixture pins. See
/// `docs/findings/scoring-spec.md`.
const LAMBDA_NUMERATOR: u128 = 1;
/// Denominator of [`LAMBDA_NUMERATOR`].
const LAMBDA_DENOMINATOR: u128 = 2;

/// Why a fixture could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
#[non_exhaustive]
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
        let count = records.len();

        for record in records {
            entries
                .entry(record.keys)
                .or_default()
                .push(PhraseEntry::new(record.token, record.text));
        }

        Ok(Self { entries, count })
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
    type Entry = PhraseEntry;
    type Error = FixtureError;
    type Syllable = SyllableKey;

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, FixtureError> {
        Ok(self.entries.get(syllables).cloned().unwrap_or_default())
    }
}

/// An in-memory interpolated bigram model read from fixtures.
#[derive(Clone, Debug, Default)]
pub struct FixtureLanguageModel {
    unigrams: BTreeMap<PhraseToken, u64>,
    unigram_total: u64,
    bigrams: BTreeMap<(PhraseToken, PhraseToken), u64>,
    bigram_totals: BTreeMap<PhraseToken, u64>,
}

impl FixtureLanguageModel {
    /// Reads a vocabulary fixture for unigrams and a bigram fixture for pairs.
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

        let mut pairs = BTreeMap::new();
        let mut totals: BTreeMap<PhraseToken, u64> = BTreeMap::new();
        for (index, raw) in bigrams.lines().enumerate() {
            let line = index + 1;
            let Some(fields) = record_fields(raw) else {
                continue;
            };

            let previous = token_field(&fields, "prev", line, &unigrams)?;
            let next = token_field(&fields, "next", line, &unigrams)?;
            let count = number(&fields, "count", line)?;

            // Saturate like the unigram total above: fixture counts are
            // untrusted text, and wrapping totals would poison every cost.
            let current = *totals.entry(previous).or_default();
            totals.insert(previous, current.saturating_add(count));
            pairs.insert((previous, next), count);
        }

        Ok(Self {
            unigrams,
            unigram_total,
            bigrams: pairs,
            bigram_totals: totals,
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

    /// Number of bigram pairs held.
    #[must_use]
    pub fn bigram_count(&self) -> usize {
        self.bigrams.len()
    }

    /// The interpolated model cost of `token` after `history`, without the
    /// edge cost.
    ///
    /// `P(next | prev) = λ·P_bigram + (1 − λ)·P_unigram`, evaluated as one
    /// exact integer ratio so no intermediate rounding enters, then converted
    /// by [`cost::surprisal`].
    #[must_use]
    pub fn model_cost(&self, history: &[PhraseToken], token: &PhraseToken) -> Cost {
        let unigram = u128::from(self.unigrams.get(token).copied().unwrap_or_default());
        let unigram_total = u128::from(self.unigram_total);
        if unigram == 0 || unigram_total == 0 {
            return UNKNOWN_COST;
        }

        let bigram = history
            .last()
            .and_then(|previous| {
                let count = self.bigrams.get(&(*previous, *token)).copied()?;
                let total = self.bigram_totals.get(previous).copied()?;
                (total > 0).then_some((u128::from(count), u128::from(total)))
            })
            .unwrap_or((0, 1));

        // λ·b/bt + (1 − λ)·u/ut over a common denominator. The u128
        // products still overflow when the u64 totals approach their cap,
        // so mirror the production LM (`data::lm::interpolate_ratio`) and
        // degrade to UNKNOWN_COST instead of wrapping (audit F-2).
        let (bigram_count, bigram_total) = bigram;
        let numerator = LAMBDA_NUMERATOR
            .checked_mul(bigram_count)
            .and_then(|t| t.checked_mul(unigram_total))
            .and_then(|t| {
                (LAMBDA_DENOMINATOR - LAMBDA_NUMERATOR)
                    .checked_mul(unigram)
                    .and_then(|u| u.checked_mul(bigram_total))
                    .and_then(|u| t.checked_add(u))
            });
        let denominator = LAMBDA_DENOMINATOR
            .checked_mul(bigram_total)
            .and_then(|t| t.checked_mul(unigram_total));
        let (Some(numerator), Some(denominator)) = (numerator, denominator) else {
            return UNKNOWN_COST;
        };

        let (numerator, denominator) = cost::reduce_ratio(numerator, denominator);
        cost::surprisal(numerator, denominator)
    }
}

impl LanguageModel for FixtureLanguageModel {
    type Error = FixtureError;
    type Token = PhraseToken;

    fn score(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
        edge_cost: Cost,
    ) -> Result<Cost, FixtureError> {
        Ok(edge_cost.saturating_add(self.model_cost(history, token)))
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

#[cfg(test)]
mod tests {
    #[test]
    fn extreme_counts_saturate_and_degrade_to_unknown_cost() {
        // Regression (docs/safety/oxpinyin-audit.md F-1/F-2): two
        // u64::MAX counts sharing a prev used to wrap the bigram total
        // (overflow panic in debug, silent wrap in release), and the u128
        // interpolation products overflowed unchecked. Parsing must
        // saturate the totals and the cost must fall back to
        // UNKNOWN_COST rather than panic or wrap.
        let vocab = "token=1\tkeys=ni\ttext=你\tunigram=18446744073709551615\n\
                     token=2\tkeys=ni\ttext=尼\tunigram=18446744073709551615\n";
        let bigrams = "prev=1\tnext=2\tcount=18446744073709551615\n\
                       prev=1\tnext=1\tcount=18446744073709551615\n";
        let model =
            FixtureLanguageModel::parse(vocab, bigrams).expect("the extreme-count fixture parses");
        // Pin the saturation itself: a wrapped total would also degrade
        // to UNKNOWN_COST below, so only this assertion distinguishes
        // saturating from wrapping.
        assert_eq!(
            model.bigram_totals.get(&PhraseToken::new(1)),
            Some(&u64::MAX)
        );
        assert_eq!(
            model.model_cost(&[PhraseToken::new(1)], &PhraseToken::new(2)),
            UNKNOWN_COST
        );
    }

    use super::{FixtureDictionary, FixtureError, FixtureLanguageModel};
    use oxpinyin_core::cost::UNKNOWN_COST;
    use oxpinyin_core::{Dictionary, LanguageModel, PhraseToken, SyllableKey};

    const VOCAB: &str = include_str!("../../../fixtures/w4/mini-vocab.txt");
    const BIGRAM: &str = include_str!("../../../fixtures/w4/mini-bigram.txt");

    fn keys(text: &str) -> Vec<SyllableKey> {
        text.split(',')
            .map(|key| SyllableKey::from_text(key).expect("frozen key"))
            .collect()
    }

    fn dictionary() -> FixtureDictionary {
        FixtureDictionary::parse(VOCAB).expect("the committed fixture parses")
    }

    fn model() -> FixtureLanguageModel {
        FixtureLanguageModel::parse(VOCAB, BIGRAM).expect("the committed fixtures parse")
    }

    #[test]
    fn the_committed_vocabulary_loads() {
        let dictionary = dictionary();
        assert_eq!(dictionary.len(), 90);
        assert!(!dictionary.is_empty());
        assert_eq!(dictionary.max_keys(), 3);
    }

    #[test]
    fn lookup_preserves_the_captured_candidate_order() {
        let dictionary = dictionary();
        let entries = dictionary.lookup(&keys("ni")).expect("lookup cannot fail");
        let texts: Vec<&str> = entries.iter().map(|entry| entry.text()).collect();
        assert_eq!(
            texts,
            ["你", "尼", "呢", "泥", "妮", "拟", "逆", "倪", "腻", "溺"]
        );

        let phrase = dictionary.lookup(&keys("zhong,guo,ren")).expect("lookup");
        assert_eq!(phrase.len(), 1);
        assert_eq!(phrase[0].text(), "中国人");
    }

    #[test]
    fn an_absent_key_sequence_is_an_empty_lookup() {
        let dictionary = dictionary();
        assert!(
            dictionary
                .lookup(&keys("zhuang"))
                .expect("lookup cannot fail")
                .is_empty()
        );
        assert!(
            dictionary
                .lookup(&[])
                .expect("lookup cannot fail")
                .is_empty()
        );
    }

    #[test]
    fn lookups_are_deterministic() {
        let dictionary = dictionary();
        for sequence in ["ni", "ni,hao", "zhong,guo", "xi,an", "fang,an"] {
            let first = dictionary.lookup(&keys(sequence)).expect("lookup");
            let second = dictionary.lookup(&keys(sequence)).expect("lookup");
            assert_eq!(first, second, "sequence: {sequence}");
        }
    }

    #[test]
    fn the_model_ranks_the_captured_order() {
        let model = model();
        assert_eq!(model.len(), 90);
        assert_eq!(model.bigram_count(), 8);

        let dictionary = dictionary();
        let entries = dictionary.lookup(&keys("ni")).expect("lookup");
        let mut previous = None;
        for entry in &entries {
            let cost = model.model_cost(&[], &entry.token());
            if let Some(previous) = previous {
                assert!(
                    cost >= previous,
                    "{} scored better than a higher-ranked phrase",
                    entry.text()
                );
            }
            previous = Some(cost);
        }
    }

    #[test]
    fn a_known_bigram_beats_the_unigram_estimate() {
        let model = model();
        let nihao = PhraseToken::new(11);
        let zhongguo = PhraseToken::new(33);
        let xian = PhraseToken::new(59);

        let alone = model.model_cost(&[], &zhongguo);
        let after = model.model_cost(&[nihao], &zhongguo);
        assert!(after < alone, "the bigram must help: {after} vs {alone}");

        let unrelated = model.model_cost(&[xian], &zhongguo);
        assert_eq!(unrelated, alone, "an absent pair falls back to the unigram");
    }

    #[test]
    fn an_unknown_token_costs_the_documented_floor() {
        let model = model();
        assert_eq!(
            model.model_cost(&[], &PhraseToken::new(9_999)),
            UNKNOWN_COST
        );
        assert_eq!(
            model
                .score(&[], &PhraseToken::new(9_999), 7)
                .expect("scoring cannot fail"),
            7 + UNKNOWN_COST
        );
    }

    #[test]
    fn scoring_adds_the_edge_cost_and_is_deterministic() {
        let model = model();
        let token = PhraseToken::new(1);
        let base = model.score(&[], &token, 0).expect("scoring");
        assert_eq!(model.score(&[], &token, 250).expect("scoring"), base + 250);
        assert_eq!(model.score(&[], &token, 0).expect("scoring"), base);
    }

    #[test]
    fn malformed_records_are_reported_with_their_line() {
        let failure = |text: &str| {
            FixtureDictionary::parse(text).expect_err("the record is malformed on purpose")
        };

        assert_eq!(
            failure("token=1\tkeys=ni"),
            FixtureError::MissingField {
                line: 1,
                field: "text"
            }
        );
        assert_eq!(
            failure("token=1\tkeys=qqq\ttext=x\tunigram=1"),
            FixtureError::UnknownKey {
                line: 1,
                key: "qqq".to_owned()
            }
        );
        assert_eq!(
            failure("token=1\tkeys=ni\ttext=x\tunigram=1\ntoken=1\tkeys=ni\ttext=y\tunigram=1"),
            FixtureError::DuplicateToken { line: 2, token: 1 }
        );
        assert_eq!(
            failure("token=1\tkeys=ni\ttext=x\tunigram=nope"),
            FixtureError::InvalidNumber {
                line: 1,
                field: "unigram",
                value: "nope".to_owned()
            }
        );
        assert_eq!(
            FixtureLanguageModel::parse(
                "token=1\tkeys=ni\ttext=x\tunigram=1",
                "prev=1\tnext=2\tcount=1"
            )
            .expect_err("the bigram names an undefined token"),
            FixtureError::UnknownToken { line: 1, token: 2 }
        );

        for message in [
            FixtureError::MissingField {
                line: 1,
                field: "text",
            }
            .to_string(),
            FixtureError::UnknownKey {
                line: 2,
                key: "qqq".to_owned(),
            }
            .to_string(),
            FixtureError::DuplicateToken { line: 3, token: 1 }.to_string(),
            FixtureError::InvalidNumber {
                line: 4,
                field: "unigram",
                value: "nope".to_owned(),
            }
            .to_string(),
            FixtureError::UnknownToken { line: 5, token: 2 }.to_string(),
        ] {
            assert!(message.starts_with("line "), "message: {message}");
        }
    }

    #[test]
    fn comments_and_blank_lines_are_skipped() {
        let dictionary = FixtureDictionary::parse(
            "# header\n\n  \ntoken=1\tkeys=ni\ttext=x\tunigram=1\t# trailing\n",
        )
        .expect("comments parse");
        assert_eq!(dictionary.len(), 1);
    }
}
