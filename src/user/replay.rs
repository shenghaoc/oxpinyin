//! Ordered replay of a system library's phrase journal.
//!
//! Each item record addresses one slot. Replay compares the old item as wire
//! bytes and applies the records before the first mismatch. REMOVE removes
//! the item before comparing; MODIFY compares before replacing it
//! (`phrase_index.cpp:439-517` at 074a2219).
//!
//! The mapped baseline stays immutable. Changed slots own their bytes, and
//! decoding is bounded to the record. A journal record whose payload extends
//! beyond the log rolls back the changes made by that replay.

use crate::user::persistence::record_skip;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, btree_map::Entry};

use crate::PHRASE_MASK;
use crate::data::chunk_format::{CHEWING_KEY_SIZE, ITEM_HEADER_SIZE};
use crate::data::chunk_write::{ChunkItem, decode_phrase_item};
use crate::data::phrase_library::{PhraseContent, PhraseItemView};
use crate::data::user_files::LogRecord;

#[cfg(test)]
use crate::user::persistence::SystemLibrary;
use crate::user::persistence::{SystemLibraryRef, journal_item_bytes};

/// Why the merge ended at a record.
pub(crate) enum Stop {
    /// `merge`'s `return false` at a REMOVE, MODIFY or header record.
    Mismatch(&'static str, u32),
    /// An ADD payload shorter than the unigram `add_phrase_item` reads:
    /// the pin's `get_unigram_frequency` asserts (`memory_chunk.h:390`,
    /// reached from `phrase_index.cpp:457` through `:215`; row 108). The
    /// caller returns [`JournalFault::ShortAdd`].
    ShortAdd(u32),
}

/// A pin assertion a system library's `.dbin` walk reaches (074a2219). The
/// shared Rust crates return it at the pin's point, after the records the pin
/// applied before it; the C adapters abort on it. Each names the library
/// (sub-index) whose log reached it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JournalFault {
    /// `PhraseIndexLogger::next_record`'s `assert(token == null_token)` on a
    /// `MODIFY_HEADER` record (`phrase_index_logger.h:202`), reached by
    /// `merge` and by `merge_with_mask`'s `_peek_header` (row 91).
    HeaderToken {
        /// The library whose log holds the record.
        library: u8,
        /// The record's token.
        token: u32,
    },
    /// `merge`'s ADD stores a payload shorter than six bytes and
    /// `add_phrase_item` reads its unigram through
    /// `PhraseItem::get_unigram_frequency`, whose `check_result` asserts
    /// (`memory_chunk.h:390` via `phrase_index.cpp:457`, `:215`; row 108).
    ShortAdd {
        /// The library whose log holds the record.
        library: u8,
        /// The record's token.
        token: u32,
    },
    /// `_peek_header`'s `assert(1 >= header_count)` (`phrase_index.cpp:745`),
    /// reached only by `merge_with_mask` (row 92).
    MultipleHeaders {
        /// The library whose log holds the headers.
        library: u8,
    },
    /// `_compute_new_header` reads the unigram of every record the mask keeps
    /// (`phrase_index.cpp:778`, `:785`, `:791`, `:795`): a payload shorter
    /// than six bytes asserts in `get_unigram_frequency`'s `check_result`
    /// (`memory_chunk.h:390`), the read of row 108 on `mask_out`'s path.
    ShortMaskedItem {
        /// The library whose log holds the record.
        library: u8,
        /// The record's token.
        token: u32,
    },
    /// `_mask_out_records` hands every kept record to `append_record` with
    /// both chunk pointers (`phrase_index.cpp:838`): a kept ADD fails
    /// `assert(NULL == oldone)` (`phrase_index_logger.h:245`, row 97) and a
    /// kept REMOVE fails `assert(NULL == newone)` (`:257`).
    SurvivingRecord {
        /// The library whose log holds the record.
        library: u8,
        /// The record's token.
        token: u32,
    },
}

impl JournalFault {
    /// The library (sub-index) whose log reached the assertion.
    #[cfg(test)]
    #[must_use]
    pub fn library(self) -> u8 {
        match self {
            Self::HeaderToken { library, .. }
            | Self::ShortAdd { library, .. }
            | Self::MultipleHeaders { library }
            | Self::ShortMaskedItem { library, .. }
            | Self::SurvivingRecord { library, .. } => library,
        }
    }
}

impl std::fmt::Display for JournalFault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderToken { library, token } => write!(
                f,
                "library {library}'s log: MODIFY_HEADER token {token:#010x} \
                 (upstream asserts, phrase_index_logger.h:202)"
            ),
            Self::ShortAdd { library, token } => write!(
                f,
                "library {library}'s log: ADD payload under six bytes for {token:#010x} \
                 (upstream asserts, memory_chunk.h:390)"
            ),
            Self::MultipleHeaders { library } => write!(
                f,
                "library {library}'s log: more than one MODIFY_HEADER \
                 (upstream asserts, phrase_index.cpp:745)"
            ),
            Self::ShortMaskedItem { library, token } => write!(
                f,
                "library {library}'s log: kept payload under six bytes for {token:#010x} \
                 (upstream asserts, memory_chunk.h:390)"
            ),
            Self::SurvivingRecord { library, token } => write!(
                f,
                "library {library}'s log: kept ADD or REMOVE for {token:#010x} \
                 (upstream asserts, phrase_index_logger.h:245/257)"
            ),
        }
    }
}

impl std::error::Error for JournalFault {}

/// What `FacadePhraseIndex::add_unigram_frequency` answers, through
/// `SubPhraseIndex::add_unigram_frequency`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnigramAdd {
    /// The field and the sub-index total took the delta.
    Accepted,
    /// `ERROR_INTEGER_OVERFLOW`: the total cannot take the delta; nothing was
    /// written.
    Refused,
    /// No sub-index or no readable item under the slot:
    /// `ERROR_NO_SUB_PHRASE_INDEX`, `ERROR_OUT_OF_RANGE`, `ERROR_NO_ITEM` or
    /// `ERROR_FILE_CORRUPTION`.
    Missing,
}

impl UnigramAdd {
    /// `ERROR_OK`.
    #[must_use]
    pub(crate) fn is_accepted(self) -> bool {
        self == Self::Accepted
    }
}

/// The byte owner of the shipped items. Inline fixtures and heap-loaded
/// baselines carry the same per-slot bounds as a mapped baseline.
enum Baseline {
    Mapped(PhraseContent),
    Inline(BTreeMap<u32, Vec<u8>>),
}

impl Baseline {
    fn fields(&self, slot: u32) -> Option<&[u8]> {
        match self {
            Self::Mapped(mapping) => mapping.item_fields(slot),
            Self::Inline(items) => items
                .get(&slot)
                .filter(|bytes| bytes.len() >= 2)
                .map(Vec::as_slice),
        }
    }

    fn contains(&self, slot: u32) -> bool {
        match self {
            Self::Mapped(mapping) => mapping.slot_offset(slot).is_some(),
            Self::Inline(items) => items.contains_key(&slot),
        }
    }
}

enum Slot {
    Item(Vec<u8>),
    Unreadable,
    Removed,
}

/// The unigram in a journal payload's six-byte wire header.
pub(crate) fn raw_unigram(item: &[u8]) -> u32 {
    item.get(2..ITEM_HEADER_SIZE)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

/// Replay owns the live library and records each changed slot's previous
/// value once. A mismatch keeps the prefix; an out-of-bounds journal record
/// restores this operation's starting state.
pub(crate) struct Merge {
    live: ReplayedLibrary,
    undo: BTreeMap<u32, Option<Slot>>,
    has_original: bool,
    always_retain: bool,
    initial_total: u32,
}

impl Merge {
    pub(crate) fn new(original: Option<SystemLibraryRef<'_>>) -> Self {
        let live = original.map_or_else(
            || ReplayedLibrary::new(Baseline::Inline(BTreeMap::new()), 0, 1),
            ReplayedLibrary::pristine,
        );
        Self::start(live, original.is_some(), false)
    }

    pub(crate) fn from_live(original: Option<SystemLibraryRef<'_>>, live: ReplayedLibrary) -> Self {
        Self::start(live, original.is_some(), true)
    }

    fn start(live: ReplayedLibrary, has_original: bool, always_retain: bool) -> Self {
        let initial_total = live.total;
        Self {
            live,
            undo: BTreeMap::new(),
            has_original,
            always_retain,
            initial_total,
        }
    }

    fn replace(&mut self, slot: u32, item: Slot) {
        let previous = self.live.changes.insert(slot, item);
        self.undo.entry(slot).or_insert(previous);
    }

    pub(crate) fn apply(&mut self, record: LogRecord<&[u8]>) -> Result<(), Stop> {
        match record {
            LogRecord::Add { token, new_item } => {
                if new_item.len() < ITEM_HEADER_SIZE {
                    return Err(Stop::ShortAdd(token));
                }
                self.live.total = self.live.total.wrapping_add(raw_unigram(new_item));
                self.replace(token & PHRASE_MASK, journal_slot(new_item));
                Ok(())
            }
            LogRecord::Remove { token, old_item } => self.remove(token, old_item),
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => self.modify(token, old_item, new_item),
            LogRecord::ModifyHeader {
                old_total,
                new_total,
            } => {
                if self.live.total != old_total {
                    return Err(Stop::Mismatch("header", 0));
                }
                self.live.total = new_total;
                Ok(())
            }
        }
    }

    /// A bounded header without totals can leave an already-zero total alone.
    pub(crate) fn apply_header_without_totals(&self) -> Result<(), Stop> {
        if self.live.total == 0 {
            Ok(())
        } else {
            Err(Stop::Mismatch("header", 0))
        }
    }

    fn remove(&mut self, token: u32, old_item: &[u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let mismatch = Stop::Mismatch("REMOVE", token);
        let Some(bytes) = self.live.complete_item(slot) else {
            // The class (b) result for an absent token is a no-op for a
            // readable old item. A removal that would read an incomplete
            // item stops at this record.
            if self.live.item_is_incomplete(slot) || PhraseItemView::from_bytes(old_item).is_none()
            {
                return Err(mismatch);
            }
            return Ok(());
        };
        let equal = bytes == old_item;
        self.live.total = self.live.total.wrapping_sub(raw_unigram(bytes));
        self.replace(slot, Slot::Removed);
        if equal { Ok(()) } else { Err(mismatch) }
    }

    fn modify(&mut self, token: u32, old_item: &[u8], new_item: &[u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let bytes = self
            .live
            .complete_item(slot)
            .filter(|&bytes| bytes == old_item)
            .ok_or(Stop::Mismatch("MODIFY", token))?;
        // The journal's total accounts for MODIFY records of the same size
        // or smaller in its header. A larger payload also replaces the old
        // unigram in the running total.
        if new_item.len() > bytes.len() {
            self.live.total = self
                .live
                .total
                .wrapping_sub(raw_unigram(bytes))
                .wrapping_add(raw_unigram(new_item));
        }
        self.replace(slot, journal_slot(new_item));
        Ok(())
    }

    pub(crate) fn rollback(mut self) -> ReplayedLibrary {
        for (slot, previous) in self.undo {
            match previous {
                Some(item) => {
                    self.live.changes.insert(slot, item);
                }
                None => {
                    self.live.changes.remove(&slot);
                }
            }
        }
        self.live.total = self.initial_total;
        self.live
    }

    pub(crate) fn finish_views(
        self,
        skipped: &mut Option<&mut Vec<String>>,
        name: &impl std::fmt::Display,
    ) -> Finished {
        let mut gone = BTreeSet::new();
        for &slot in self.undo.keys() {
            match self.live.changes.get(&slot) {
                Some(Slot::Unreadable) => {
                    record_skip(
                        skipped,
                        format_args!("{name}: item {slot} ends past its phrase record"),
                    );
                    if !self.always_retain {
                        gone.insert(slot);
                    }
                }
                Some(Slot::Removed) if !self.always_retain => {
                    gone.insert(slot);
                }
                _ => {}
            }
        }
        let total = self.has_original.then_some(self.live.total);
        let retained = (self.always_retain
            || !self.live.changes.is_empty()
            || self.live.total != self.initial_total)
            .then_some(self.live);
        (total, gone, retained)
    }

    #[cfg(test)]
    pub(crate) fn apply_owned(&mut self, record: LogRecord<Vec<u8>>) -> Result<(), Stop> {
        self.apply(match &record {
            LogRecord::Add { token, new_item } => LogRecord::Add {
                token: *token,
                new_item,
            },
            LogRecord::Remove { token, old_item } => LogRecord::Remove {
                token: *token,
                old_item,
            },
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => LogRecord::Modify {
                token: *token,
                old_item,
                new_item,
            },
            LogRecord::ModifyHeader {
                old_total,
                new_total,
            } => LogRecord::ModifyHeader {
                old_total: *old_total,
                new_total: *new_total,
            },
        })
    }
}

fn stored_fields(bytes: &[u8]) -> &[u8] {
    PhraseItemView::fields(bytes).map_or(bytes, |item| item.as_bytes())
}

fn journal_slot(bytes: &[u8]) -> Slot {
    PhraseItemView::from_bytes(bytes).map_or(Slot::Unreadable, |item| {
        Slot::Item(item.as_bytes().to_vec())
    })
}

/// A live sub-index's mapped baseline and independently owned slot changes.
pub(crate) struct ReplayedLibrary {
    baseline: Baseline,
    baseline_end: u32,
    changes: BTreeMap<u32, Slot>,
    total: u32,
}

impl std::fmt::Debug for ReplayedLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayedLibrary")
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}

impl ReplayedLibrary {
    fn new(baseline: Baseline, total: u32, baseline_end: u32) -> Self {
        Self {
            baseline,
            baseline_end,
            changes: BTreeMap::new(),
            total,
        }
    }

    pub(crate) fn from_mapping(mapping: PhraseContent, total: u32) -> Self {
        let end = mapping
            .offsets()
            .as_chunks::<4>()
            .0
            .iter()
            .rposition(|word| *word != [0; 4])
            .map_or(1, |last| u32::try_from(last + 1).unwrap_or(u32::MAX));
        Self::new(Baseline::Mapped(mapping), total, end)
    }

    pub(crate) fn pristine(original: SystemLibraryRef<'_>) -> Self {
        let items: BTreeMap<_, _> = match original.mapped {
            Some(mapped) => mapped
                .offset_array()
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter_map(|(slot, word)| {
                    if *word == [0; 4] {
                        return None;
                    }
                    let fields = mapped.item_bytes(slot).unwrap_or_default();
                    Some((u32::try_from(slot).ok()?, stored_fields(fields).to_vec()))
                })
                .collect(),
            None => original
                .items
                .iter()
                .map(|(&slot, item)| (slot, journal_item_bytes(item)))
                .collect(),
        };
        let end = items
            .last_key_value()
            .map_or(1, |(&slot, _)| slot.saturating_add(1));
        Self::new(Baseline::Inline(items), original.total, end)
    }

    fn fields(&self, slot: u32) -> Option<&[u8]> {
        match self.changes.get(&slot) {
            Some(Slot::Item(bytes)) => Some(bytes),
            Some(Slot::Unreadable | Slot::Removed) => None,
            None => self.baseline.fields(slot),
        }
    }

    fn edit_fields(&mut self, slot: u32) -> Option<&mut [u8]> {
        let item = match self.changes.entry(slot) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let bytes = stored_fields(self.baseline.fields(slot)?).to_vec();
                entry.insert(Slot::Item(bytes))
            }
        };
        match item {
            Slot::Item(bytes) => Some(bytes),
            Slot::Unreadable | Slot::Removed => None,
        }
    }

    fn complete_item(&self, slot: u32) -> Option<&[u8]> {
        Some(PhraseItemView::from_bytes(self.fields(slot)?)?.as_bytes())
    }

    pub(crate) fn item(&self, slot: u32) -> Option<ChunkItem> {
        decode_phrase_item(self.complete_item(slot)?)
    }

    /// Whether a slot holds an incomplete item.
    pub(crate) fn item_is_incomplete(&self, slot: u32) -> bool {
        matches!(self.changes.get(&slot), Some(Slot::Unreadable))
            || (self.fields(slot).is_some() && self.complete_item(slot).is_none())
    }

    pub(crate) fn pronunciation_count(&self, slot: u32) -> Option<u32> {
        self.fields(slot)?.get(1).copied().map(u32::from)
    }

    pub(crate) fn total(&self) -> u32 {
        self.total
    }

    pub(crate) fn set_total(&mut self, total: u32) {
        self.total = total;
    }

    pub(crate) fn unigram(&self, slot: u32) -> Option<u32> {
        self.complete_item(slot).map(raw_unigram)
    }

    pub(crate) fn add_unigram(&mut self, slot: u32, delta: u32) -> UnigramAdd {
        let Some(fields) = self
            .fields(slot)
            .filter(|bytes| bytes.len() >= ITEM_HEADER_SIZE)
        else {
            return UnigramAdd::Missing;
        };
        let Some(total) = self.total.checked_add(delta) else {
            return UnigramAdd::Refused;
        };
        let frequency = raw_unigram(fields).wrapping_add(delta);
        // Copy only this slot's record, even if several baseline slots
        // refer to the same file offset.
        let Some(bytes) = self.edit_fields(slot) else {
            return UnigramAdd::Missing;
        };
        bytes[2..ITEM_HEADER_SIZE].copy_from_slice(&frequency.to_le_bytes());
        self.total = total;
        UnigramAdd::Accepted
    }

    pub(crate) fn add_item(&mut self, slot: u32, payload: &[u8]) {
        self.changes.insert(slot, journal_slot(payload));
        self.total = self.total.wrapping_add(raw_unigram(payload));
    }

    pub(crate) fn remove_item(&mut self, slot: u32) -> Option<Vec<u8>> {
        let bytes = self.complete_item(slot)?.to_vec();
        self.changes.insert(slot, Slot::Removed);
        self.total = self.total.wrapping_sub(raw_unigram(&bytes));
        Some(bytes)
    }

    /// One past the final named slot. An unreadable record still occupies
    /// its slot for token allocation.
    pub(crate) fn range_end(&self) -> u32 {
        let last_change = self
            .changes
            .iter()
            .rev()
            .find(|(_, item)| !matches!(item, Slot::Removed))
            .map_or(1, |(&slot, _)| slot.saturating_add(1));
        let mut end = self.baseline_end.max(last_change);
        while end > last_change {
            let last = end - 1;
            if !matches!(self.changes.get(&last), Some(Slot::Removed))
                && self.baseline.contains(last)
            {
                break;
            }
            end -= 1;
        }
        end
    }

    pub(crate) fn has_added(&self) -> bool {
        self.changes
            .iter()
            .any(|(&slot, item)| matches!(item, Slot::Item(_)) && !self.baseline.contains(slot))
    }

    /// Apply pronunciation increments in stored reading order, using the
    /// model's wrapping 32-bit frequency sum. Earlier matching increments
    /// survive an overflow refusal later in the walk.
    pub(crate) fn train(
        &mut self,
        slot: u32,
        readings: impl Iterator<Item = Vec<crate::ChewingKey>>,
        delta: u32,
    ) -> bool {
        let Some(bytes) = self.complete_item(slot) else {
            return false;
        };
        let length = usize::from(bytes[0]);
        let first = ITEM_HEADER_SIZE + 4 * length;
        let end = bytes.len();
        let Some(bytes) = self.edit_fields(slot) else {
            return false;
        };
        let keys_len = CHEWING_KEY_SIZE * length;
        for reading in readings {
            let mut total = 0_u32;
            for entry in bytes[first..end].chunks_exact_mut(keys_len + 4) {
                let (keys, count_bytes) = entry.split_at_mut(keys_len);
                let Ok(count_bytes) = <&mut [u8; 4]>::try_from(count_bytes) else {
                    break;
                };
                let count = u32::from_le_bytes(*count_bytes);
                total = total.wrapping_add(count);
                let words = keys
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .copied()
                    .map(u16::from_le_bytes);
                if !crate::user::store_libpinyin::pronunciation_matches(&reading, words) {
                    continue;
                }
                let Some(next) = total.checked_add(delta) else {
                    break;
                };
                total = next;
                *count_bytes = count.wrapping_add(delta).to_le_bytes();
            }
        }
        true
    }

    pub(crate) fn item_bytes(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        self.complete_item(slot).map(Cow::Borrowed)
    }

    pub(crate) fn item_field_bytes(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        self.fields(slot).map(Cow::Borrowed)
    }

    pub(crate) fn is_added_slot(&self, slot: u32) -> bool {
        !self.baseline.contains(slot)
    }

    pub(crate) fn readable_unigram_sum(&self) -> u32 {
        (1..self.range_end()).fold(0_u32, |sum, slot| {
            sum.wrapping_add(self.unigram(slot).unwrap_or(0))
        })
    }

    pub(crate) fn has_removed(&self) -> bool {
        self.changes
            .values()
            .any(|item| matches!(item, Slot::Removed | Slot::Unreadable))
    }

    #[cfg(test)]
    pub(crate) fn slots(&self) -> impl DoubleEndedIterator<Item = u32> + '_ {
        self.changes
            .iter()
            .filter_map(|(&slot, item)| matches!(item, Slot::Item(_)).then_some(slot))
    }

    #[cfg(test)]
    pub(crate) fn add_candidates(&self) -> impl Iterator<Item = u32> + '_ {
        self.slots().filter(|&slot| self.is_added_slot(slot))
    }
}

/// The updated total, removed or unreadable slots and retained live sub-index.
pub(crate) type Finished = (Option<u32>, BTreeSet<u32>, Option<ReplayedLibrary>);

#[cfg(test)]
mod tests;

#[cfg(feature = "tools")]
pub mod tool;
