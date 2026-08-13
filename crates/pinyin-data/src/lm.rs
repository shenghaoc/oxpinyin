//! Bigram language model over the verbatim-copied system bigram.
//!
//! Implements [`pinyin_core::LanguageModel`] on the byte format frozen in
//! `docs/findings/data-layer-export.md`: each key is the previous
//! `phrase_token_t` as 4 bytes little-endian; each value is a `total: u32`
//! followed by 8-byte `{next_token: u32, count: u32}` records, with
//! `total == Σ count`.
//!
//! Scoring follows the interpolated form frozen in
//! `docs/findings/scoring-spec.md`:
//!
//! ```text
//! P(w_n | w_n-1) = λ · P_bigram(w_n | w_n-1) + (1 − λ) · P_unigram(w_n)
//! ```
//!
//! with provisional λ = 1/2. Unigram counts are installed from a
//! [`crate::SystemDictionary`]'s aggregated export frequencies. Without
//! unigrams the model falls back to pure bigram-or-floor behaviour so the
//! mini-fixture unit tests stay self-contained.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use pinyin_core::cost::{UNKNOWN_COST, reduce_ratio, surprisal};
use pinyin_core::{Cost, LanguageModel, PhraseToken};

use crate::interp::{self, InterpolationError, UnigramTable};
use crate::table::{LookupTable, TableError};

/// Error conditions for bigram lookups.
#[derive(Debug)]
pub enum LmError {
    /// A table-level error (I/O, redb, etc.).
    Table(TableError),
    /// Value bytes did not parse under the frozen bigram schema.
    Parse(String),
}

impl fmt::Display for LmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for LmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
            Self::Parse(_) => None,
        }
    }
}

impl From<TableError> for LmError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

/// Weight of the bigram term in the interpolated estimate (λ = 1/2).
///
/// Authored and deliberately neutral; same provisional value as
/// `pinyin_core::fixture` and `docs/findings/scoring-spec.md`.
const LAMBDA_NUMERATOR: u128 = 1;
/// Denominator of [`LAMBDA_NUMERATOR`].
const LAMBDA_DENOMINATOR: u128 = 2;

/// Divides empty-history unigram surprisal so it ranks by frequency without
/// overpowering `phrase_key_bonus`.
///
/// Measured on the pin export: 你 vs 你好 differ by ~11,615 cost units of
/// raw unigram surprisal. With `phrase_key_bonus` at its swept value of
/// 1,000, full unigram would still drown coverage; a factor of 16 leaves a
/// ~700-unit frequency signal — enough to order same-length phrases, small
/// enough that a two-key phrase still wins.
const UNIGRAM_TIEBREAK_SCALE: i64 = 16;

/// Bigram language model backed by `bigram.redb`.
pub struct BigramLanguageModel {
    bigram: LookupTable,
    unigrams: Option<UnigramTable>,
    unigram_total: u64,
    /// Whether `unigrams` came from `interpolation2.text`: only the phrase
    /// index's real counts switch the engine to its pinned construction. The
    /// export-ABI map (flat 100s) keeps feeding the interpolated cost but is
    /// never mistaken for real frequencies.
    real_unigrams: bool,
}

impl BigramLanguageModel {
    /// Opens the bigram model from a redb table file.
    pub fn open(path: &Path) -> Result<Self, LmError> {
        Ok(Self {
            bigram: LookupTable::open(path).map_err(LmError::Table)?,
            unigrams: None,
            unigram_total: 0,
            real_unigrams: false,
        })
    }

    /// Installs unigram counts aggregated from a [`crate::SystemDictionary`].
    ///
    /// Call after opening both tables and before constructing a
    /// [`pinyin_core::scoring::Scorer`]. Scoring falls back to pure
    /// bigram-or-floor behaviour when no unigrams have been installed, so the
    /// fixture-only unit tests remain self-contained. These counts are the
    /// export ABI's — not the phrase index's real frequencies — and are never
    /// reported through [`pinyin_core::LanguageModel::unigram_freq`].
    pub fn set_unigrams(&mut self, unigrams: BTreeMap<u32, u64>, total: u64) {
        self.unigrams = Some(UnigramTable::from_map(unigrams));
        self.unigram_total = total;
        self.real_unigrams = false;
    }

    /// Installs the phrase index's **real** unigram counts from an
    /// `interpolation2.text` model export in the fetched model cache.
    ///
    /// This replaces the export-ABI counts ([`Self::set_unigrams`],
    /// [`Self::set_unigrams_from_dict`]), which report a flat `100` for every
    /// multi-character phrase. The real counts are what the pinned oracle
    /// ranks candidates by, so parity with its candidate construction requires
    /// them; without them the model keeps behaving as it did before.
    ///
    /// The caller resolves the cache path (`PINYIN_MODEL_DIR` /
    /// `tools/model/fetch-model.sh`); this crate discovers nothing.
    ///
    /// # Errors
    ///
    /// Returns [`InterpolationError`] when the file cannot be read or parsed.
    pub fn set_unigrams_from_interpolation2(
        &mut self,
        path: &Path,
    ) -> Result<(), InterpolationError> {
        let table = interp::parse_interpolation2(path)?;
        self.unigram_total = table.total();
        self.unigrams = Some(table);
        self.real_unigrams = true;
        Ok(())
    }

    /// Convenience: installs unigrams from a dictionary's aggregated map.
    pub fn set_unigrams_from_dict(&mut self, dict: &crate::SystemDictionary) {
        self.set_unigrams(dict.unigram_map().clone(), dict.unigram_total());
    }

    /// Number of previous-token entries.
    pub fn entry_count(&self) -> Result<u64, LmError> {
        self.bigram.len().map_err(LmError::Table)
    }

    /// Whether unigram counts have been installed for interpolation.
    #[must_use]
    pub fn has_unigrams(&self) -> bool {
        self.unigram_total > 0
            && self
                .unigrams
                .as_ref()
                .is_some_and(|table| !table.is_empty())
    }

    /// The installed unigram count of `token`.
    ///
    /// `None` when no unigram table is installed at all; `Some(0)` when a
    /// table is installed but the phrase index has no such token. A real
    /// table counts phrases the n-gram corpus never saw as zero rather than
    /// absent, which is what lets candidate ranking put them last among equal
    /// keys.
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        self.unigrams
            .as_ref()
            .map(|table| table.count(token).unwrap_or(0))
    }

    /// The sum of the installed unigram counts, `0` when none are installed.
    #[must_use]
    pub const fn unigram_total(&self) -> u64 {
        self.unigram_total
    }

    /// Returns `(count, total)` for the `prev → next` transition, or `None`
    /// when `prev` has no bigram entry.
    fn transition(&self, prev: u32, next: u32) -> Result<Option<(u32, u32)>, LmError> {
        let Some(raw) = self
            .bigram
            .get(&prev.to_le_bytes())
            .map_err(LmError::Table)?
        else {
            return Ok(None);
        };
        let (total, records) = parse_bigram_value(&raw)?;
        let count = records
            .iter()
            .find(|(next_token, _)| *next_token == next)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        Ok(Some((count, total)))
    }

    /// Interpolated model cost of `token` after `history`, without `edge_cost`.
    ///
    /// Empty-history ranking: a *scaled-down* unigram surprisal acts as a
    /// tie-break within the same structural cost. Full unigram surprisal
    /// (~8–20k on the export) drowns the provisional `phrase_key_bonus` and
    /// makes longer phrases lose to their first syllable; dividing by
    /// [`UNIGRAM_TIEBREAK_SCALE`] keeps the order of frequencies without
    /// undoing coverage credit. Bigram transitions keep full scale — that is
    /// the term that should move multi-phrase sentences. A history whose
    /// previous token has no bigram entry floors at [`UNKNOWN_COST`], like a
    /// count-0 next-token, so an unseen transition never undercuts a rare but
    /// observed one.
    fn model_cost(&self, history: &[PhraseToken], token: &PhraseToken) -> Result<Cost, LmError> {
        if !self.has_unigrams() {
            return self.pure_bigram_cost(history, token);
        }

        let unigram = self.unigram_count(token.value()).unwrap_or(0);
        if unigram == 0 || self.unigram_total == 0 {
            return Ok(UNKNOWN_COST);
        }

        let unigram_cost = surprisal(unigram, self.unigram_total);

        let Some(prev) = history.last() else {
            return Ok(unigram_cost / UNIGRAM_TIEBREAK_SCALE);
        };

        match self.transition(prev.value(), token.value())? {
            Some((bigram_count, bigram_total)) if bigram_total > 0 => {
                // λ·b/bt + (1 − λ)·u/ut over a common denominator.
                let unigram_128 = u128::from(unigram);
                let unigram_total = u128::from(self.unigram_total);
                let bigram_count = u128::from(bigram_count);
                let bigram_total = u128::from(bigram_total);
                let numerator = LAMBDA_NUMERATOR * bigram_count * unigram_total
                    + (LAMBDA_DENOMINATOR - LAMBDA_NUMERATOR) * unigram_128 * bigram_total;
                let denominator = LAMBDA_DENOMINATOR * bigram_total * unigram_total;
                let (numerator, denominator) = reduce_ratio(numerator, denominator);
                Ok(surprisal(numerator, denominator))
            }
            // Previous token absent from the bigram (no evidence of this
            // transition at all), or a degenerate zero-total entry: floor at
            // UNKNOWN_COST — the same floor a count-0 next-token gets — so an
            // unseen transition never scores cheaper than a rare observed one.
            _ => Ok(UNKNOWN_COST),
        }
    }

    /// Pre-interpolation behaviour: pure bigram when history exists, else
    /// pass-through (dictionary order carries unigram ranking).
    fn pure_bigram_cost(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
    ) -> Result<Cost, LmError> {
        let Some(prev) = history.last() else {
            return Ok(0);
        };
        match self.transition(prev.value(), token.value())? {
            Some((count, total)) => Ok(surprisal(u64::from(count), u64::from(total))),
            None => Ok(UNKNOWN_COST),
        }
    }
}

impl LanguageModel for BigramLanguageModel {
    type Token = PhraseToken;
    type Error = LmError;

    fn score(
        &self,
        history: &[Self::Token],
        token: &Self::Token,
        edge_cost: Cost,
    ) -> Result<Cost, Self::Error> {
        Ok(edge_cost.saturating_add(self.model_cost(history, token)?))
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        // Only the interpolation2 table is a real frequency table; the
        // export-ABI map is a scoring input, not candidate-ranking data.
        Ok(self
            .real_unigrams
            .then(|| self.unigram_count(token.value()).unwrap_or(0)))
    }
}

/// Parses a bigram value as `(total, [{next_token, count}])`.
fn parse_bigram_value(data: &[u8]) -> Result<(u32, Vec<(u32, u32)>), LmError> {
    if data.len() < 4 || !(data.len() - 4).is_multiple_of(8) {
        return Err(LmError::Parse(format!(
            "bigram value length {} is not 4 + 8n",
            data.len()
        )));
    }
    let total = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let records = data[4..]
        .chunks_exact(8)
        .map(|chunk| {
            (
                u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
                u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]),
            )
        })
        .collect();
    Ok((total, records))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 你's gb_char token in the pinned model.
    const NI: u32 = 0x0100_1225;
    /// 的's gb_char token in the pinned model.
    const DE: u32 = 0x0100_05db;

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

    fn model() -> BigramLanguageModel {
        BigramLanguageModel::open(&fixtures_dir().join("bigram.redb")).unwrap()
    }

    #[test]
    fn mini_fixture_opens() {
        assert!(model().entry_count().unwrap() > 0);
    }

    #[test]
    fn observed_transition_is_cheaper_than_novel() {
        let model = model();
        let history = [PhraseToken::new(NI)];
        let observed = model
            .score(&history, &PhraseToken::new(DE), 0)
            .expect("你 → 的 scores");
        let novel = model
            .score(&history, &PhraseToken::new(0x0100_0001), 0)
            .expect("你 → rare scores");
        assert!(
            observed < novel,
            "你 → 的 ({observed}) must undercut a novel transition ({novel})"
        );
    }

    #[test]
    fn empty_history_returns_edge_cost_without_unigrams() {
        let cost = model().score(&[], &PhraseToken::new(DE), 1234).unwrap();
        assert_eq!(cost, 1234);
    }

    #[test]
    fn empty_history_uses_scaled_unigram_when_installed() {
        let mut model = model();
        let mut unigrams = BTreeMap::new();
        unigrams.insert(DE, 100);
        unigrams.insert(NI, 10);
        model.set_unigrams(unigrams, 110);

        let de = model.score(&[], &PhraseToken::new(DE), 0).unwrap();
        let ni = model.score(&[], &PhraseToken::new(NI), 0).unwrap();
        assert!(
            de < ni,
            "higher unigram count must cost less: de={de} ni={ni}"
        );
        // Scaled, not the raw surprisal floor for a known token.
        let raw = surprisal(100, 110);
        assert_eq!(de, raw / UNIGRAM_TIEBREAK_SCALE);
        assert_eq!(
            model.score(&[], &PhraseToken::new(0x0100_0001), 0).unwrap(),
            UNKNOWN_COST
        );
    }

    #[test]
    fn interpolation_prefers_observed_bigram() {
        let mut model = model();
        let mut unigrams = BTreeMap::new();
        // Equal unigrams so only the bigram term can separate them.
        unigrams.insert(DE, 50);
        unigrams.insert(0x0100_0001, 50);
        model.set_unigrams(unigrams, 100);

        let history = [PhraseToken::new(NI)];
        let observed = model
            .score(&history, &PhraseToken::new(DE), 0)
            .expect("你 → 的");
        let novel = model
            .score(&history, &PhraseToken::new(0x0100_0001), 0)
            .expect("你 → rare");
        assert!(
            observed < novel,
            "interpolated 你 → 的 ({observed}) must undercut a novel pair ({novel})"
        );
    }

    #[test]
    fn a_no_entry_history_floors_instead_of_discounting() {
        // Regression: when the previous token has no bigram entry at all, the
        // transition is *unseen*, not merely rare. It must floor at
        // UNKNOWN_COST — the same floor a count-0 next-token gets — never a
        // discounted unigram, which used to rank an unseen transition below a
        // rare but observed one.
        let mut model = model();
        let mut unigrams = BTreeMap::new();
        unigrams.insert(DE, 100);
        model.set_unigrams(unigrams, 110);

        const NO_ENTRY_PREV: u32 = 0xFFFF_FFFF;
        assert!(
            matches!(model.transition(NO_ENTRY_PREV, DE), Ok(None)),
            "precondition: the previous token must be absent from the bigram"
        );

        let unseen = model
            .score(&[PhraseToken::new(NO_ENTRY_PREV)], &PhraseToken::new(DE), 0)
            .expect("scoring an unseen transition");
        assert_eq!(
            unseen, UNKNOWN_COST,
            "a no-entry history must floor at UNKNOWN_COST, not discount"
        );
    }

    #[test]
    fn invariant_holds_for_every_fixture_entry() {
        let model = model();
        for (key, value) in model.bigram.iter().unwrap() {
            assert_eq!(key.len(), 4, "bigram keys are 4-byte prev tokens");
            let (total, records) = parse_bigram_value(&value).expect("schema parses");
            let sum: u64 = records.iter().map(|(_, count)| u64::from(*count)).sum();
            assert_eq!(u64::from(total), sum, "total == Σ count for {key:02x?}");
        }
    }
}
