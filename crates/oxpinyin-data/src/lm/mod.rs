//! Bigram language model over libpinyin's own `bigram.db` and the phrase
//! libraries' unigram counts.
//!
//! Implements [`oxpinyin_core::LanguageModel`] with two lazy sources:
//!
//! * the bigram rows — `Bigram::load` per previous token through
//!   [`BigramTable`] (a point read on the hash container: key
//!   `phrase_token_t` LE, value `total: u32` + `{next: u32, count: u32}`
//!   records, `SingleGram`'s layout);
//! * the unigram counts — `PhraseItem::get_unigram_frequency` on the
//!   shared [`PhraseLibraries`], the mmap'd chunk items, and
//!   `FacadePhraseIndex::get_phrase_index_total_freq` for the total.
//!
//! Nothing is loaded at open beyond the DBM handle. The unigram the seam
//! hands out is upstream's item field itself —
//! `PhraseItem::get_unigram_frequency`, the `\1-gram` count plus the one
//! `gen_unigram` adds to every item "to avoid zero value when computing
//! unigram frequency in float format" (`utils/training/gen_unigram.cpp`)
//! — and the total is `FacadePhraseIndex::get_phrase_index_total_freq`,
//! `Σ item`. That is the arithmetic every upstream reader of the field
//! performs (`PinyinLookup2`'s unigram term, `_compute_frequency_of_items`,
//! `train_result3`): an item the corpus never saw has frequency 1, not 0,
//! and stays reachable in the trellis. Verified on the pinned model:
//! `Σ item == Σ \1-gram count + 138,096`.
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
//! `data-formats.md` §3) when no `table.conf` is available.
//!
//! An unloaded library (`pinyin_unload_phrase_library`) leaves the model
//! through the shared library mask: its items answer no unigram and its
//! total leaves the denominator, as freeing the sub-index does upstream.

use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::cost::{UNKNOWN_COST, reduce_ratio, surprisal};
use oxpinyin_core::{Cost, LanguageModel, PhraseToken, UserCountDelta};

use crate::bigram_table::BigramTable;
use crate::dict::DictError;
use crate::phrase_libraries::PhraseLibraries;
use crate::table::TableError;
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

impl From<DictError> for LmError {
    fn from(e: DictError) -> Self {
        match e {
            DictError::Table(table) => Self::Table(table),
            DictError::Parse(message) => Self::Parse(message),
            DictError::Library(library) => Self::Parse(library.to_string()),
        }
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

/// Whether library `nibble` is visible under `mask` (bit `n` set =
/// library `n` unloaded). `mask == 0` and nibbles outside the u32 bit
/// range are trivially visible — the same rule the runtime's dictionary
/// applies.
#[must_use]
pub fn library_visible(mask: u32, nibble: u8) -> bool {
    mask == 0 || nibble >= 32 || mask & (1_u32 << nibble) == 0
}

/// Bigram language model over `bigram.db` and the phrase libraries.
pub struct BigramLanguageModel {
    bigram: BigramTable,
    /// The facade's chunk items — the unigram source.
    libraries: Arc<PhraseLibraries>,
    /// The loaded-library mask shared with the dictionary: bit `n` set =
    /// library `n` unloaded.
    library_mask: Arc<AtomicU32>,
    /// Bigram/unigram interpolation weight λ, read from the model's
    /// `table.conf` when available and [`Lambda::PINNED`] (`0.312699`,
    /// `data-formats.md` §3) otherwise. Read from config rather than
    /// hardcoded.
    lambda: Lambda,
}

impl BigramLanguageModel {
    /// Opens the bigram model: a lazy handle on `bigram_path` (the
    /// selected backend's hash container — `bigram.db` on Kyoto Cabinet
    /// and tkrzw) over the facade's `libraries`, with every library
    /// visible.
    ///
    /// λ defaults to [`Lambda::PINNED`] (`0.312699`, `data-formats.md` §3);
    /// override it from a real install's config with
    /// [`Self::set_lambda_from_table_conf`] or [`Self::set_lambda`].
    ///
    /// # Errors
    ///
    /// Returns [`LmError`] when the bigram file cannot be opened.
    pub fn open(bigram_path: &Path, libraries: Arc<PhraseLibraries>) -> Result<Self, LmError> {
        Self::open_with_mask(bigram_path, libraries, Arc::new(AtomicU32::new(0)))
    }

    /// [`Self::open`] sharing `library_mask` with the dictionary that
    /// owns the unload state.
    ///
    /// # Errors
    ///
    /// Returns [`LmError`] when the bigram file cannot be opened.
    pub fn open_with_mask(
        bigram_path: &Path,
        libraries: Arc<PhraseLibraries>,
        library_mask: Arc<AtomicU32>,
    ) -> Result<Self, LmError> {
        let bigram = BigramTable::open(bigram_path)?;
        Ok(Self {
            bigram,
            libraries,
            library_mask,
            lambda: Lambda::PINNED,
        })
    }

    /// The interpolation weight λ currently in effect.
    #[must_use]
    pub const fn lambda(&self) -> Lambda {
        self.lambda
    }

    /// Sets the interpolation weight λ directly.
    pub const fn set_lambda(&mut self, lambda: Lambda) {
        self.lambda = lambda;
    }

    /// Reads λ from a model's `table.conf` and installs it (`data-formats.md`
    /// §3).
    ///
    /// Returns `true` when a `table.conf` with a parsable `lambda parameter:`
    /// line was found and applied; `false` when the file is absent or carries
    /// no such line, in which case λ is left unchanged. Never errors: a
    /// missing config is the normal case.
    pub fn set_lambda_from_table_conf(&mut self, table_conf_path: &Path) -> bool {
        match crate::table_conf::read_table_conf_lambda(table_conf_path) {
            Some(lambda) => {
                self.lambda = lambda;
                true
            }
            None => false,
        }
    }

    /// The facade's phrase libraries this model reads unigrams from.
    #[must_use]
    pub fn libraries(&self) -> &Arc<PhraseLibraries> {
        &self.libraries
    }

    fn visible(&self, nibble: u8) -> bool {
        library_visible(self.library_mask.load(Ordering::SeqCst), nibble)
    }

    /// Whether unigram counts are available for interpolation: some
    /// visible library carries corpus counts.
    #[must_use]
    pub fn has_unigrams(&self) -> bool {
        self.unigram_total() > 0
    }

    /// `PhraseItem::get_unigram_frequency` for `token`: the stored item
    /// field, `gen_unigram`'s `+1` included, so a phrase the n-gram corpus
    /// never saw answers `Some(1)`.
    ///
    /// `None` when the token's library is not loaded or is masked
    /// (upstream's `get_phrase_item` failing), or owns no such item.
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        if !self.visible((token >> 24) as u8) {
            return None;
        }
        self.libraries.unigram_count(token)
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq` over the visible
    /// libraries: the sum of every item's stored unigram.
    #[must_use]
    pub fn unigram_total(&self) -> u64 {
        let mask = self.library_mask.load(Ordering::SeqCst);
        self.libraries
            .unigram_total_where(|nibble| library_visible(mask, nibble))
    }

    /// Loads the system-bigram row for `prev` — one point read.
    ///
    /// `None` means `prev` has no entry: the same miss
    /// `PhraseLookup::search_bigram2` treats as "skip this node" (no merged
    /// single-gram). A next-token that is not in the row is a `get_freq`
    /// miss, not a zero-count hit.
    ///
    /// # Errors
    ///
    /// Returns [`LmError`] when the table cannot be read or a value does not
    /// parse under the stored schema.
    pub fn load_successors(&self, prev: u32) -> Result<Option<BigramRow>, LmError> {
        Ok(self.bigram.load_successors(prev)?)
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
            .map_or(0, |(_, count)| *count);
        Ok(Some((count, row.total)))
    }

    /// System `prev → next` counts with the §5 overlay already merged.
    ///
    /// This is the scoring-path load: callers must not take a raw
    /// [`Self::transition`] and skip [`merge_bigram`].
    fn merged_transition(
        &self,
        prev: u32,
        next: u32,
        user: UserCountDelta,
    ) -> Result<Option<(u64, u64)>, LmError> {
        Ok(merge_bigram(
            self.transition(prev, next)?,
            user.bigram_count,
            user.bigram_total,
        ))
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
        let unigram_total = merge_counts(self.unigram_total(), user.unigram_total_delta);
        if unigram == 0 || unigram_total == 0 {
            return Ok(UNKNOWN_COST);
        }

        let unigram_cost = surprisal(unigram, unigram_total);

        let Some(prev) = history.last() else {
            return Ok(unigram_cost / UNIGRAM_TIEBREAK_SCALE);
        };

        match self.merged_transition(prev.value(), token.value(), user)? {
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
        match self.merged_transition(prev.value(), token.value(), user)? {
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

    /// The item unigram of `token` plus a user delta. `Some(0)` is a token
    /// with no item that the user overlay did not raise; the chunk items
    /// are the real frequency table, so this is never `None`.
    #[must_use]
    pub fn unigram_freq_with_user_delta(&self, token: u32, user_delta: u64) -> Option<u64> {
        Some(merge_counts(
            self.unigram_count(token).unwrap_or(0),
            user_delta,
        ))
    }

    /// [`LanguageModel::nbest_step_costs`] with an explicit §5 user-count
    /// overlay.
    ///
    /// Both branches run over the same additive merge as
    /// [`Self::score_with_user_delta`]. The bigram merge happens *before*
    /// the count > 0 presence gate, so a user-trained successor with no
    /// system count — including a prev that has a system row but not this
    /// next token — takes the blended branch over the merged denominator.
    /// `UserCountDelta::ZERO` is bit-identical to the trait method.
    ///
    /// # Errors
    ///
    /// Returns [`LmError`] when the system table cannot be read.
    pub fn nbest_step_costs_with_user_delta(
        &self,
        prev: &PhraseToken,
        token: &PhraseToken,
        user: UserCountDelta,
    ) -> Result<oxpinyin_core::NbestStepCosts, LmError> {
        // No installed unigram table is the seam's default answer: no n-best
        // cost data, no rows.
        let (Some(count), unigram_total) =
            (self.unigram_count(token.value()), self.unigram_total())
        else {
            return Ok(oxpinyin_core::NbestStepCosts::default());
        };
        let count = merge_counts(count, user.unigram_delta);
        let unigram_total = merge_counts(unigram_total, user.unigram_total_delta);
        if unigram_total == 0 {
            return Ok(oxpinyin_core::NbestStepCosts::default());
        }

        let lambda_num = self.lambda.numerator();
        let lambda_den = self.lambda.denominator();
        let one_minus_lambda = lambda_den.saturating_sub(lambda_num);

        // The no-evidence branch: (1 − λ) · u / ut.
        let unigram = (count > 0)
            .then(|| {
                one_minus_lambda
                    .checked_mul(u128::from(count))
                    .zip(lambda_den.checked_mul(u128::from(unigram_total)))
            })
            .flatten()
            .and_then(ratio_cost);

        // The blended branch, only when the merged bigram row actually
        // carries this successor: upstream's bigram expansion walks
        // merged-gram successors (`search_bigram2`), so a count-0 next token
        // is not a successor and takes the unigram branch instead.
        let blended = match self.merged_transition(prev.value(), token.value(), user)? {
            Some((bigram_count, bigram_total)) if bigram_count > 0 => interpolate_ratio(
                lambda_num,
                lambda_den,
                u128::from(bigram_count),
                u128::from(bigram_total),
                u128::from(count),
                u128::from(unigram_total),
            )
            .and_then(ratio_cost),
            _ => None,
        };

        Ok(oxpinyin_core::NbestStepCosts { blended, unigram })
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
        // The chunk items are the phrase index's real frequency table.
        Ok(Some(self.unigram_count(token.value()).unwrap_or(0)))
    }

    fn has_real_unigrams(&self) -> bool {
        true
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        Ok(Some(self.unigram_total()))
    }

    fn nbest_step_costs(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
    ) -> Result<oxpinyin_core::NbestStepCosts, Self::Error> {
        // System-only: the empty-user identity of the merged path.
        self.nbest_step_costs_with_user_delta(prev, token, UserCountDelta::ZERO)
    }
}

/// Surprisal of one ratio, `None` when the reduced parts do not fit `u64`
/// (a pathological λ; the step then reports no cost, like a below-ε
/// possibility upstream).
fn ratio_cost((numerator, denominator): (u128, u128)) -> Option<Cost> {
    let (numerator, denominator) = reduce_ratio(numerator, denominator);
    match surprisal(numerator, denominator) {
        UNKNOWN_COST => None,
        cost => Some(cost),
    }
}
