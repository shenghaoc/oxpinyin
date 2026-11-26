//! Ordered replay of a system library's phrase journal.
//!
//! Each item record addresses one slot. Replay compares the old item as wire
//! bytes and applies the records before the first mismatch. REMOVE removes
//! the item before comparing; MODIFY compares before replacing it
//! (`phrase_index.cpp:439-517` at 074a2219).
//!
//! The live sub-index keeps the pin's representation: one offset table and
//! one content buffer, read in place from the library's private mapping
//! until a write must grow them. A journal record whose payload extends
//! beyond the log rolls back the changes made by that replay.

use crate::user::record_skip;
use std::borrow::Cow;
use std::collections::BTreeSet;

use crate::data::phrase_library::ITEM_HEADER_SIZE;
use crate::data::phrase_library::{ChunkItem, decode_phrase_item};
use crate::data::phrase_library::{PhraseContent, PhraseItemView, item_record};
use crate::data::user_files::LogRecord;
use crate::vocab::PHRASE_MASK;

#[cfg(test)]
use crate::user::SystemLibrary;
use crate::user::SystemLibraryRef;
use crate::user::persistence::journal_item_bytes;

/// Why the merge ended at a record.
pub(crate) enum Stop {
    /// `merge`'s `return false` at a REMOVE, MODIFY or header record.
    Mismatch(&'static str, u32),
    /// An ADD payload shorter than the unigram `add_phrase_item` reads:
    /// the pin's `get_unigram_frequency` asserts (`memory_chunk.h:390`,
    /// reached from `phrase_index.cpp:457` through `:215`). The
    /// caller returns [`JournalFault::ShortAdd`].
    ShortAdd(u32),
}

/// A pin assertion a system library's `.dbin` walk reaches (074a2219). The
/// shared Rust crates return it at the pin's point, after the records the pin
/// applied before it; the C adapters abort on it. Each names the library
/// (sub-index) whose log reached it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum JournalFault {
    /// `PhraseIndexLogger::next_record`'s `assert(token == null_token)` on a
    /// `MODIFY_HEADER` record (`phrase_index_logger.h:202`), reached by
    /// `merge` and by `merge_with_mask`'s `_peek_header`.
    #[error(
        "library {library}'s log: MODIFY_HEADER token {token:#010x} \
         (upstream asserts, phrase_index_logger.h:202)"
    )]
    HeaderToken {
        /// The library whose log holds the record.
        library: u8,
        /// The record's token.
        token: u32,
    },
    /// `merge`'s ADD stores a payload shorter than six bytes and
    /// `add_phrase_item` reads its unigram through
    /// `PhraseItem::get_unigram_frequency`, whose `check_result` asserts
    /// (`memory_chunk.h:390` via `phrase_index.cpp:457`, `:215`).
    #[error(
        "library {library}'s log: ADD payload under six bytes for {token:#010x} \
         (upstream asserts, memory_chunk.h:390)"
    )]
    ShortAdd {
        /// The library whose log holds the record.
        library: u8,
        /// The record's token.
        token: u32,
    },
    /// `_peek_header`'s `assert(1 >= header_count)` (`phrase_index.cpp:745`),
    /// reached only by `merge_with_mask`.
    #[error(
        "library {library}'s log: more than one MODIFY_HEADER \
         (upstream asserts, phrase_index.cpp:745)"
    )]
    MultipleHeaders {
        /// The library whose log holds the headers.
        library: u8,
    },
    /// `_compute_new_header` reads the unigram of every record the mask keeps
    /// (`phrase_index.cpp:778`, `:785`, `:791`, `:795`): a payload shorter
    /// than six bytes asserts in `get_unigram_frequency`'s `check_result`
    /// (`memory_chunk.h:390`), the read of row 108 on `mask_out`'s path.
    #[error(
        "library {library}'s log: kept payload under six bytes for {token:#010x} \
         (upstream asserts, memory_chunk.h:390)"
    )]
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
    #[error(
        "library {library}'s log: kept ADD or REMOVE for {token:#010x} \
         (upstream asserts, phrase_index_logger.h:245/257)"
    )]
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

/// The unigram in a journal payload's six-byte wire header.
pub(crate) fn raw_unigram(item: &[u8]) -> u32 {
    item.get(2..ITEM_HEADER_SIZE)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

/// The offset of the content's first item when it starts empty:
/// `add_phrase_item` keeps offset 0 for "no item" (074a2219
/// `phrase_index.cpp:208-212`).
const FIRST_OFFSET: usize = 8;

/// The word `compact` keeps for a slot whose offset is not 0 but whose
/// item cannot be read: an offset past any content, as before the rebuild.
const PAST_CONTENT: u32 = u32::MAX;

/// A change that a replay's rollback undoes.
enum Undo {
    Word { slot: u32, word: u32 },
    Bytes { at: usize, bytes: Vec<u8> },
}

/// What a replay's rollback restores besides the undone changes.
struct Journal {
    undo: Vec<Undo>,
    /// The slots the replay's records addressed.
    touched: BTreeSet<u32>,
    offsets_len: usize,
    content_len: usize,
    total: u32,
}

/// Replay owns the live library and logs each change once. A mismatch
/// keeps the prefix; an out-of-bounds journal record restores this
/// operation's starting state.
pub(crate) struct Merge {
    live: ReplayedLibrary,
    has_original: bool,
    always_retain: bool,
    initial_total: u32,
}

impl Merge {
    pub(crate) fn new(original: Option<SystemLibraryRef<'_>>) -> Self {
        let live = original.map_or_else(ReplayedLibrary::empty, ReplayedLibrary::pristine);
        Self::start(live, original.is_some(), false)
    }

    pub(crate) fn from_live(original: Option<SystemLibraryRef<'_>>, live: ReplayedLibrary) -> Self {
        Self::start(live, original.is_some(), true)
    }

    fn start(mut live: ReplayedLibrary, has_original: bool, always_retain: bool) -> Self {
        let initial_total = live.total;
        live.journal = Some(Journal {
            undo: Vec::new(),
            touched: BTreeSet::new(),
            offsets_len: live.offsets().len(),
            content_len: live.content().len(),
            total: initial_total,
        });
        Self {
            live,
            has_original,
            always_retain,
            initial_total,
        }
    }

    pub(crate) fn apply(&mut self, record: LogRecord<&[u8]>) -> Result<(), Stop> {
        match record {
            LogRecord::Add { token, new_item } => {
                if new_item.len() < ITEM_HEADER_SIZE {
                    return Err(Stop::ShortAdd(token));
                }
                self.live.total = self.live.total.wrapping_add(raw_unigram(new_item));
                let slot = token & PHRASE_MASK;
                self.live.touch(slot);
                self.live.store_payload(slot, new_item);
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
        let Some(bytes) = self.live.complete_item(slot).map(|item| item.as_bytes()) else {
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
        self.live.touch(slot);
        self.live.set_word(slot, 0);
        if equal { Ok(()) } else { Err(mismatch) }
    }

    fn modify(&mut self, token: u32, old_item: &[u8], new_item: &[u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let old_len = self
            .live
            .complete_item(slot)
            .map(|item| item.as_bytes())
            .filter(|&bytes| bytes == old_item)
            .ok_or(Stop::Mismatch("MODIFY", token))?
            .len();
        self.live.touch(slot);
        // The journal's total accounts for MODIFY records of the same size
        // or smaller in its header. A larger payload also replaces the old
        // unigram in the running total.
        if new_item.len() > old_len {
            self.live.total = self
                .live
                .total
                .wrapping_sub(raw_unigram(old_item))
                .wrapping_add(raw_unigram(new_item));
        }
        // `phrase_index.cpp:486-500`: a payload no larger than the item is
        // written over it in place, whatever shares or follows its bytes;
        // a larger one is appended whole.
        if new_item.len() > old_len {
            self.live.store_payload(slot, new_item);
        } else {
            let offset = self.live.word(slot) as usize;
            self.live.write_bytes(offset, new_item);
        }
        Ok(())
    }

    /// Undoes every logged change in reverse, then cuts both buffers back
    /// to their lengths when the replay started.
    pub(crate) fn rollback(mut self) -> ReplayedLibrary {
        let Some(journal) = self.live.journal.take() else {
            return self.live;
        };
        for change in journal.undo.into_iter().rev() {
            match change {
                Undo::Word { slot, word } => self.live.put_word(slot, word),
                Undo::Bytes { at, bytes } => {
                    if let Some(target) = self.live.content_mut().get_mut(at..at + bytes.len()) {
                        target.copy_from_slice(&bytes);
                    }
                }
            }
        }
        if let Some(offsets) = &mut self.live.offsets {
            offsets.truncate(journal.offsets_len);
        }
        if let Some(content) = &mut self.live.content {
            content.truncate(journal.content_len);
        }
        self.live.total = journal.total;
        self.live
    }

    pub(crate) fn finish_views(
        mut self,
        skipped: &mut Option<&mut Vec<String>>,
        name: &impl std::fmt::Display,
    ) -> Finished {
        let journal = self.live.journal.take();
        let changed = journal
            .as_ref()
            .is_some_and(|journal| !journal.undo.is_empty());
        let touched = journal.map(|journal| journal.touched).unwrap_or_default();
        let mut gone = BTreeSet::new();
        for &slot in &touched {
            if self.live.item_is_incomplete(slot) {
                record_skip(
                    skipped,
                    format_args!("{name}: item {slot} ends past its phrase record"),
                );
                if !self.always_retain {
                    gone.insert(slot);
                }
            } else if self.live.word(slot) == 0 && !self.always_retain {
                gone.insert(slot);
            }
        }
        let total = self.has_original.then_some(self.live.total);
        let retained = (self.always_retain || changed || self.live.total != self.initial_total)
            .then_some(self.live);
        (total, gone, retained)
    }

    #[cfg(test)]
    pub(crate) fn apply_fixture(&mut self, record: &LogRecord<Vec<u8>>) -> Result<(), Stop> {
        self.apply(match record {
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

/// The little-endian word of `slot` in an offset table; 0 past its end.
fn word_at(offsets: &[u8], slot: u32) -> u32 {
    let at = slot as usize * 4;
    offsets
        .get(at..at + 4)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

/// The offset `add_phrase_item` gives the next item: the content size as
/// the pin's `guint32`, wrapping, and 8 where that is 0.
fn place_offset(length: usize) -> u32 {
    match length as u32 {
        0 => FIRST_OFFSET as u32,
        offset => offset,
    }
}

/// `add_phrase_item` (074a2219 `phrase_index.cpp:208-217`): the content
/// size as a `guint32` offset, wrapping as the pin's `table_offset_t`
/// does, with 8 for an empty content; then `set_content` writes `bytes`
/// there, over whatever an offset that wrapped finds, and grows the
/// content as far as they reach. A system sub-index and a `USER_FILE`
/// library both place their items here. An offset that wrapped lies at
/// least 4 GiB before the content end, so its write stays inside the
/// content.
pub(crate) fn place(content: &mut Vec<u8>, bytes: &[u8]) -> u32 {
    let offset = place_offset(content.len());
    let at = offset as usize;
    if at == content.len() {
        content.extend_from_slice(bytes);
        return offset;
    }
    let end = at + bytes.len();
    if content.len() < end {
        content.resize(end, 0);
    }
    content[at..end].copy_from_slice(bytes);
    offset
}

/// One bit per slot of the shipped offset table: whether its offset is not
/// 0. Built at the first write that changes an offset word, before the
/// table loses that knowledge.
struct ShippedSlots(Vec<u64>);

impl ShippedSlots {
    fn of(offsets: &[u8]) -> Self {
        let words = offsets.as_chunks::<4>().0;
        let mut bits = vec![0_u64; words.len().div_ceil(64)];
        for (slot, word) in words.iter().enumerate() {
            if *word != [0; 4]
                && let Some(chunk) = bits.get_mut(slot / 64)
            {
                *chunk |= 1 << (slot % 64);
            }
        }
        Self(bits)
    }

    fn contains(&self, slot: u32) -> bool {
        let slot = slot as usize;
        self.0
            .get(slot / 64)
            .is_some_and(|chunk| (chunk >> (slot % 64)) & 1 == 1)
    }
}

/// A live system sub-index as `SubPhraseIndex` keeps it (074a2219
/// `phrase_index.h:257-261`): one offset table and one content buffer.
/// Both are read in place from the library's private copy-on-write
/// mapping until a write must grow them; inline baselines own both.
/// A removal writes a 0 offset and an add appends to the content
/// (`phrase_index.cpp:208-235`); every other write goes in place, as the
/// pin's does, whatever shares or follows the record (policy row 111).
pub struct ReplayedLibrary {
    /// The library's private mapping, while a buffer still reads it.
    mapping: Option<PhraseContent>,
    /// The offset table and the content: `None` reads the mapping, as
    /// `MemoryChunk` reads its mapped chunk until a write must grow it
    /// and it copies it (074a2219 `memory_chunk.h:102-122`).
    offsets: Option<Vec<u8>>,
    content: Option<Vec<u8>>,
    shipped: Option<ShippedSlots>,
    /// The readable unigram sum the last `compact` found, while no write
    /// has followed it. `None` until the first rebuild, so the shipped
    /// layout is rebuilt once as the pin rebuilds it.
    compacted: Option<u32>,
    /// A replay's undo log.
    journal: Option<Journal>,
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
    fn owned(offsets: Vec<u8>, content: Vec<u8>, total: u32) -> Self {
        Self {
            mapping: None,
            offsets: Some(offsets),
            content: Some(content),
            shipped: None,
            compacted: None,
            journal: None,
            total,
        }
    }

    fn empty() -> Self {
        Self::owned(Vec::new(), Vec::new(), 0)
    }

    pub(crate) fn from_mapping(mapping: PhraseContent, total: u32) -> Self {
        Self {
            mapping: Some(mapping),
            offsets: None,
            content: None,
            shipped: None,
            compacted: None,
            journal: None,
            total,
        }
    }

    /// Builds a fresh mutable index from a library baseline, on owned
    /// buffers laid out as `add_phrase_item` lays them.
    #[must_use]
    pub fn pristine(original: SystemLibraryRef<'_>) -> Self {
        let mut offsets = Vec::new();
        let mut content = Vec::new();
        let mut store = |slot: u32, bytes: &[u8]| {
            let at = slot as usize * 4;
            if offsets.len() < at + 4 {
                offsets.resize(at + 4, 0);
            }
            offsets[at..at + 4].copy_from_slice(&place(&mut content, bytes).to_le_bytes());
        };
        match original.mapped {
            Some(mapped) => {
                for (slot, word) in mapped.offset_array().as_chunks::<4>().0.iter().enumerate() {
                    let Ok(index) = u32::try_from(slot) else {
                        break;
                    };
                    if *word != [0; 4] {
                        store(
                            index,
                            stored_fields(mapped.item_bytes(slot).unwrap_or_default()),
                        );
                    }
                }
            }
            None => {
                for (&slot, item) in original.items {
                    store(slot, &journal_item_bytes(item));
                }
            }
        }
        Self::owned(offsets, content, original.total)
    }

    fn offsets(&self) -> &[u8] {
        match &self.offsets {
            Some(bytes) => bytes,
            None => self.mapping.as_ref().map_or(&[], PhraseContent::offsets),
        }
    }

    fn content(&self) -> &[u8] {
        match &self.content {
            Some(bytes) => bytes,
            None => self.mapping.as_ref().map_or(&[], PhraseContent::as_slice),
        }
    }

    /// The content for an append: an owned copy of the mapped content the
    /// first time.
    fn content_vec(&mut self) -> &mut Vec<u8> {
        self.compacted = None;
        if self.content.is_none() {
            self.content = Some(self.content().to_vec());
            self.release_mapping();
        }
        self.content.get_or_insert_with(Vec::new)
    }

    /// The content for an in-place write: the mapping's copy-on-write
    /// pages, or the owned content.
    fn content_mut(&mut self) -> &mut [u8] {
        self.compacted = None;
        match &mut self.content {
            Some(bytes) => bytes,
            None => self
                .mapping
                .as_mut()
                .map_or(&mut [], PhraseContent::as_mut_slice),
        }
    }

    /// Drops the mapping once neither buffer reads it.
    fn release_mapping(&mut self) {
        if self.offsets.is_some() && self.content.is_some() {
            self.mapping = None;
        }
    }

    fn word(&self, slot: u32) -> u32 {
        word_at(self.offsets(), slot)
    }

    /// Writes `slot`'s offset word without a log entry: in place where the
    /// table holds the slot, into the mapping's copy-on-write page or the
    /// owned table; otherwise the table is grown, as `set_content` grows it.
    fn put_word(&mut self, slot: u32, word: u32) {
        self.compacted = None;
        let at = slot as usize * 4;
        if at + 4 > self.offsets().len() {
            if word == 0 {
                return;
            }
            if self.offsets.is_none() {
                self.offsets = Some(self.offsets().to_vec());
                self.release_mapping();
            }
            if let Some(bytes) = &mut self.offsets {
                bytes.resize(at + 4, 0);
            }
        }
        let table = match &mut self.offsets {
            Some(bytes) => Some(bytes.as_mut_slice()),
            None => self.mapping.as_mut().map(PhraseContent::offsets_mut),
        };
        if let Some(target) = table.and_then(|table| table.get_mut(at..at + 4)) {
            target.copy_from_slice(&word.to_le_bytes());
        }
    }

    /// Writes `slot`'s offset word, logged during a replay. The first write
    /// that changes a word first records which slots the shipped table
    /// holds.
    fn set_word(&mut self, slot: u32, word: u32) {
        let old = self.word(slot);
        if old == word {
            return;
        }
        if self.shipped.is_none() {
            self.shipped = Some(ShippedSlots::of(self.offsets()));
        }
        if let Some(journal) = &mut self.journal {
            journal.undo.push(Undo::Word { slot, word: old });
        }
        self.compacted = None;
        self.put_word(slot, word);
    }

    /// Notes a slot a replay's record addressed.
    fn touch(&mut self, slot: u32) {
        if let Some(journal) = &mut self.journal {
            journal.touched.insert(slot);
        }
    }

    /// Stores a payload at `slot` as `add_phrase_item` does: its bytes go
    /// to the content as they are, and the offset is written. A read takes
    /// the extent its header declares.
    fn store_payload(&mut self, slot: u32, payload: &[u8]) {
        let length = self.content().len();
        // An offset that wrapped writes over earlier content: log it.
        let wrapped = {
            let at = place_offset(length) as usize;
            (at < length && self.journal.is_some()).then(|| {
                (
                    at,
                    self.content()[at..length.min(at + payload.len())].to_vec(),
                )
            })
        };
        if let (Some((at, bytes)), Some(journal)) = (wrapped, &mut self.journal) {
            journal.undo.push(Undo::Bytes { at, bytes });
        }
        let offset = place(self.content_vec(), payload);
        self.set_word(slot, offset);
    }

    /// Overwrites content bytes at `at`, logged during a replay.
    fn write_bytes(&mut self, at: usize, bytes: &[u8]) {
        if self.journal.is_some() {
            let old = self
                .content()
                .get(at..at + bytes.len())
                .unwrap_or_default()
                .to_vec();
            if let Some(journal) = &mut self.journal {
                journal.undo.push(Undo::Bytes { at, bytes: old });
            }
        }
        if let Some(target) = self.content_mut().get_mut(at..at + bytes.len()) {
            target.copy_from_slice(bytes);
        }
    }

    fn fields(&self, slot: u32) -> Option<&[u8]> {
        item_record(self.content(), self.word(slot)).filter(|record| record.len() >= 2)
    }

    /// Where [`Self::fields`] lies in the content: the slot's offset is
    /// read once, as `get_phrase_item` reads it (`phrase_index.cpp:180-206`),
    /// so a write goes to the same bytes without a second lookup.
    fn record(&self, slot: u32) -> Option<std::ops::Range<usize>> {
        let at = self.word(slot);
        let length = item_record(self.content(), at)?.len();
        (length >= 2).then(|| at as usize..at as usize + length)
    }

    /// The complete item under `slot`, checked once: every read of a
    /// complete item derives what it needs from this view.
    pub(crate) fn complete_item(&self, slot: u32) -> Option<PhraseItemView<'_>> {
        PhraseItemView::from_bytes(self.fields(slot)?)
    }

    /// Decodes a complete item at the supplied slot.
    #[must_use]
    pub fn item(&self, slot: u32) -> Option<ChunkItem> {
        decode_phrase_item(self.complete_item(slot)?.as_bytes())
    }

    /// Whether a slot holds an incomplete item.
    pub(crate) fn item_is_incomplete(&self, slot: u32) -> bool {
        self.fields(slot).is_some() && self.complete_item(slot).is_none()
    }

    pub(crate) fn total(&self) -> u32 {
        self.total
    }

    pub(crate) fn set_total(&mut self, total: u32) {
        self.total = total;
    }

    pub(crate) fn unigram(&self, slot: u32) -> Option<u32> {
        self.complete_item(slot)
            .map(|item| raw_unigram(item.as_bytes()))
    }

    /// `SubPhraseIndex::add_unigram_frequency` (`phrase_index.cpp:150-176`):
    /// one offset read, the overflow guard, then the frequency is read and
    /// written in place, through the view any slot sharing the bytes reads.
    pub(crate) fn add_unigram(&mut self, slot: u32, delta: u32) -> UnigramAdd {
        let Some(record) = self
            .record(slot)
            .filter(|record| record.len() >= ITEM_HEADER_SIZE)
        else {
            return UnigramAdd::Missing;
        };
        let Some(total) = self.total.checked_add(delta) else {
            return UnigramAdd::Refused;
        };
        let Some(bytes) = self.content_mut().get_mut(record) else {
            return UnigramAdd::Missing;
        };
        let frequency = raw_unigram(bytes).wrapping_add(delta);
        bytes[2..ITEM_HEADER_SIZE].copy_from_slice(&frequency.to_le_bytes());
        self.total = total;
        UnigramAdd::Accepted
    }

    /// Installs the item bytes and adds their unigram count to the index total.
    pub fn add_item(&mut self, slot: u32, payload: &[u8]) {
        self.store_payload(slot, payload);
        self.total = self.total.wrapping_add(raw_unigram(payload));
    }

    /// Takes the complete item at `slot` out, and its unigram count out of
    /// the index total; whether there was one.
    pub(crate) fn remove_item(&mut self, slot: u32) -> bool {
        let Some(item) = self.complete_item(slot) else {
            return false;
        };
        let unigram = raw_unigram(item.as_bytes());
        self.set_word(slot, 0);
        self.total = self.total.wrapping_sub(unigram);
        true
    }

    /// `get_range`'s end (074a2219 `phrase_index.cpp:624-646`): one past
    /// the last offset that is not 0, at least 1.
    pub(crate) fn range_end(&self) -> u32 {
        self.offsets()
            .as_chunks::<4>()
            .0
            .iter()
            .rposition(|word| *word != [0; 4])
            .map_or(1, |last| u32::try_from(last + 1).unwrap_or(u32::MAX).max(1))
    }

    /// Apply pronunciation increments in stored reading order, using the
    /// model's wrapping 32-bit frequency sum. Earlier matching increments
    /// survive an overflow refusal later in the walk.
    pub(crate) fn train(
        &mut self,
        slot: u32,
        readings: &mut impl crate::model::ReadingWalk,
        delta: u32,
    ) -> bool {
        // One offset read gives the item (`get_phrase_item`,
        // `phrase_index.cpp:180-206`); a complete item is trained in place.
        let Some(record) = self.record(slot).filter(|record| {
            self.content()
                .get(record.clone())
                .and_then(PhraseItemView::from_bytes)
                .is_some()
        }) else {
            return false;
        };
        let Some(bytes) = self.content_mut().get_mut(record) else {
            return false;
        };
        crate::user::user_library::train_readings(bytes, readings, delta);
        true
    }

    pub(crate) fn item_bytes(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        self.complete_item(slot)
            .map(|item| Cow::Borrowed(item.as_bytes()))
    }

    /// The complete item under `slot`, read in place, unless the shipped
    /// table lacks the slot ([`Self::is_added_slot`]: an import's or a
    /// replayed ADD's item). Every other slot answers the item
    /// [`Self::item_bytes`] answers.
    pub(crate) fn ordinary_item(&self, slot: u32) -> Option<PhraseItemView<'_>> {
        if self.is_added_slot(slot) {
            return None;
        }
        PhraseItemView::from_bytes(self.fields(slot)?)
    }

    /// The item's fields under `slot`, as `get_phrase_item` reads them: one
    /// offset load, then the two header bytes size the view
    /// (`phrase_index.cpp:180-206`). `None` for an empty or out-of-content
    /// offset and for a record too short for its header, where
    /// [`Self::item_field_bytes`] answers no item with a header either.
    pub(crate) fn field_view(&self, slot: u32) -> Option<PhraseItemView<'_>> {
        let content = self.content();
        let at = self.word(slot) as usize;
        if at == 0 || at >= content.len() {
            return None;
        }
        PhraseItemView::fields(content.get(at..)?)
    }

    pub(crate) fn item_field_bytes(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        self.fields(slot).map(Cow::Borrowed)
    }

    /// Whether the shipped offset table held no item at `slot`.
    pub(crate) fn is_added_slot(&self, slot: u32) -> bool {
        match &self.shipped {
            Some(shipped) => !shipped.contains(slot),
            None => self.word(slot) == 0,
        }
    }

    pub(crate) fn readable_unigram_sum(&self) -> u32 {
        (1..self.range_end()).fold(0_u32, |sum, slot| {
            sum.wrapping_add(self.unigram(slot).unwrap_or(0))
        })
    }

    /// `compact` (074a2219 `phrase_index.cpp:648-675`): one pass over the
    /// offset table builds a new table and content, with each readable
    /// item copied in slot order as `add_phrase_item` places it, and adds
    /// up the readable items' unigrams, as `add_phrase_item` does
    /// (`:215`). Returns that sum. A slot whose offset is not 0 but whose
    /// item cannot be read keeps an offset past any content, so it reads
    /// and occupies as before. A library not written since its last
    /// rebuild is left as it is and answers the sum that rebuild found.
    pub(crate) fn compact(&mut self) -> u32 {
        if let Some(total) = self.compacted {
            return total;
        }
        let end = self.range_end() as usize;
        let words = self.offsets().as_chunks::<4>().0;
        let source = self.content();
        let mut shipped = self
            .shipped
            .is_none()
            .then(|| vec![0_u64; words.len().div_ceil(64)]);
        let mut offsets = Vec::with_capacity(end * 4);
        let mut content = Vec::with_capacity(source.len().max(FIRST_OFFSET));
        let mut total = 0_u32;
        for (slot, word) in words.iter().enumerate() {
            let word = u32::from_le_bytes(*word);
            if word != 0
                && let Some(bits) = &mut shipped
                && let Some(chunk) = bits.get_mut(slot / 64)
            {
                *chunk |= 1 << (slot % 64);
            }
            if slot >= end {
                continue;
            }
            let moved = if slot == 0 || word == 0 {
                0
            } else {
                match item_record(source, word).filter(|record| record.len() >= 2) {
                    Some(fields) => {
                        if let Some(item) = PhraseItemView::from_bytes(fields) {
                            total = total.wrapping_add(raw_unigram(item.as_bytes()));
                        }
                        place(&mut content, fields)
                    }
                    None => PAST_CONTENT,
                }
            };
            offsets.extend_from_slice(&moved.to_le_bytes());
        }
        offsets.resize(end * 4, 0);
        if let Some(bits) = shipped {
            self.shipped = Some(ShippedSlots(bits));
        }
        self.offsets = Some(offsets);
        self.content = Some(content);
        self.mapping = None;
        self.compacted = Some(total);
        total
    }

    #[cfg(test)]
    pub(crate) fn add_candidates(&self) -> impl Iterator<Item = u32> + '_ {
        (1..self.range_end())
            .filter(|&slot| self.is_added_slot(slot) && self.complete_item(slot).is_some())
    }
}

/// The updated total, removed or incomplete slots and retained live sub-index.
pub(crate) type Finished = (Option<u32>, BTreeSet<u32>, Option<ReplayedLibrary>);

#[cfg(test)]
mod tests;
