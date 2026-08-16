//! Bigram language model over the verbatim-copied system bigram.
//!
//! Implements [`oxpinyin_core::LanguageModel`] on the byte format frozen in
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
//! λ is read from the model's `table.conf` ([`crate::table_conf`]); it
//! defaults to [`crate::table_conf::Lambda::PINNED`] (`0.312699`,
//! `data-formats.md` §3) when no `table.conf` is available. Unigram counts are
//! installed from a [`crate::SystemDictionary`]'s aggregated export
//! frequencies. Without unigrams the model falls back to pure
//! bigram-or-floor behaviour so the mini-fixture unit tests stay
//! self-contained.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use oxpinyin_core::cost::{UNKNOWN_COST, reduce_ratio, surprisal};
use oxpinyin_core::{Cost, LanguageModel, PhraseToken, UserCountDelta};

use crate::interp::{self, InterpolationError, UnigramTable};
use crate::table::{LookupTable, TableError};
use crate::table_conf::Lambda;

/// Error conditions for bigram lookups.
#[derive(Debug)]
pub enum LmError {
    /// A table-level error (I/O, redb, etc.).
    Table(TableError),
    /// Value bytes did not parse under the frozen bigram schema.
    Parse(String),
    /// The user-count overlay failed (a redb read on the user store).
    User(String),
}

impl fmt::Display for LmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::User(msg) => write!(f, "user store error: {msg}"),
        }
    }
}

impl std::error::Error for LmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
            Self::Parse(_) | Self::User(_) => None,
        }
    }
}

impl From<TableError> for LmError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

/// Divides empty-history unigram surprisal so it ranks by frequency without
/// overpowering `phrase_key_bonus`.
///
/// Measured on the pin export: 你 vs 你好 differ by ~11,615 cost units of
/// raw unigram surprisal. With `phrase_key_bonus` at its swept value of
/// 1,000, full unigram would still drown coverage; a factor of 16 leaves a
/// ~700-unit frequency signal — enough to order same-length phrases, small
/// enough that a two-key phrase still wins.
const UNIGRAM_TIEBREAK_SCALE: i64 = 16;

/// Interpolated `(numerator, denominator)` for `λ·b/bt + (1 − λ)·u/ut` over a
/// common denominator, where `λ = lambda_num / lambda_den`.
///
/// Returns `None` if any `u128` product overflows — reachable only for a
/// pathological λ denominator, which the caller floors at `UNKNOWN_COST`
/// rather than panicking (constitution §4). [`Lambda`] keeps
/// `lambda_num ≤ lambda_den`, so the `(1 − λ)` weight never underflows.
fn interpolate_ratio(
    lambda_num: u128,
    lambda_den: u128,
    bigram_count: u128,
    bigram_total: u128,
    unigram: u128,
    unigram_total: u128,
) -> Option<(u128, u128)> {
    let one_minus_lambda = lambda_den.saturating_sub(lambda_num);
    let bigram_term = lambda_num
        .checked_mul(bigram_count)?
        .checked_mul(unigram_total)?;
    let unigram_term = one_minus_lambda
        .checked_mul(unigram)?
        .checked_mul(bigram_total)?;
    let numerator = bigram_term.checked_add(unigram_term)?;
    let denominator = lambda_den
        .checked_mul(bigram_total)?
        .checked_mul(unigram_total)?;
    Some((numerator, denominator))
}

/// Additive merge of one count pair (`merge_single_gram`, §5).
///
/// Saturates rather than wrapping: a user count must not overflow into a
/// wrong score (constitution §4).
#[must_use]
pub const fn merge_counts(system: u64, user: u64) -> u64 {
    system.saturating_add(user)
}

/// Merged `(count, total)` for one bigram, or `None` when neither side has
/// a gram (`merge_single_gram` returns false when both loads miss).
///
/// A system miss with a non-zero user total is the user gram alone. A
/// system hit with a zero user overlay is the system gram alone. Both
/// sides present: saturating addition of each field.
#[must_use]
pub fn merge_bigram(
    system: Option<(u32, u32)>,
    user_count: u64,
    user_total: u64,
) -> Option<(u64, u64)> {
    match system {
        Some((count, total)) => {
            let merged_count = merge_counts(u64::from(count), user_count);
            let merged_total = merge_counts(u64::from(total), user_total);
            if merged_total == 0 {
                None
            } else {
                Some((merged_count, merged_total))
            }
        }
        None if user_total > 0 => Some((user_count, user_total)),
        None => None,
    }
}

/// One previous-token row of the system bigram.
///
/// `total` is the stored row total and equals `Σ count` over [`records`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BigramRow {
    /// Sum of the successor counts.
    pub total: u32,
    /// `(next_token, count)` records, stored order.
    pub records: Vec<(u32, u32)>,
}

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
    /// Bigram/unigram interpolation weight λ, read from the model's
    /// `table.conf` when available and [`Lambda::PINNED`] (`0.312699`,
    /// `data-formats.md` §3) otherwise. Read from config rather than
    /// hardcoded.
    lambda: Lambda,
}

impl BigramLanguageModel {
    /// Opens the bigram model from a redb table file.
    ///
    /// λ defaults to [`Lambda::PINNED`] (`0.312699`, `data-formats.md` §3);
    /// override it from a real install's config with
    /// [`Self::set_lambda_from_table_conf`] or [`Self::set_lambda`].
    pub fn open(path: &Path) -> Result<Self, LmError> {
        Ok(Self {
            bigram: LookupTable::open(path).map_err(LmError::Table)?,
            unigrams: None,
            unigram_total: 0,
            real_unigrams: false,
            lambda: Lambda::PINNED,
        })
    }

    /// The interpolation weight λ currently in effect.
    #[must_use]
    pub const fn lambda(&self) -> Lambda {
        self.lambda
    }

    /// Sets the interpolation weight λ directly.
    pub fn set_lambda(&mut self, lambda: Lambda) {
        self.lambda = lambda;
    }

    /// Reads λ from a model's `table.conf` and installs it (`data-formats.md`
    /// §3).
    ///
    /// Returns `true` when a `table.conf` with a parsable `lambda parameter:`
    /// line was found and applied; `false` when the file is absent or carries
    /// no such line, in which case λ is left unchanged (so the
    /// [`Lambda::PINNED`] default stands for the fetched cache, which ships no
    /// `table.conf`). Never errors: a missing config is the normal case.
    pub fn set_lambda_from_table_conf(&mut self, table_conf_path: &Path) -> bool {
        match crate::table_conf::read_table_conf_lambda(table_conf_path) {
            Some(lambda) => {
                self.lambda = lambda;
                true
            }
            None => false,
        }
    }

    /// Installs unigram counts aggregated from a [`crate::SystemDictionary`].
    ///
    /// Call after opening both tables and before constructing a
    /// [`oxpinyin_core::scoring::Scorer`]. Scoring falls back to pure
    /// bigram-or-floor behaviour when no unigrams have been installed, so the
    /// fixture-only unit tests remain self-contained. These counts are the
    /// export ABI's — not the phrase index's real frequencies — and are never
    /// reported through [`oxpinyin_core::LanguageModel::unigram_freq`].
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

    /// Loads the system-bigram row for `prev`.
    ///
    /// `None` means `prev` has no entry: the same miss
    /// `PhraseLookup::search_bigram2` treats as "skip this node" (no merged
    /// single-gram). A next-token that is not in the row is a `get_freq`
    /// miss, not a zero-count hit.
    ///
    /// # Errors
    ///
    /// Returns [`LmError`] when the table cannot be read or a value does not
    /// parse under the frozen schema.
    pub fn load_successors(&self, prev: u32) -> Result<Option<BigramRow>, LmError> {
        let Some(raw) = self
            .bigram
            .get(&prev.to_le_bytes())
            .map_err(LmError::Table)?
        else {
            return Ok(None);
        };
        let (total, records) = parse_bigram_value(&raw)?;
        Ok(Some(BigramRow { total, records }))
    }

    /// Returns `(count, total)` for the `prev → next` transition, or `None`
    /// when `prev` has no bigram entry.
    fn transition(&self, prev: u32, next: u32) -> Result<Option<(u32, u32)>, LmError> {
        let Some(row) = self.load_successors(prev)? else {
            return Ok(None);
        };
        let count = row
            .records
            .iter()
            .find(|(next_token, _)| *next_token == next)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        Ok(Some((count, row.total)))
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
    ///
    /// `user` is the §5 additive overlay. Zero is identity: the frozen
    /// λ-blend (`docs/findings/scoring-spec.md`) runs over the same counts
    /// as before. Non-zero values are saturating-added to the system counts
    /// *before* the probability is taken; λ is not touched.
    fn model_cost(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
        user: UserCountDelta,
    ) -> Result<Cost, LmError> {
        if !self.has_unigrams() {
            return self.pure_bigram_cost(history, token, user);
        }

        let unigram = merge_counts(
            self.unigram_count(token.value()).unwrap_or(0),
            user.unigram_delta,
        );
        let unigram_total = merge_counts(self.unigram_total, user.unigram_total_delta);
        if unigram == 0 || unigram_total == 0 {
            return Ok(UNKNOWN_COST);
        }

        let unigram_cost = surprisal(unigram, unigram_total);

        let Some(prev) = history.last() else {
            return Ok(unigram_cost / UNIGRAM_TIEBREAK_SCALE);
        };

        match merge_bigram(
            self.transition(prev.value(), token.value())?,
            user.bigram_count,
            user.bigram_total,
        ) {
            Some((bigram_count, bigram_total)) => {
                // λ·b/bt + (1 − λ)·u/ut over a common denominator, with λ =
                // lambda_num / lambda_den from the model config. Checked
                // arithmetic: a pathological λ denominator floors at
                // UNKNOWN_COST rather than overflowing (constitution §4).
                // The inputs are the merged counts; the formula is unchanged.
                match interpolate_ratio(
                    self.lambda.numerator(),
                    self.lambda.denominator(),
                    u128::from(bigram_count),
                    u128::from(bigram_total),
                    u128::from(unigram),
                    u128::from(unigram_total),
                ) {
                    Some((numerator, denominator)) => {
                        let (numerator, denominator) = reduce_ratio(numerator, denominator);
                        Ok(surprisal(numerator, denominator))
                    }
                    None => Ok(UNKNOWN_COST),
                }
            }
            // Previous token absent from both grams (no evidence of this
            // transition at all), or a degenerate zero-total entry: floor at
            // UNKNOWN_COST — the same floor a count-0 next-token gets — so an
            // unseen transition never scores cheaper than a rare observed one.
            None => Ok(UNKNOWN_COST),
        }
    }

    /// Pre-interpolation behaviour: pure bigram when history exists, else
    /// pass-through (dictionary order carries unigram ranking).
    fn pure_bigram_cost(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
        user: UserCountDelta,
    ) -> Result<Cost, LmError> {
        let Some(prev) = history.last() else {
            return Ok(0);
        };
        match merge_bigram(
            self.transition(prev.value(), token.value())?,
            user.bigram_count,
            user.bigram_total,
        ) {
            Some((count, total)) => Ok(surprisal(count, total)),
            None => Ok(UNKNOWN_COST),
        }
    }

    /// [`LanguageModel::score`] with an explicit §5 user-count overlay.
    ///
    /// `UserCountDelta::ZERO` is bit-identical to [`LanguageModel::score`].
    ///
    /// # Errors
    ///
    /// Returns [`LmError`] when the system table cannot be read.
    pub fn score_with_user_delta(
        &self,
        history: &[PhraseToken],
        token: &PhraseToken,
        edge_cost: Cost,
        user: UserCountDelta,
    ) -> Result<Cost, LmError> {
        Ok(edge_cost.saturating_add(self.model_cost(history, token, user)?))
    }

    /// Real unigram count of `token` plus a user delta, when a real table
    /// is installed.
    ///
    /// `None` when the model has no real frequency table (same switch as
    /// [`LanguageModel::unigram_freq`]). `Some(0)` is a miss that the user
    /// overlay did not raise.
    #[must_use]
    pub fn unigram_freq_with_user_delta(&self, token: u32, user_delta: u64) -> Option<u64> {
        self.real_unigrams
            .then(|| merge_counts(self.unigram_count(token).unwrap_or(0), user_delta))
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
        // Always the merge path: a zero overlay is identity, so existing
        // callers stay bit-identical and the empty-store pin stays honest.
        self.score_with_user_delta(history, token, edge_cost, UserCountDelta::ZERO)
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        // Only the interpolation2 table is a real frequency table; the
        // export-ABI map is a scoring input, not candidate-ranking data.
        Ok(self
            .real_unigrams
            .then(|| self.unigram_count(token.value()).unwrap_or(0)))
    }

    fn has_real_unigrams(&self) -> bool {
        self.real_unigrams
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        Ok(self.real_unigrams.then_some(self.unigram_total))
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

    #[test]
    fn default_lambda_is_the_pinned_config_value() {
        // Not the old authored 1/2: the LM defaults to the pinned 0.312699.
        assert_eq!(model().lambda(), Lambda::PINNED);
    }

    #[test]
    fn set_lambda_from_table_conf_reads_the_config() {
        let dir = std::env::temp_dir().join(format!("pinyin-lm-tableconf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("table.conf");
        std::fs::write(
            &path,
            "binary format version:7\nlambda parameter:0.312699\n",
        )
        .unwrap();

        let mut model = model();
        assert!(
            model.set_lambda_from_table_conf(&path),
            "a present config is read"
        );
        assert_eq!(model.lambda(), Lambda::PINNED);

        // Absent config: returns false and leaves λ unchanged (the default
        // stands for the fetched cache, which ships no table.conf).
        assert!(!model.set_lambda_from_table_conf(&dir.join("does-not-exist.conf")));
        assert_eq!(model.lambda(), Lambda::PINNED);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn interpolate_ratio_floors_instead_of_overflowing() {
        // A pathological λ denominator whose products exceed u128 returns
        // None (the caller floors at UNKNOWN_COST) rather than panicking.
        assert_eq!(
            interpolate_ratio(1, 2, u128::MAX, u128::MAX, 1, u128::MAX),
            None
        );
        // Ordinary values interpolate fine.
        assert_eq!(
            interpolate_ratio(312_699, 1_000_000, 3, 10, 4, 100),
            Some((312_699 * 3 * 100 + 687_301 * 4 * 10, 1_000_000 * 10 * 100))
        );
    }

    #[test]
    fn lambda_is_live_in_the_interpolated_cost() {
        // The bigram term is weighted by λ, so a different λ yields a
        // different interpolated cost for an observed transition — proof the
        // config value reaches the hot path rather than a stale constant.
        let mut model = model();
        let mut unigrams = BTreeMap::new();
        unigrams.insert(DE, 50);
        unigrams.insert(NI, 50);
        model.set_unigrams(unigrams, 100);
        let history = [PhraseToken::new(NI)];

        model.set_lambda(Lambda::PINNED);
        let at_pinned = model.score(&history, &PhraseToken::new(DE), 0).unwrap();

        // λ = 1/2, the old hardcoded value, via a synthetic table.conf.
        assert!(model.set_lambda_from_table_conf(&write_temp_conf("0.5")));
        let at_half = model.score(&history, &PhraseToken::new(DE), 0).unwrap();

        assert_ne!(
            at_pinned, at_half,
            "λ must change the interpolated bigram cost (0.312699 vs 0.5)"
        );
    }

    fn write_temp_conf(value: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "pinyin-lm-conf-{}-{value}.conf",
            std::process::id()
        ));
        std::fs::write(&path, format!("lambda parameter:{value}\n")).unwrap();
        path
    }

    #[test]
    fn merge_counts_is_saturating_addition() {
        assert_eq!(merge_counts(10, 0), 10);
        assert_eq!(merge_counts(0, 10), 10);
        assert_eq!(merge_counts(10, 20), 30);
        assert_eq!(merge_counts(u64::MAX, 1), u64::MAX);
        assert_eq!(merge_counts(1, u64::MAX), u64::MAX);
    }

    #[test]
    fn merge_bigram_empty_user_is_the_system_gram() {
        assert_eq!(merge_bigram(Some((3, 10)), 0, 0), Some((3, 10)));
        assert_eq!(merge_bigram(None, 0, 0), None);
        assert_eq!(merge_bigram(Some((0, 0)), 0, 0), None);
    }

    #[test]
    fn merge_bigram_user_only_is_the_user_gram() {
        // merge_single_gram: system NULL + user present → user unchanged.
        assert_eq!(merge_bigram(None, 69, 69), Some((69, 69)));
        assert_eq!(merge_bigram(None, 0, 69), Some((0, 69)));
    }

    #[test]
    fn merge_bigram_adds_both_sides() {
        assert_eq!(merge_bigram(Some((10, 100)), 69, 69), Some((79, 169)));
        assert_eq!(
            merge_bigram(Some((u32::MAX, u32::MAX)), 1, 1),
            Some((u64::from(u32::MAX) + 1, u64::from(u32::MAX) + 1))
        );
    }

    #[test]
    fn zero_overlay_is_bit_identical_to_system_score() {
        let mut model = model();
        let mut unigrams = BTreeMap::new();
        unigrams.insert(DE, 50);
        unigrams.insert(NI, 50);
        model.set_unigrams(unigrams, 100);
        let history = [PhraseToken::new(NI)];
        let token = PhraseToken::new(DE);

        let system = model.score(&history, &token, 0).unwrap();
        let merged = model
            .score_with_user_delta(&history, &token, 0, UserCountDelta::ZERO)
            .unwrap();
        assert_eq!(system, merged, "a zero overlay must be identity");

        let empty_history_system = model.score(&[], &token, 7).unwrap();
        let empty_history_merged = model
            .score_with_user_delta(&[], &token, 7, UserCountDelta::ZERO)
            .unwrap();
        assert_eq!(empty_history_system, empty_history_merged);
    }

    #[test]
    fn populated_overlay_lowers_the_interpolated_cost() {
        // Populated-store pin (separate from the empty-store contract).
        // Training sequence this models: one first-seen selection of
        // DE after NI — seed 69 on the bigram and 69*7 = 483 on the
        // unigram (`docs/findings/user-store.md` §2.1).
        let mut model = model();
        let mut unigrams = BTreeMap::new();
        unigrams.insert(DE, 50);
        unigrams.insert(NI, 50);
        model.set_unigrams(unigrams, 100);
        let history = [PhraseToken::new(NI)];
        let token = PhraseToken::new(DE);

        let empty = model.score(&history, &token, 0).unwrap();
        let trained = UserCountDelta {
            bigram_count: 69,
            bigram_total: 69,
            unigram_delta: 483,
            unigram_total_delta: 483,
        };
        let populated = model
            .score_with_user_delta(&history, &token, 0, trained)
            .unwrap();
        assert!(
            populated < empty,
            "raising merged counts must cheapen the cost: empty={empty} populated={populated}"
        );

        // Empty-history ranking uses the merged unigram only.
        let empty_first = model.score(&[], &token, 0).unwrap();
        let populated_first = model
            .score_with_user_delta(&[], &token, 0, trained)
            .unwrap();
        assert!(
            populated_first < empty_first,
            "a user unigram delta must cheapen empty-history cost: \
             empty={empty_first} populated={populated_first}"
        );
    }

    #[test]
    fn a_user_only_transition_does_not_floor() {
        // System has no row for this prev; the user gram is the merged gram.
        let mut model = model();
        let mut unigrams = BTreeMap::new();
        unigrams.insert(DE, 100);
        model.set_unigrams(unigrams, 110);

        const NO_ENTRY_PREV: u32 = 0xFFFF_FFFF;
        assert!(
            matches!(model.transition(NO_ENTRY_PREV, DE), Ok(None)),
            "precondition: the previous token must be absent from the system bigram"
        );

        let empty = model
            .score(&[PhraseToken::new(NO_ENTRY_PREV)], &PhraseToken::new(DE), 0)
            .unwrap();
        assert_eq!(empty, UNKNOWN_COST);

        let user_only = UserCountDelta {
            bigram_count: 69,
            bigram_total: 69,
            unigram_delta: 0,
            unigram_total_delta: 0,
        };
        let populated = model
            .score_with_user_delta(
                &[PhraseToken::new(NO_ENTRY_PREV)],
                &PhraseToken::new(DE),
                0,
                user_only,
            )
            .unwrap();
        assert!(
            populated < UNKNOWN_COST,
            "a user-only gram must interpolate, not floor: {populated}"
        );
    }
}
