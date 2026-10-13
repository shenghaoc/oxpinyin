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

use std::borrow::Cow;
use std::fmt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::cost::{UNKNOWN_COST, cost_of_probability, reduce_ratio, surprisal};
use oxpinyin_core::{Cost, LanguageModel, PhraseToken, UserCountDelta};

use crate::bigram_table::BigramTable;
use crate::dict::DictError;
use crate::phrase_libraries::PhraseLibraries;
use crate::table::TableError;
use crate::table_conf::Lambda;

/// Error conditions for bigram lookups.
#[derive(Debug)]
pub enum LmError {
    /// A table-level error (I/O, backend, etc.).
    Table(TableError),
    /// Value bytes did not parse under the frozen bigram schema.
    Parse(String),
    /// The user-count overlay failed (a read on the user store).
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

/// Stored `SingleGram` fields, unlike scoring costs, are `guint32`s.
/// Both sums in 074a2219 `storage/ngram.cpp:305,332` wrap before the
/// merged row is read. Keep this separate from wider unigram overlays.
const fn merge_bigram_counts(system: u64, user: u64) -> u64 {
    (system as u32).wrapping_add(user as u32) as u64
}

/// Merged `(count, total)` for one bigram, or `None` when neither side has
/// a gram (`merge_single_gram` returns false when both loads miss).
///
/// A system miss with a non-zero user count or total is the user gram alone. A
/// system hit with a zero user overlay is the system gram alone. Both
/// sides present: wrapping `guint32` addition of each stored field.
#[must_use]
pub fn merge_bigram(
    system: Option<(u32, u32)>,
    user_count: u64,
    user_total: u64,
) -> Option<(u64, u64)> {
    match system {
        Some((count, total)) => {
            let merged_count = merge_bigram_counts(u64::from(count), user_count);
            let merged_total = merge_bigram_counts(u64::from(total), user_total);
            Some((merged_count, merged_total))
        }
        None if user_count > 0 || user_total > 0 => {
            Some((u64::from(user_count as u32), u64::from(user_total as u32)))
        }
        None => None,
    }
}

/// Merged `(records, total)` for one whole `prev → *` successor row with
/// the §5 overlay applied across the row — the row-shaped form of
/// [`merge_bigram`]. Upstream's Gate 2 (`pinyin.cpp:2209-2213`) loads the
/// system gram and the user gram once each and merges them as ONE row the
/// caller then indexes per candidate, so the row totals merge once rather
/// than once per pair.
///
/// `None` is the both-loads-miss of [`merge_bigram`]: no system row and a
/// user side contributing nothing (no rows, zero total). A present
/// system row with a zero total still merges —
/// the pin's `merge_single_gram` builds the merged gram whenever either
/// load answers, and a row-level miss would drop the row's records.
///
/// Records are the union of both sides' tokens, each count a
/// wrapping `guint32` add, ascending by token (`ngram.cpp:277-358`).
#[must_use]
pub fn merge_bigram_row(
    system: Option<&BigramRow>,
    user_rows: &[(u32, u64)],
    user_total: u64,
) -> Option<(Vec<(u32, u64)>, u64)> {
    if system.is_none() && user_rows.is_empty() && user_total == 0 {
        return None;
    }
    // The pin merges two sorted arrays linearly (074a2219 ngram.cpp:308-348).
    // Installed rows are sorted already. Preserve the existing helper's
    // acceptance of arbitrary stored order and duplicate tokens as well.
    fn ordered<T: Copy>(rows: &[(u32, T)]) -> Cow<'_, [(u32, T)]> {
        if rows.windows(2).all(|pair| pair[0].0 <= pair[1].0) {
            Cow::Borrowed(rows)
        } else {
            let mut rows = rows.to_vec();
            rows.sort_by_key(|(token, _)| *token);
            Cow::Owned(rows)
        }
    }
    let system_rows = ordered(system.map_or(&[][..], |row| row.records.as_slice()));
    let user_rows = ordered(user_rows);
    let mut counts: Vec<(u32, u64)> =
        Vec::with_capacity(system_rows.len().saturating_add(user_rows.len()));
    let total = merge_bigram_counts(system.map_or(0, |row| u64::from(row.total)), user_total);
    let mut system = system_rows
        .iter()
        .map(|&(token, count)| (token, u64::from(count)))
        .peekable();
    let mut user = user_rows.iter().copied().peekable();
    loop {
        let next = match (system.peek(), user.peek()) {
            (Some(&(left, _)), Some(&(right, _))) if left <= right => system.next(),
            (Some(_), Some(_)) | (None, Some(_)) => user.next(),
            (Some(_), None) => system.next(),
            (None, None) => break,
        };
        if let Some((token, count)) = next {
            if let Some((last, slot)) = counts.last_mut()
                && *last == token
            {
                *slot = merge_bigram_counts(*slot, count);
            } else {
                counts.push((token, merge_bigram_counts(0, count)));
            }
        }
    }
    Some((counts, total))
}

/// One previous-token row of the system bigram — re-exported from
/// [`crate::row_format::bigram`], where the row's on-disk layout lives so
/// the reader and `oxpinyin-datagen`'s writer share one definition.
pub use crate::row_format::bigram::BigramRow;

/// Whether library `nibble` is visible under `mask` (bit `n` set =
/// library `n` unloaded).
///
/// `mask == 0` and nibbles outside the u32 bit
/// range are trivially visible — the same rule the runtime's dictionary
/// applies.
#[must_use]
pub const fn library_visible(mask: u32, nibble: u8) -> bool {
    mask == 0 || nibble >= 32 || mask & (1_u32 << nibble) == 0
}

/// Whether `nibble` is one of the default facade's three `USER_FILE`
/// sub-indexes (`novel_types.h:159-161`): the promoted addon library (5),
/// the network library (6) and the user library (7), whose items live in
/// the user directory (`addon.bin` / `network.bin` / `user.bin`,
/// [`crate::user_files::USER_LIBRARY_FILES`]) rather than a system chunk
/// — so no [`PhraseLibraries`] item backs their tokens, and the counts
/// that price them live in the §5 user overlay.
const fn is_user_file_library(nibble: u8) -> bool {
    matches!(nibble, 5..=7)
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
    /// λ as the pin's `gfloat` when `table.conf` spells a value the exact
    /// decimal form cannot hold (outside `[0, 1]`, an exponent, `inf`,
    /// `nan`): the interpolation then runs the pin's `gfloat`/`gdouble`
    /// arithmetic (`phonetic_lookup.h:663`, `:693`) up to the `log`.
    lambda_raw: Option<f32>,
    /// λ as the pin's `gfloat`, whichever way it was installed: the weight of
    /// the candidate frequency law.
    lambda_f32: f32,
}

/// `unigram_lambda(1. - lambda)` (`phonetic_lookup.h:712-713`): the double
/// subtraction, stored as a `gfloat`.
fn unigram_lambda(lambda: f32) -> f32 {
    (1.0_f64 - f64::from(lambda)) as f32
}

/// The cost of a probability `x` computed the pin's way: `-log₂ x` on the
/// fixed-point scale. Where the pin's `log` is not a finite negative number
/// the cost is the matching special value — NaN for a negative or NaN
/// argument, `+inf` for 0, a negative cost above one, `-inf` for `inf` — and
/// the pin keeps the step. `None` only where the probability underflows the
/// scale, as before.
fn probability_cost(x: f64) -> Option<Cost> {
    const SCALE: u64 = 1 << 62;
    if x.is_nan() || x <= 0.0 || x > 1.0 {
        return Some(cost_of_probability(x));
    }
    let count = (x * SCALE as f64) as u64;
    match surprisal(count, SCALE) {
        UNKNOWN_COST => None,
        cost => Some(cost),
    }
}

/// `log(elem_poss * unigram_lambda)` without the pronunciation factor
/// (`phonetic_lookup.h:663`).
fn raw_unigram_cost(lambda: f32, unigram: u64, unigram_total: u64) -> Option<Cost> {
    if unigram == 0 || unigram_total == 0 {
        return None;
    }
    let elem_poss = unigram as f64 / unigram_total as f64;
    probability_cost(elem_poss * f64::from(unigram_lambda(lambda)))
}

/// `log(bigram_lambda * bigram_poss + unigram_lambda * unigram_poss)`
/// without the pronunciation factor (`phonetic_lookup.h:608`, `:693`): the
/// bigram term in `gfloat`, the unigram term in `gdouble`.
fn raw_blend_cost(
    lambda: f32,
    bigram_count: u64,
    bigram_total: u64,
    unigram: u64,
    unigram_total: u64,
) -> Option<Cost> {
    if unigram_total == 0 {
        return None;
    }
    // A wrapped SingleGram total remains present. The pin divides in
    // gfloat even at zero (`phonetic_lookup.h:608`), yielding infinity
    // for a positive count; probability_cost preserves its special cost.
    let bigram_poss = bigram_count as f32 / bigram_total as f32;
    let unigram_poss = unigram as f64 / unigram_total as f64;
    probability_cost(
        f64::from(lambda * bigram_poss) + f64::from(unigram_lambda(lambda)) * unigram_poss,
    )
}

/// The integer-ratio path's equivalent of the pin's positive count / 0.
/// `lambda * inf` is NaN at zero lambda, infinity otherwise. Preserve
/// those existing special costs without a transcendental or wider sum.
fn zero_total_bigram_cost(lambda_num: u128) -> Cost {
    if lambda_num == 0 {
        oxpinyin_core::cost::NAN_COST
    } else {
        oxpinyin_core::cost::NEG_INF_COST
    }
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
            lambda_raw: None,
            lambda_f32: crate::table_conf::PINNED_LAMBDA,
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
        self.lambda_raw = None;
        self.lambda_f32 = lambda.as_f64() as f32;
    }

    /// Installs λ from the parsed system `table.conf`.
    pub fn set_lambda_from_conf(&mut self, conf: &crate::table_info::TableConf) {
        self.lambda_f32 = conf.lambda_f32();
        match conf.lambda_unit() {
            Some(lambda) => {
                self.lambda = lambda;
                self.lambda_raw = None;
            }
            None => self.lambda_raw = Some(conf.lambda_f32()),
        }
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
                self.lambda_raw = None;
                self.lambda_f32 = lambda.as_f64() as f32;
                true
            }
            None => false,
        }
    }

    /// The facade's phrase libraries this model reads unigrams from.
    #[must_use]
    pub const fn libraries(&self) -> &Arc<PhraseLibraries> {
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

    /// Returns `(count, total, successor_present)` for `prev → next`, or
    /// `None` when `prev` has no bigram entry. A stored zero-count record is
    /// still a `get_freq` hit (`074a2219 ngram.cpp:231-252`).
    fn transition(&self, prev: u32, next: u32) -> Result<Option<(u32, u32, bool)>, LmError> {
        let Some(row) = self.load_successors(prev)? else {
            return Ok(None);
        };
        let count = row
            .records
            .iter()
            .find(|(next_token, _)| *next_token == next)
            .map(|(_, count)| *count);
        Ok(Some((count.unwrap_or(0), row.total, count.is_some())))
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
    ) -> Result<Option<(u64, u64, bool)>, LmError> {
        let system = self.transition(prev, next)?;
        let present = system.is_some_and(|(_, _, present)| present) || user.bigram_count > 0;
        Ok(merge_bigram(
            system.map(|(count, total, _)| (count, total)),
            user.bigram_count,
            user.bigram_total,
        )
        .map(|(count, total)| (count, total, present)))
    }

    /// Loads and merges one n-best predecessor row. Reuses the whole-row
    /// arithmetic, retaining the pair path's first system record and its
    /// positive-user-count presence gate. A stored system zero stays present.
    /// At 074a2219, phonetic_lookup.h:588-599 loads/merges before the
    /// successor walk; ngram.cpp:231-252 binary-searches the resulting row.
    ///
    /// # Errors
    /// Propagates table and parse errors, including a present empty value.
    pub fn load_nbest_row(
        &self,
        prev: u32,
        user_rows: &[(u32, u64)],
        user_total: u64,
    ) -> Result<Option<oxpinyin_core::MergedGram>, LmError> {
        let mut system = self.load_successors(prev)?;
        if let Some(row) = system.as_mut() {
            // Parsing accepts stored order and duplicates. The old point
            // lookup used the first record; stable sorting keeps that law.
            row.records.sort_by_key(|(token, _)| *token);
            row.records.dedup_by_key(|(token, _)| *token);
        }
        let user_rows: Vec<_> = user_rows
            .iter()
            .copied()
            .filter(|(_, count)| *count > 0)
            .collect();
        Ok(merge_bigram_row(system.as_ref(), &user_rows, user_total)
            .map(|(records, total)| oxpinyin_core::MergedGram::new(total, records)))
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
    /// as before. Bigram fields wrap at their stored `guint32` width
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
            Some((bigram_count, bigram_total, _)) if self.lambda_raw.is_some() => {
                let lambda = self.lambda_raw.unwrap_or_default();
                Ok(
                    raw_blend_cost(lambda, bigram_count, bigram_total, unigram, unigram_total)
                        .unwrap_or(UNKNOWN_COST),
                )
            }
            Some((0, 0, true)) => Ok(oxpinyin_core::cost::NAN_COST),
            Some((bigram_count, 0, _)) if bigram_count > 0 => {
                Ok(zero_total_bigram_cost(self.lambda.numerator()))
            }
            Some((bigram_count, bigram_total, _)) => {
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
            // transition at all): floor at
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
            Some((count, total, _)) => Ok(surprisal(count, total)),
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
        let model = self.model_cost(history, token, user)?;
        if model == oxpinyin_core::cost::NAN_COST {
            return Ok(model);
        }
        Ok(edge_cost.saturating_add(model))
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
    /// The presence gate itself is the pin's `get_phrase_item` over the
    /// token's *loaded* sub-index (`unigram_gen_next_step`,
    /// `phonetic_lookup.h:643-668`): a token of one of the default
    /// facade's three `USER_FILE` sub-indexes — imports, learnings and
    /// choose-promoted addon phrases, whose items live in the user
    /// directory, not a system chunk — is priced from its user delta
    /// alone, the loaded user sub-index's own frequency field, while a
    /// masked (unloaded) library's token and a loaded library's missing
    /// item answer the default, exactly as the pin's failing
    /// `get_phrase_item` does. `UserCountDelta::ZERO` is bit-identical to
    /// the trait method.
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
        self.nbest_step_costs_with_imported_item(prev, token, user, false)
    }

    /// The n-best overlay with a caller-verified appended system item.
    /// Its base count is zero; its full frequency is in the user delta.
    /// The visibility gate still applies (074a2219 phonetic_lookup.h:643-668).
    ///
    /// # Errors
    /// Returns an error when the system bigram table cannot be read.
    pub fn nbest_step_costs_with_imported_item(
        &self,
        prev: &PhraseToken,
        token: &PhraseToken,
        user: UserCountDelta,
        imported_item: bool,
    ) -> Result<oxpinyin_core::NbestStepCosts, LmError> {
        self.nbest_step_costs_using(token, user, imported_item, || {
            self.merged_transition(prev.value(), token.value(), user)
        })
    }

    /// The unchanged n-best cost law over a lazily supplied merged row.
    /// Unigram and item visibility remain live; only the predecessor row
    /// belongs to the caller's current step.
    ///
    /// # Errors
    /// Propagates the row loader's error after the existing item gates.
    pub fn nbest_step_costs_from_row(
        &self,
        token: &PhraseToken,
        user: UserCountDelta,
        imported_item: bool,
        row: &mut oxpinyin_core::NbestStepRow,
        load: impl FnOnce() -> Result<Option<oxpinyin_core::MergedGram>, LmError>,
    ) -> Result<oxpinyin_core::NbestStepCosts, LmError> {
        self.nbest_step_costs_from_row_in_generation(token, user, imported_item, row, 0, load)
    }

    /// The same cost law with row reuse bound to one user-store generation.
    /// The caller validates that generation after the score is assembled.
    ///
    /// # Errors
    /// Propagates the row loader's error after the existing item gates.
    pub fn nbest_step_costs_from_row_in_generation(
        &self,
        token: &PhraseToken,
        user: UserCountDelta,
        imported_item: bool,
        row: &mut oxpinyin_core::NbestStepRow,
        generation: u64,
        load: impl FnOnce() -> Result<Option<oxpinyin_core::MergedGram>, LmError>,
    ) -> Result<oxpinyin_core::NbestStepCosts, LmError> {
        self.nbest_step_costs_using(token, user, imported_item, || {
            Ok(row
                .get_or_try_init_in_generation(generation, load)?
                .map(|gram| {
                    let count = gram.count_if_present(token.value());
                    (count.unwrap_or(0), gram.total(), count.is_some())
                }))
        })
    }

    fn nbest_step_costs_using(
        &self,
        token: &PhraseToken,
        user: UserCountDelta,
        imported_item: bool,
        transition: impl FnOnce() -> Result<Option<(u64, u64, bool)>, LmError>,
    ) -> Result<oxpinyin_core::NbestStepCosts, LmError> {
        // The pin's presence gate is `get_phrase_item` over the token's
        // loaded sub-index; `unigram_count` reads the system chunk
        // libraries only, so its `None` folds three cases the gate must
        // tell apart: a `USER_FILE` sub-index's token (priced from the
        // user delta — `0 + delta` — the frequency field the import or
        // promotion wrote, `pinyin.cpp:604-605`), a masked (unloaded)
        // library's token (`ERROR_NO_SUB_PHRASE_INDEX` on the freed
        // sub-index), and a loaded system library's missing item
        // (`ERROR_NO_ITEM`). Only the first may pass; the other two keep
        // the seam's default answer — no n-best cost data, no rows — as
        // does no installed unigram table at all.
        let nibble = (token.value() >> 24) as u8;
        let count = match self.unigram_count(token.value()) {
            Some(count) => count,
            None if self.visible(nibble) && (is_user_file_library(nibble) || imported_item) => 0,
            None => return Ok(oxpinyin_core::NbestStepCosts::default()),
        };
        let count = merge_counts(count, user.unigram_delta);
        let unigram_total = merge_counts(self.unigram_total(), user.unigram_total_delta);
        if unigram_total == 0 {
            return Ok(oxpinyin_core::NbestStepCosts::default());
        }

        if let Some(lambda) = self.lambda_raw {
            let blended = match transition()? {
                Some((bigram_count, bigram_total, true)) => {
                    raw_blend_cost(lambda, bigram_count, bigram_total, count, unigram_total)
                }
                _ => None,
            };
            return Ok(oxpinyin_core::NbestStepCosts {
                blended,
                unigram: raw_unigram_cost(lambda, count, unigram_total),
            });
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
        // merged-gram successors (`search_bigram2`). Stored records survive
        // a wrapped count of zero; absence, rather than count, gates the walk.
        let blended = match transition()? {
            Some((0, 0, true)) => Some(oxpinyin_core::cost::NAN_COST),
            Some((_, 0, true)) => Some(zero_total_bigram_cost(lambda_num)),
            Some((bigram_count, bigram_total, true)) => interpolate_ratio(
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

    fn amplification_lambda(&self) -> f32 {
        self.lambda_f32
    }

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

    fn nbest_step_costs_in_row(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
        row: &mut oxpinyin_core::NbestStepRow,
    ) -> Result<oxpinyin_core::NbestStepCosts, Self::Error> {
        self.nbest_step_costs_from_row(token, UserCountDelta::ZERO, false, row, || {
            self.load_nbest_row(prev.value(), &[], 0)
        })
    }

    fn nbest_successors_in_row<'a>(
        &self,
        prev: &Self::Token,
        row: &'a mut oxpinyin_core::NbestStepRow,
    ) -> Result<oxpinyin_core::NbestSuccessors<'a>, Self::Error> {
        Ok(Some(
            row.get_or_try_init(|| self.load_nbest_row(prev.value(), &[], 0))?
                .map_or(&[], oxpinyin_core::MergedGram::records),
        ))
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

#[cfg(test)]
mod merge_tests {
    use super::{BigramRow, merge_bigram, merge_bigram_row};

    #[test]
    fn linear_row_merge_retains_the_wrapped_union_for_arbitrary_rows() {
        use std::collections::BTreeMap;
        for seed in 0..128_u32 {
            let system = row(
                seed.wrapping_mul(0x8000_0001),
                &[(seed % 7, u32::MAX), (3, seed), (seed % 5, 0), (3, 1)],
            );
            let user = [
                (seed % 6, u64::MAX),
                (3, u64::from(seed)),
                (seed % 7, 0),
                (3, 1),
            ];
            let total = u64::from(seed).wrapping_mul(0x1_8000_0001);
            for system in [None, Some(&system)] {
                let mut expected: BTreeMap<u32, u32> = BTreeMap::new();
                if let Some(row) = system {
                    for &(token, count) in &row.records {
                        let slot = expected.entry(token).or_default();
                        *slot = slot.wrapping_add(count);
                    }
                }
                for &(token, count) in &user {
                    let slot = expected.entry(token).or_default();
                    *slot = slot.wrapping_add(count as u32);
                }
                let total = system.map_or(0, |row| row.total).wrapping_add(total as u32);
                assert_eq!(
                    merge_bigram_row(system, &user, u64::from(seed).wrapping_mul(0x1_8000_0001)),
                    Some((
                        expected
                            .into_iter()
                            .map(|(t, c)| (t, u64::from(c)))
                            .collect(),
                        u64::from(total)
                    ))
                );
            }
        }
    }

    fn row(total: u32, records: &[(u32, u32)]) -> BigramRow {
        BigramRow {
            total,
            records: records.to_vec(),
        }
    }

    #[test]
    fn row_merge_is_none_only_when_both_sides_miss() {
        // No system row, no user rows, zero user total: the
        // both-loads-miss of `merge_single_gram`.
        assert!(merge_bigram_row(None, &[], 0).is_none());
        // A user side that contributes anything keeps the row alive …
        let (records, total) = merge_bigram_row(None, &[], 7).expect("user total alone merges");
        assert!(records.is_empty());
        assert_eq!(total, 7);
        // … and so does any user row. The merged total is each side's
        // *stored* row total added — not a sum recomputed from the rows —
        // so a store that reports rows but a zero total contributes zero,
        // the same additive law the runtime's guess path always used.
        let (records, total) = merge_bigram_row(None, &[(9, 1)], 0).expect("user rows alone merge");
        assert_eq!(records, [(9, 1)]);
        assert_eq!(total, 0);
    }

    #[test]
    fn row_merge_unions_tokens() {
        let system = row(10, &[(1, 3), (2, 4)]);
        let (records, total) =
            merge_bigram_row(Some(&system), &[(2, 5), (3, 6)], 11).expect("both sides merge");
        assert_eq!(records, [(1, 3), (2, 9), (3, 6)]);
        assert_eq!(total, 21);
    }

    #[test]
    fn row_merge_keeps_a_zero_total_system_row() {
        // Both forms retain a zero-total pair: `merge_single_gram` answers true as
        // soon as one load answers, and dropping the row would lose its
        // records.
        let system = row(0, &[(5, 2)]);
        let (records, total) =
            merge_bigram_row(Some(&system), &[], 0).expect("system row alone merges");
        assert_eq!(records, [(5, 2)]);
        assert_eq!(total, 0);
        assert_eq!(merge_bigram(Some((2, 0)), 0, 0), Some((2, 0)));
    }

    #[test]
    fn bigram_merge_wraps_each_field_at_the_stored_width() {
        let system = row(1, &[(1, 1)]);
        for (user, expected) in [(u32::MAX - 1, u32::MAX), (u32::MAX, 0)] {
            let (records, total) =
                merge_bigram_row(Some(&system), &[(1, u64::from(user))], u64::from(user))
                    .expect("a wrapped total still has a row");
            assert_eq!(records, [(1, u64::from(expected))]);
            assert_eq!(total, u64::from(expected));
            assert_eq!(
                merge_bigram(Some((1, 1)), u64::from(user), u64::from(user)),
                Some((u64::from(expected), u64::from(expected)))
            );
        }
        assert_eq!(
            merge_bigram(Some((2, 2)), u64::from(u32::MAX), u64::from(u32::MAX)),
            Some((1, 1))
        );
    }

    #[test]
    fn wrapped_zero_total_keeps_the_pins_infinite_probability() {
        assert_eq!(
            super::raw_blend_cost(0.312699, 2, 0, 1, 10),
            Some(oxpinyin_core::cost::NEG_INF_COST)
        );
        assert_eq!(
            super::zero_total_bigram_cost(1),
            oxpinyin_core::cost::NEG_INF_COST
        );
        assert_eq!(
            super::zero_total_bigram_cost(0),
            oxpinyin_core::cost::NAN_COST
        );
    }
}
