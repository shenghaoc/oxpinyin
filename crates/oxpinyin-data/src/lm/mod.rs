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
//! Scoring follows the interpolated form defined in
//! `docs/findings/scoring-spec.md`:
//!
//! ```text
//! P(w_n | w_n-1) = λ · P_bigram(w_n | w_n-1) + (1 − λ) · P_unigram(w_n)
//! ```
//!
//! λ is read from the model's `table.conf` ([`crate::table_conf`]); it
//! defaults to [`crate::table_conf::PINNED_LAMBDA`] (`0.312699`,
//! `data-formats.md` §3) when no `table.conf` is available.
//!
//! An unloaded library (`pinyin_unload_phrase_library`) leaves the model
//! through the shared library mask: its items answer no unigram and its
//! total leaves the denominator, as freeing the sub-index does upstream.

use std::cell::Cell;
use std::fmt;
use std::path::Path;

use oxpinyin_core::{LanguageModel, PhraseToken, UserCountDelta, single_gram};

use crate::bigram_table::BigramTable;
use crate::dict::DictError;
use crate::phrase_libraries::PhraseLibraries;
use crate::table::TableError;

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

impl LmError {
    /// Whether this is the checked-read failure: a record the size check
    /// found and the body read then failed or sized differently. Kyoto
    /// Cabinet's `Bigram::load` asserts there (`ngram_kyotodb.cpp:131-139`
    /// at 074a2219); the engine reports `ReadAfterCheck` and the C adapters
    /// abort (register row 110).
    #[must_use]
    pub fn is_read_after_check(&self) -> bool {
        matches!(
            self,
            Self::Table(TableError::Store(
                oxpinyin_store::StoreError::ReadAfterCheck
            ))
        )
    }
}

/// Additive merge of one count pair (`merge_single_gram`, §5).
///
/// Saturates rather than wrapping: a user count must not overflow into a
/// wrong score (constitution §4).
#[must_use]
pub const fn merge_counts(system: u64, user: u64) -> u64 {
    system.saturating_add(user)
}

/// Phrase-item frequencies are `guint32`, even when their accumulated
/// overlay is wider (074a2219 `phrase_index.cpp:173-175`).
const fn merge_unigram_counts(system: u64, user: u64) -> u64 {
    (system as u32).wrapping_add(user as u32) as u64
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

/// One previous-token row of the system bigram — re-exported from
/// [`crate::row_format::bigram`], where the row's on-disk layout lives so
/// the reader and `import_interpolation`'s writer share one definition.
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
#[derive(Clone, Copy)]
pub struct BigramLanguageModel<L = PhraseLibraries, T = BigramTables, M = Cell<u32>> {
    tables: T,
    /// The facade's chunk items — the unigram source.
    libraries: L,
    /// The loaded-library mask shared with the dictionary: bit `n` set =
    /// library `n` unloaded.
    library_mask: M,
}

/// Bigram DBM and interpolation parameters, without phrase byte ownership.
pub struct BigramTables {
    bigram: BigramTable,
    /// Bigram/unigram interpolation weight λ as the pin's `gfloat`, read
    /// from the model's `table.conf` when available and
    /// [`crate::table_conf::PINNED_LAMBDA`] (`0.312699`, `data-formats.md` §3) otherwise.
    /// Read from config rather than hardcoded.
    lambda_f32: f32,
}

impl<L, T: std::borrow::Borrow<BigramTables>, M> std::ops::Deref for BigramLanguageModel<L, T, M> {
    type Target = BigramTables;
    fn deref(&self) -> &Self::Target {
        self.tables.borrow()
    }
}

impl BigramTables {
    /// Open only the bigram table; readers borrow phrase storage separately.
    pub fn open(bigram_path: &Path) -> Result<Self, LmError> {
        Ok(Self::with_table(BigramTable::open(bigram_path)?))
    }

    /// Runtime system attachment, preserving the pin's failed BDB/KC
    /// hash object instead of refusing initialization.
    ///
    /// # Errors
    /// Returns failures before native attach; tkrzw remains strict.
    pub fn attach(bigram_path: &Path) -> Result<Self, LmError> {
        Ok(Self::with_table(BigramTable::attach(bigram_path)?))
    }

    fn with_table(bigram: BigramTable) -> Self {
        Self {
            bigram,
            lambda_f32: crate::table_conf::PINNED_LAMBDA,
        }
    }

    /// Whether the native system hash attach failed.
    #[must_use]
    pub fn attachment_failed(&self) -> bool {
        self.bigram.attachment_failed()
    }

    /// Runs the failed system object's reset at facade fini.
    pub fn finish_failed_attachment(&self) {
        self.bigram.finish_failed_attachment();
    }

    /// Whether a retained failed system hash has been finalized; `true`
    /// when the attach succeeded.
    #[must_use]
    pub fn failed_attachment_finished(&self) -> bool {
        !self.bigram.attachment_failed() || self.bigram.failed_attachment_finished()
    }

    /// Borrow all loaded phrase libraries for a model operation.
    pub fn read_all<'a>(
        &'a self,
        libraries: &'a PhraseLibraries,
    ) -> BigramLanguageModel<&'a PhraseLibraries, &'a Self> {
        BigramLanguageModel {
            tables: self,
            libraries,
            library_mask: Cell::new(0),
        }
    }

    /// Bind a model read to the owner's current library state.
    pub fn read<'a>(
        &'a self,
        libraries: &'a PhraseLibraries,
        library_mask: &'a Cell<u32>,
    ) -> BigramLanguageModel<&'a PhraseLibraries, &'a Self, &'a Cell<u32>> {
        BigramLanguageModel {
            tables: self,
            libraries,
            library_mask,
        }
    }
}

impl<L: std::borrow::Borrow<PhraseLibraries>, M: std::borrow::Borrow<Cell<u32>>>
    BigramLanguageModel<L, BigramTables, M>
{
    /// Opens the bigram model: a lazy handle on `bigram_path` (the
    /// selected backend's hash container — `bigram.db` on Kyoto Cabinet
    /// and tkrzw) over the facade's `libraries`, with every library
    /// visible.
    ///
    /// λ defaults to [`crate::table_conf::PINNED_LAMBDA`] (`0.312699`, `data-formats.md` §3);
    /// override it from a real install's config with
    /// `set_lambda_from_conf`.
    ///
    /// # Errors
    ///
    /// Returns [`LmError`] when the bigram file cannot be opened.
    pub fn open_with_mask(
        bigram_path: &Path,
        libraries: L,
        library_mask: M,
    ) -> Result<Self, LmError> {
        Ok(Self {
            tables: BigramTables::open(bigram_path)?,
            libraries,
            library_mask,
        })
    }
}

impl<L: std::borrow::Borrow<PhraseLibraries>> BigramLanguageModel<L> {
    /// Open a model with every library visible.
    pub fn open(bigram_path: &Path, libraries: L) -> Result<Self, LmError> {
        Self::open_with_mask(bigram_path, libraries, Cell::new(0))
    }
}

impl BigramTables {
    /// Installs λ from the parsed system `table.conf`.
    pub fn set_lambda_from_conf(&mut self, conf: &crate::table_info::TableConf) {
        self.lambda_f32 = conf.lambda_f32();
    }
}

impl<L, M> std::ops::DerefMut for BigramLanguageModel<L, BigramTables, M> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.tables
    }
}

impl<
    L: std::borrow::Borrow<PhraseLibraries>,
    T: std::borrow::Borrow<BigramTables>,
    M: std::borrow::Borrow<Cell<u32>>,
> BigramLanguageModel<L, T, M>
{
    /// The facade's phrase libraries this model reads unigrams from.
    #[must_use]
    pub fn libraries(&self) -> &PhraseLibraries {
        self.libraries.borrow()
    }

    /// Whether `nibble` names a library the model reads.
    #[must_use]
    pub fn is_visible(&self, nibble: u8) -> bool {
        self.visible(nibble)
    }

    fn visible(&self, nibble: u8) -> bool {
        library_visible(self.library_mask.borrow().get(), nibble)
    }

    /// Whether unigram counts are available for interpolation: some
    /// visible library holds items. Decided by the tables, not by the
    /// facade total, which a `guint32` can wrap to zero while they stand.
    #[must_use]
    pub fn has_unigrams(&self) -> bool {
        self.libraries()
            .has_items_unloaded(self.library_mask.borrow().get())
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
        self.libraries().unigram_count(token)
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq` over the visible
    /// libraries: their stored totals, a wrapping `guint32`
    /// (074a2219 `phrase_index.h:440`, `phrase_index.cpp:237-248`).
    #[must_use]
    pub fn unigram_total(&self) -> u32 {
        self.libraries()
            .unigram_total_unloaded(self.library_mask.borrow().get())
    }

    /// The facade total the n-best step divides by
    /// (074a2219 `phonetic_lookup.h:651`, `:679`; `phrase_lookup.cpp:315`,
    /// `:334`): the visible libraries' total plus `extra`, everything the
    /// user side and `add_unigram_frequency` added to it. The pin keeps one
    /// `guint32` and adds to it with no overflow check, so the sum wraps.
    #[must_use]
    pub fn facade_total(&self, extra: u64) -> u64 {
        u64::from(self.unigram_total().wrapping_add(extra as u32))
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
    /// `None` when `prev` has no bigram entry: `get_freq` over the stored
    /// row as the pin walks it, from `lower_bound`. A stored zero-count
    /// record is still a hit (`074a2219 ngram.cpp:231-250`).
    fn transition(&self, prev: u32, next: u32) -> Result<Option<(u32, u32, bool)>, LmError> {
        let Some(row) = self.bigram.load_raw_successors(prev)? else {
            return Ok(None);
        };
        let count = single_gram::get_freq(&row, next);
        let total = single_gram::get_total_freq(&row).unwrap_or(0);
        Ok(Some((count.unwrap_or(0), total, count.is_some())))
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

    /// Loads `prev`'s system gram, one point read, and merges `user` — the
    /// user gram's raw value, `None` when the user bigram has no key — as
    /// the pin's lookups do before reading the row
    /// (`phonetic_lookup.h:594-599`, `pinyin.cpp:2210-2215`,
    /// `phrase_lookup.cpp:268-276` at `074a2219`): `None` when both are
    /// absent, the present gram's own bytes when one is, and
    /// `merge_single_gram` otherwise ([`oxpinyin_core::MergedGram::merge`]).
    /// The system value enters the merge as stored, from a fresh DBM read.
    ///
    /// # Errors
    /// Propagates DBM read and schema errors, including a present empty value.
    pub fn load_merged_row(
        &self,
        prev: u32,
        user: Option<Vec<u8>>,
    ) -> Result<Option<oxpinyin_core::MergedGram>, LmError> {
        Ok(oxpinyin_core::MergedGram::merge(
            self.bigram.load_raw_successors(prev)?,
            user,
        ))
    }

    /// The unigram the model scores `token` with: the live item field when the
    /// user side holds one for a visible library, else the stored value plus
    /// the user delta, merged as `guint32` (`phrase_index.cpp:173-175`).
    fn live_unigram_count(&self, token: u32, user: &UserCountDelta) -> u64 {
        match user
            .unigram_live
            .filter(|_| self.visible((token >> 24) as u8))
        {
            Some(live) => u64::from(live),
            None => {
                merge_unigram_counts(self.unigram_count(token).unwrap_or(0), user.unigram_delta)
            }
        }
    }

    /// The item unigram of `token` plus a user delta, or the live field when
    /// there is one. `Some(0)` is a token
    /// with no item that the user overlay did not raise; the chunk items
    /// are the real frequency table, so this is never `None`.
    #[must_use]
    pub fn unigram_freq_with_user_delta(
        &self,
        token: u32,
        live: Option<u32>,
        user_delta: u64,
    ) -> Option<u64> {
        Some(self.live_unigram_count(
            token,
            &UserCountDelta {
                unigram_delta: user_delta,
                unigram_live: live,
                ..UserCountDelta::ZERO
            },
        ))
    }

    /// [`LanguageModel::nbest_step_costs`] with an explicit §5 user-count
    /// overlay.
    ///
    /// Both branches run over the same additive count merge. The bigram
    /// merge happens *before* the count > 0 presence gate, so a
    /// user-trained successor with no
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

    /// The unchanged n-best cost law for one item the caller read from the
    /// predecessor's merged row: `item` is its `(freq, total)`, and `None`
    /// leaves only the unigram branch. Unigram and item visibility stay
    /// live; only the row belongs to the caller's current step.
    ///
    /// # Errors
    /// None today; the signature keeps the cost law's error type.
    pub fn nbest_item_costs(
        &self,
        token: &PhraseToken,
        user: UserCountDelta,
        imported_item: bool,
        item: Option<(u32, u32)>,
    ) -> Result<oxpinyin_core::NbestStepCosts, LmError> {
        self.nbest_step_costs_using(token, user, imported_item, || {
            Ok(item.map(|(freq, total)| (u64::from(freq), u64::from(total), true)))
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
        let count = match user.unigram_live.filter(|_| self.visible(nibble)) {
            Some(live) => u64::from(live),
            None => {
                let count = match self.unigram_count(token.value()) {
                    Some(count) => count,
                    None if self.visible(nibble)
                        && (is_user_file_library(nibble) || imported_item) =>
                    {
                        0
                    }
                    None => return Ok(oxpinyin_core::NbestStepCosts::default()),
                };
                merge_unigram_counts(count, user.unigram_delta)
            }
        };
        let unigram_total = self.facade_total(user.unigram_total_delta);
        // Pre-log pieces, with the pin's epsilon gates before weighting.
        // IEEE division deliberately carries zero totals and NaN through.
        let elem = count as f64 / unigram_total as f64;
        let unigram_lambda = (1.0_f64 - f64::from(self.lambda_f32)) as f32;
        let unigram = if elem < f64::EPSILON {
            None
        } else {
            Some(elem)
        };
        let blended = match transition()? {
            Some((freq, total, true)) => {
                let bigram = freq as f32 / total as f32;
                if bigram < f32::EPSILON && elem < f64::EPSILON {
                    None
                } else {
                    let weighted = self.lambda_f32 * bigram;
                    Some(f64::from(weighted) + f64::from(unigram_lambda) * elem)
                }
            }
            _ => None,
        };
        if blended.is_none() && unigram.is_none() {
            return Ok(oxpinyin_core::NbestStepCosts::default());
        }
        Ok(oxpinyin_core::NbestStepCosts {
            blended,
            unigram,
            unigram_lambda,
        })
    }
}

impl<
    L: std::borrow::Borrow<PhraseLibraries>,
    T: std::borrow::Borrow<BigramTables>,
    M: std::borrow::Borrow<Cell<u32>>,
> LanguageModel for BigramLanguageModel<L, T, M>
{
    type Token = PhraseToken;
    type Error = LmError;

    fn amplification_lambda(&self) -> f32 {
        self.lambda_f32
    }

    fn unigram_freq(&self, token: &Self::Token) -> Result<Option<u64>, Self::Error> {
        // The chunk items are the phrase index's real frequency table.
        Ok(Some(self.unigram_count(token.value()).unwrap_or(0)))
    }

    fn unigram_total(&self) -> Result<Option<u64>, Self::Error> {
        Ok(Some(u64::from(self.unigram_total())))
    }

    fn nbest_step_costs(
        &self,
        prev: &Self::Token,
        token: &Self::Token,
    ) -> Result<oxpinyin_core::NbestStepCosts, Self::Error> {
        // System-only: the empty-user identity of the merged path.
        self.nbest_step_costs_with_user_delta(prev, token, UserCountDelta::ZERO)
    }

    fn nbest_item_costs_in_row(
        &self,
        _prev: &Self::Token,
        token: &Self::Token,
        item: Option<(u32, u32)>,
        _row: &mut oxpinyin_core::NbestStepRow,
    ) -> Result<oxpinyin_core::NbestStepCosts, Self::Error> {
        self.nbest_item_costs(token, UserCountDelta::ZERO, false, item)
    }

    fn begin_successor_span(&self, row: &mut oxpinyin_core::NbestStepRow) {
        if self.attachment_failed() {
            *row = oxpinyin_core::NbestStepRow::default();
        }
    }

    fn nbest_successors_in_row<'a>(
        &self,
        prev: &Self::Token,
        row: &'a mut oxpinyin_core::NbestStepRow,
    ) -> Result<Option<oxpinyin_core::NbestSuccessors<'a>>, Self::Error> {
        Ok(Some(
            row.get_or_try_init(|| self.load_merged_row(prev.value(), None))?
                .map_or(
                    oxpinyin_core::NbestSuccessors::Miss,
                    oxpinyin_core::NbestSuccessors::Row,
                ),
        ))
    }
}

#[cfg(test)]
mod merge_tests {
    use super::merge_bigram;
    use oxpinyin_core::MergedGram;

    fn gram(total: u32, records: &[(u32, u32)]) -> Vec<u8> {
        let mut bytes = total.to_le_bytes().to_vec();
        for (token, count) in records {
            bytes.extend_from_slice(&token.to_le_bytes());
            bytes.extend_from_slice(&count.to_le_bytes());
        }
        bytes
    }

    fn merged(system: Option<Vec<u8>>, user: Option<Vec<u8>>) -> Option<Vec<u8>> {
        MergedGram::merge(system, user).map(|row| row.to_bytes())
    }

    /// `merge_single_gram`'s loop (`ngram.cpp:308-349` at `074a2219`),
    /// written out item by item: no sort, no dedup, equal heads summed,
    /// remainders appended.
    fn pin_merge(system: (u32, &[(u32, u32)]), user: (u32, &[(u32, u32)])) -> Vec<u8> {
        let mut items = Vec::new();
        let (mut left, mut right) = (0, 0);
        while left < system.1.len() && right < user.1.len() {
            let (s, u) = (system.1[left], user.1[right]);
            if s.0 < u.0 {
                items.push(s);
                left += 1;
            } else if s.0 > u.0 {
                items.push(u);
                right += 1;
            } else {
                items.push((s.0, s.1.wrapping_add(u.1)));
                left += 1;
                right += 1;
            }
        }
        items.extend_from_slice(&system.1[left..]);
        items.extend_from_slice(&user.1[right..]);
        gram(system.0.wrapping_add(user.0), &items)
    }

    #[test]
    fn row_merge_is_the_pins_two_pointer_interleave_for_arbitrary_rows() {
        for seed in 0..128_u32 {
            let system = [(seed % 7, u32::MAX), (3, seed), (seed % 5, 0), (3, 1)];
            let user = [(seed % 6, u32::MAX), (3, seed), (seed % 7, 0), (3, 1)];
            let (system_total, user_total) = (seed.wrapping_mul(0x8000_0001), seed.wrapping_mul(3));
            assert_eq!(
                merged(
                    Some(gram(system_total, &system)),
                    Some(gram(user_total, &user))
                ),
                Some(pin_merge((system_total, &system), (user_total, &user))),
                "seed {seed}"
            );
        }
    }

    #[test]
    fn row_merge_is_none_only_when_both_sides_miss() {
        assert!(merged(None, None).is_none());
        // One side absent: that side's bytes, item-less or not.
        let itemless = gram(7, &[]);
        assert_eq!(merged(None, Some(itemless.clone())), Some(itemless));
        let user = gram(0, &[(9, 1), (2, 0)]);
        assert_eq!(merged(None, Some(user.clone())), Some(user));
    }

    #[test]
    fn row_merge_unions_sorted_tokens() {
        assert_eq!(
            merged(
                Some(gram(10, &[(1, 3), (2, 4)])),
                Some(gram(11, &[(2, 5), (3, 6)]))
            ),
            Some(gram(21, &[(1, 3), (2, 9), (3, 6)]))
        );
    }

    #[test]
    fn row_merge_keeps_a_zero_total_system_row() {
        // `merge_single_gram` answers true as soon as one load answers.
        let system = gram(0, &[(5, 2)]);
        assert_eq!(merged(Some(system.clone()), None), Some(system));
        assert_eq!(merge_bigram(Some((2, 0)), 0, 0), Some((2, 0)));
    }

    #[test]
    fn bigram_merge_wraps_each_field_at_the_stored_width() {
        for (user, expected) in [(u32::MAX - 1, u32::MAX), (u32::MAX, 0)] {
            assert_eq!(
                merged(Some(gram(1, &[(1, 1)])), Some(gram(user, &[(1, user)]))),
                Some(gram(expected, &[(1, expected)]))
            );
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
}
