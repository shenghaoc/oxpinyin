//! The pin's `SubPhraseIndex::merge` over one system library's journal
//! (`phrase_index.cpp:439-517` at 074a2219), over the pin's own storage: one
//! phrase-content buffer, a slot-to-offset index and `m_total_freq`.
//!
//! * `PhraseItem::operator==` is equal chunk sizes, then `memcmp`
//!   (`phrase_index.h:235-240`).
//! * A loaded library's content is the mmap'd item area
//!   (`SubPhraseIndex::load`, `:301-330`); `MemoryChunk::mmap` maps it
//!   writable (`memory_chunk.h:505-506`), so an in-place MODIFY writes into
//!   it. The first `add_phrase_item` past its end copies the content into one
//!   allocated buffer and appends there (`:208-217`).
//! * `get_phrase_item` reads the two header bytes through
//!   `MemoryChunk::get_content`, which bounds-checks the whole buffer
//!   (`memory_chunk.h:372-377`), then hands out the header-derived length;
//!   a payload shorter than its header therefore reads the following bytes
//!   as long as they are inside the buffer (`:180-206`).
//! * REMOVE takes the item and its unigram out before it compares, so a
//!   mismatch keeps the removal (`:460-475`, `:219-236`).
//! * MODIFY compares first; a longer new chunk is a remove plus an add of the
//!   whole payload, otherwise it is `memmove`d over the stored bytes
//!   (`:477-501`). Neither the new chunk nor an ADD payload is checked
//!   against its own header.
//! * `item`, the `PhraseItem` MODIFY reads into, outlives its call: when
//!   `get_phrase_item` fails the compare runs against what it still points
//!   at (`:478-483`); before the first successful read that is the default
//!   `PhraseItem`, six zero bytes (`phrase_index.h:81-84`). A view into the
//!   copy an append may have moved is not read here.
//! * A read that ends inside the content buffer returns the bytes there: a
//!   payload shorter than its header borrows the following bytes, a 0- or
//!   1-byte payload's header included. A read that ends past the buffer's end
//!   is class (b) (row 107); the merge stops at a record that would read it
//!   and [`Merge::finish`] drops it.
//! * The first failure ends the merge and the records before it stay applied
//!   (`pinyin.cpp:276` ignores the answer).
//!
//! The records come from [`oxpinyin_data::user_files::log_records`], the pin's
//! own walk; the walk's and the merge's assertions are [`JournalFault`]s.
//! Session opens keep each retained payload's private byte view. Typed [`ChunkItem`]s are cached on use; value-only callers use
//! [`Merge::finish`].

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use oxpinyin_data::chunk_write::{
    ChunkItem, PHRASE_MASK, decode_phrase_item, validate_phrase_item,
};
use oxpinyin_data::phrase_library::PhraseLibrary;
use oxpinyin_data::user_files::LogRecord;

#[cfg(test)]
use crate::persistence::SystemLibrary;
use crate::persistence::{SystemLibraryRef, journal_item_bytes};

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

/// `phrase_item_header` at 074a2219: the two length bytes and the unigram.
const PHRASE_ITEM_HEADER: usize = 6;

/// `SubPhraseIndex::add_phrase_item`'s first offset: 0 becomes 8 so 0 stays
/// the "no item" sentinel.
const FIRST_ITEM_OFFSET: usize = 8;

/// What `get_phrase_item` answers at a slot.
enum Found {
    /// The item's offset and header-derived length, both inside the buffer.
    Item { offset: usize, len: usize },
    /// The header names a length that ends past the content buffer's end:
    /// the pin reads past its allocation (class (b), row 107).
    Past,
    /// The header bytes are not in the buffer: `get_phrase_item` failed.
    Failed,
}

/// The `PhraseItem` MODIFY last read into, by where it points.
#[derive(Clone, Copy)]
enum Place {
    /// The pin's default `PhraseItem`, six zero bytes of its own.
    Default,
    /// The shipped mapping, before or after its first copy.
    Shipped { offset: usize, len: usize },
    /// The copy `add_phrase_item` made, at a given append count.
    Owned { offset: usize, len: usize },
}

enum Stale {
    Default(Vec<u8>),
    Shipped {
        offset: usize,
        len: usize,
    },
    Owned {
        offset: usize,
        len: usize,
        appends: u64,
    },
}

/// The private mapping the pin edits, or inline byte buffers for a fixture.
enum Backing {
    Mapped(oxpinyin_data::phrase_library::PhraseContent),
    Inline { offsets: Vec<u8>, content: Vec<u8> },
}

/// Which of the sub-index's two chunks an operation addresses.
#[derive(Clone, Copy)]
enum Area {
    Offsets,
    Content,
}

impl Backing {
    fn bytes(&self, area: Area) -> &[u8] {
        match (self, area) {
            (Self::Mapped(map), Area::Content) => map.as_slice(),
            (Self::Mapped(map), Area::Offsets) => map.offsets(),
            (Self::Inline { content, .. }, Area::Content) => content,
            (Self::Inline { offsets, .. }, Area::Offsets) => offsets,
        }
    }
    fn bytes_mut(&mut self, area: Area) -> &mut [u8] {
        match (self, area) {
            (Self::Mapped(map), Area::Content) => map.as_mut_slice(),
            (Self::Mapped(map), Area::Offsets) => map.offsets_mut(),
            (Self::Inline { content, .. }, Area::Content) => content,
            (Self::Inline { offsets, .. }, Area::Offsets) => offsets,
        }
    }
}

/// One chunk's growth as `MemoryChunk` keeps it (`memory_chunk.h:102-130`).
///
/// `data_end` is the pin's `size()` and `capacity` its `capacity()`. Both are
/// logical: the reached-payload outcome and the zero-filled extension depend
/// on them, so they are tracked here and never read from a `Vec`. `owned` is
/// the copy `ensure_has_more_space` makes on the first growth of a chunk that
/// is a view into the mapping; until then every write lands in the mapping.
struct Chunk {
    owned: Option<Vec<u8>>,
    data_end: usize,
    capacity: usize,
}

impl Chunk {
    const fn shipped(len: usize) -> Self {
        Self {
            owned: None,
            data_end: len,
            capacity: len,
        }
    }

    /// Whether a write of `len` bytes at `offset` lands in the mapping.
    fn hits_mapping(&self, offset: usize, len: usize) -> bool {
        self.owned.is_none()
            && offset
                .checked_add(len)
                .is_some_and(|end| end <= self.capacity)
    }

    fn byte(&self, shipped: &[u8], pos: usize) -> Option<u8> {
        match &self.owned {
            Some(owned) if pos < self.capacity => Some(owned.get(pos).copied().unwrap_or(0)),
            Some(_) => None,
            None => shipped.get(pos).copied(),
        }
    }

    /// `MemoryChunk::set_content` (`memory_chunk.h:285-291`): write at `offset`,
    /// growing as `ensure_has_space` does. Returns whether the write was legal
    /// (the end does not overflow).
    fn set_content(&mut self, shipped: &mut [u8], offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else {
            return false;
        };
        match &mut self.owned {
            Some(owned) => {
                if end > self.data_end {
                    if end > self.capacity {
                        self.capacity = self.capacity.saturating_mul(2).max(end);
                    }
                    self.data_end = end;
                }
                if end > owned.len() {
                    owned.resize(end, 0);
                }
                owned[offset..end].copy_from_slice(bytes);
            }
            None if end <= self.data_end => shipped[offset..end].copy_from_slice(bytes),
            None => {
                let mut owned = shipped.to_vec();
                owned.resize(end, 0);
                owned[offset..end].copy_from_slice(bytes);
                self.capacity = end;
                self.data_end = end;
                self.owned = Some(owned);
            }
        }
        true
    }

    /// A write at a pointer the pin already holds: no growth, no new size. It
    /// may land in the allocation past the pin's `size()`.
    fn poke(&mut self, shipped: &mut [u8], offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else {
            return false;
        };
        if end > self.capacity {
            return false;
        }
        match &mut self.owned {
            Some(owned) => {
                if end > owned.len() {
                    owned.resize(end, 0);
                }
                owned[offset..end].copy_from_slice(bytes);
                true
            }
            None => Self::poke_shipped(shipped, offset, bytes),
        }
    }

    fn poke_shipped(shipped: &mut [u8], offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else {
            return false;
        };
        let Some(target) = shipped.get_mut(offset..end) else {
            return false;
        };
        target.copy_from_slice(bytes);
        true
    }
}

/// The sub-index the pin edits in place: its offset array (`m_phrase_index`)
/// and its phrase content (`m_phrase_content`), each a view into the private
/// mapping until its first growth.
struct Content {
    backing: Backing,
    items: Chunk,
    slots: Chunk,
    // Inject a failed decode independently of the earlier header bounds check.
    #[cfg(test)]
    item_override: Option<(usize, Vec<u8>)>,
}

impl Content {
    fn new(backing: Backing) -> Self {
        let items = Chunk::shipped(backing.bytes(Area::Content).len());
        let slots = Chunk::shipped(backing.bytes(Area::Offsets).len());
        Self {
            backing,
            items,
            slots,
            #[cfg(test)]
            item_override: None,
        }
    }
    fn data_len(&self) -> usize {
        self.items.data_end
    }
    fn buffer_capacity(&self) -> usize {
        self.items.capacity
    }
    fn byte(&self, pos: usize) -> Option<u8> {
        self.items.byte(self.backing.bytes(Area::Content), pos)
    }
    fn header(&self, offset: usize) -> Found {
        if offset
            .checked_add(2)
            .is_none_or(|end| end > self.data_len())
        {
            return Found::Failed;
        }
        let (Some(length), Some(prons)) = (self.byte(offset), self.byte(offset + 1)) else {
            return Found::Failed;
        };
        let length = usize::from(length);
        let len = PHRASE_ITEM_HEADER + 4 * length + usize::from(prons) * (2 * length + 4);
        match offset.checked_add(len) {
            Some(end) if end <= self.buffer_capacity() => Found::Item { offset, len },
            _ => Found::Past,
        }
    }
    fn item(&self, offset: usize, len: usize) -> Option<Cow<'_, [u8]>> {
        #[cfg(test)]
        if let Some((at, bytes)) = &self.item_override
            && *at == offset
        {
            return Some(Cow::Borrowed(bytes));
        }
        let end = offset.checked_add(len)?;
        if end > self.items.capacity {
            return None;
        }
        match &self.items.owned {
            Some(owned) if end <= owned.len() => Some(Cow::Borrowed(&owned[offset..end])),
            Some(_) => Some(Cow::Owned(
                (offset..end)
                    .map(|pos| self.byte(pos))
                    .collect::<Option<Vec<_>>>()?,
            )),
            None => self.shipped_item(offset, len),
        }
    }
    fn shipped_item(&self, offset: usize, len: usize) -> Option<Cow<'_, [u8]>> {
        Some(Cow::Borrowed(
            self.backing
                .bytes(Area::Content)
                .get(offset..offset.checked_add(len)?)?,
        ))
    }
    /// `add_phrase_item`'s `set_content(m_phrase_content.size(), ...)`.
    fn append(&mut self, payload: &[u8]) -> u32 {
        let offset = if self.data_len() == 0 {
            FIRST_ITEM_OFFSET
        } else {
            self.data_len()
        };
        let shipped = self.backing.bytes_mut(Area::Content);
        self.items.set_content(shipped, offset, payload);
        u32::try_from(offset).unwrap_or(u32::MAX)
    }
    /// Whether a content write of `len` bytes at `offset` lands in the mapping.
    fn content_write_hits_mapping(&self, offset: usize, len: usize) -> bool {
        self.items.hits_mapping(offset, len)
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> bool {
        let shipped = self.backing.bytes_mut(Area::Content);
        self.items.poke(shipped, offset, bytes)
    }
    fn write_shipped(&mut self, offset: usize, bytes: &[u8]) -> bool {
        Chunk::poke_shipped(self.backing.bytes_mut(Area::Content), offset, bytes)
    }
    /// Whether a write at `offset` of `len` bytes would land in the mapping's
    /// content, whatever the chunk's growth: for a stale `Shipped` place.
    fn shipped_has(&self, offset: usize, len: usize) -> bool {
        offset
            .checked_add(len)
            .is_some_and(|end| end <= self.backing.bytes(Area::Content).len())
    }
    /// `get_content(slot * 4, &offset, 4)` on the offset array: `None` where
    /// the array ends before the slot (`ERROR_OUT_OF_RANGE`).
    fn slot_offset(&self, slot: u32) -> Option<u32> {
        let pos = (slot as usize).checked_mul(4)?;
        if pos.checked_add(4)? > self.slots.data_end {
            return None;
        }
        let shipped = self.backing.bytes(Area::Offsets);
        let word = match &self.slots.owned {
            Some(owned) => owned.get(pos..pos + 4)?,
            None => shipped.get(pos..pos + 4)?,
        };
        Some(u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
    }
    /// Write an offset word back into the mapping, whatever the chunk's growth.
    fn restore_slot(&mut self, slot: u32, old: u32) {
        if let Some(pos) = (slot as usize).checked_mul(4) {
            Chunk::poke_shipped(
                self.backing.bytes_mut(Area::Offsets),
                pos,
                &old.to_le_bytes(),
            );
        }
    }
    /// Whether setting `slot` writes the mapping's offset array in place.
    fn slot_write_hits_mapping(&self, slot: u32) -> bool {
        (slot as usize)
            .checked_mul(4)
            .is_some_and(|pos| self.slots.hits_mapping(pos, 4))
    }
    /// `m_phrase_index.set_content(slot * 4, &offset, 4)`.
    fn set_slot_offset(&mut self, slot: u32, offset: u32) {
        let Some(pos) = (slot as usize).checked_mul(4) else {
            return;
        };
        let shipped = self.backing.bytes_mut(Area::Offsets);
        self.slots.set_content(shipped, pos, &offset.to_le_bytes());
    }
    /// Put the mapping's bytes and both chunks' state back as they were
    /// before the replay's first write: the undo entries restore the mapped
    /// bytes, and this drops any grown copy.
    fn reset_growth(&mut self) {
        self.items = Chunk::shipped(self.backing.bytes(Area::Content).len());
        self.slots = Chunk::shipped(self.backing.bytes(Area::Offsets).len());
    }
}

/// Where a mapped library's item offsets come from.
enum Origin<'a> {
    Mapped(&'a PhraseLibrary),
    /// A fixture `SystemLibrary`'s inline items, laid out as the pin's
    /// appended content would be. `bytes` is the shipped item area, kept
    /// for the finish-time overlap comparison.
    Inline {
        offsets: BTreeMap<u32, OriginalItem>,
        bytes: Vec<u8>,
    },
    None,
}

struct OriginalItem {
    offset: u32,
    unigram: u32,
    /// The original payload's stored length, before any replay write.
    len: u32,
}

/// The content and offsets a replay starts from.
fn origin_and_content(original: Option<SystemLibraryRef<'_>>) -> (Origin<'_>, Content) {
    match original {
        Some(library) => match &library.mapped {
            Some(mapped) => (
                Origin::Mapped(mapped),
                Content::new(mapped.private_content().map_or_else(
                    |_| Backing::Inline {
                        offsets: mapped.offset_array().to_vec(),
                        content: mapped.item_area().to_vec(),
                    },
                    Backing::Mapped,
                )),
            ),
            None if !library.items.is_empty() => {
                let mut bytes = vec![0; FIRST_ITEM_OFFSET];
                let mut offsets = BTreeMap::new();
                for (slot, item) in library.items {
                    let item_bytes = journal_item_bytes(item);
                    offsets.insert(
                        *slot,
                        OriginalItem {
                            offset: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
                            unigram: item.unigram,
                            len: u32::try_from(item_bytes.len()).unwrap_or(u32::MAX),
                        },
                    );
                    bytes.extend_from_slice(&item_bytes);
                }
                // The offset array a fixture would have been stored with: one
                // word per slot up to the highest, zero where no item is.
                let slots = offsets
                    .keys()
                    .next_back()
                    .map_or(0, |slot| *slot as usize + 1);
                let mut array = vec![0_u8; 4 * slots];
                for (slot, item) in &offsets {
                    let at = *slot as usize * 4;
                    array[at..at + 4].copy_from_slice(&item.offset.to_le_bytes());
                }
                (
                    Origin::Inline {
                        offsets,
                        bytes: bytes.clone(),
                    },
                    Content::new(Backing::Inline {
                        offsets: array,
                        content: bytes,
                    }),
                )
            }
            None => (Origin::None, Content::new(empty_backing())),
        },
        None => (Origin::None, Content::new(empty_backing())),
    }
}

fn empty_backing() -> Backing {
    Backing::Inline {
        offsets: Vec::new(),
        content: Vec::new(),
    }
}

/// `PhraseItem::get_unigram_frequency` read from an item's bytes: the `guint32`
/// after the two length bytes (`phrase_index.h:124-126`).
pub(crate) fn raw_unigram(item: &[u8]) -> u32 {
    item.get(2..6)
        .and_then(|bytes| bytes.try_into().ok())
        .map_or(0, u32::from_le_bytes)
}

/// The header-derived length of `bytes` when `bytes` holds that much; for
/// judging an old chunk's shape, not for reading the content.
fn declared_len(bytes: &[u8]) -> Option<usize> {
    let (&length, &prons) = (bytes.first()?, bytes.get(1)?);
    let (length, prons) = (usize::from(length), usize::from(prons));
    Some(PHRASE_ITEM_HEADER + 4 * length + prons * (2 * length + 4))
}

fn visible_len(bytes: &[u8]) -> Option<usize> {
    let len = declared_len(bytes)?;
    (len <= bytes.len()).then_some(len)
}

/// What reverses one in-place write to the mapping. A write that lands in a
/// grown copy has no entry: reversing drops the copy.
enum Undo<'l> {
    /// An offset word the replay overwrote in the mapping.
    Offset { slot: u32, old: u32 },
    /// Content bytes the replay overwrote in the mapping. The old bytes are the
    /// checked old payload of the record that wrote them, borrowed from the
    /// journal; no payload is copied.
    Content { offset: usize, old: &'l [u8] },
}

pub(crate) struct Merge<'a, 'l> {
    origin: Origin<'a>,
    content: Content,
    /// One entry per mutation of the mapping, in order. Dropped when the replay
    /// ends or stops; applied backwards only by [`Self::rollback`].
    undo: Vec<Undo<'l>>,
    /// Slot IDs only. Replay appends; finalization sorts and deduplicates before
    /// any membership query or ascending walk. No record payloads are staged.
    touched: Vec<u32>,
    /// Slots whose unigram changed, or whose write needs a conservative check.
    /// Payloads and unchanged reading-only slots are never staged here.
    unigram_changes: Vec<u32>,
    /// Validation retained an item absent from the replay origin. Without one,
    /// ADD-view consumers need not revisit every retained original slot.
    has_added_items: bool,
    /// A finished in-place write's header claims more than the slot it wrote.
    overreach: bool,
    /// Slots whose offset word the replay wrote: relocated or removed.
    relocated: BTreeSet<u32>,
    /// `SubPhraseIndex::m_total_freq`; `None` for a library with no original.
    total: Option<u32>,
    /// The same total for a library with no original to start from.
    running: u32,
    /// The `PhraseItem` MODIFY reads into.
    stale: Stale,
    /// The slot behind `stale`, which an absent-token MODIFY can still edit.
    stale_slot: Option<u32>,
    /// `add_phrase_item` calls so far.
    appends: u64,
}

impl<'a, 'l> Merge<'a, 'l> {
    pub(crate) fn new(original: Option<SystemLibraryRef<'a>>) -> Self {
        let (origin, content) = origin_and_content(original);
        Self {
            origin,
            content,
            undo: Vec::new(),
            touched: Vec::new(),
            unigram_changes: Vec::new(),
            has_added_items: false,
            overreach: false,
            relocated: BTreeSet::new(),
            total: original.as_ref().map(|library| library.total),
            running: original.as_ref().map_or(0, |library| library.total),
            stale: Stale::Default(vec![0; PHRASE_ITEM_HEADER]),
            stale_slot: None,
            appends: 0,
        }
    }

    /// `get_content(slot * 4, ...)` on the offset array: `0`, the "no item"
    /// sentinel, where the array ends before the slot.
    fn slot_offset(&self, slot: u32) -> u32 {
        self.content.slot_offset(slot).unwrap_or(0)
    }

    /// The original item's unigram and stored length at `slot`, before any
    /// replay write. `None` for a slot the shipped chunk does not hold.
    fn original_item(&self, slot: u32) -> Option<(u32, usize)> {
        match &self.origin {
            Origin::Mapped(library) => library
                .item(slot)
                .map(|item| (item.unigram(), item.as_bytes().len())),
            Origin::Inline { offsets, .. } => offsets
                .get(&slot)
                .map(|item| (item.unigram, item.len as usize)),
            Origin::None => None,
        }
    }

    /// The shipped item area, for the finish-time tail comparison.
    fn original_bytes(&self, start: usize, end: usize) -> Option<&[u8]> {
        match &self.origin {
            Origin::Mapped(library) => library.item_area().get(start..end),
            Origin::Inline { bytes, .. } => bytes.get(start..end),
            Origin::None => None,
        }
    }

    /// The original slots whose bytes a finished over-length item changed.
    /// Only a touched slot whose header claims more than its original payload
    /// takes this walk; the finish validation already read both lengths.
    #[cold]
    #[inline(never)]
    fn original_area_len(&self) -> usize {
        match &self.origin {
            Origin::Mapped(library) => library.item_area().len(),
            Origin::Inline { bytes, .. } => bytes.len(),
            Origin::None => 0,
        }
    }

    #[cold]
    #[inline(never)]
    fn overlapped_slots_at_finish(
        &self,
        offset: usize,
        original_extent: usize,
        current_len: usize,
    ) -> Vec<u32> {
        let tail_start = offset + original_extent;
        let tail_len = current_len.saturating_sub(original_extent);
        if tail_len == 0 {
            return Vec::new();
        }
        let area_len = self.original_area_len();
        let tail_end = tail_start.saturating_add(tail_len).min(area_len);
        if tail_end <= tail_start {
            return Vec::new();
        }
        let tail_len = tail_end - tail_start;
        let Some(current) = self.content.item(tail_start, tail_len) else {
            return Vec::new();
        };
        let Some(original) = self.original_bytes(tail_start, tail_end) else {
            return Vec::new();
        };
        let changed: Vec<bool> = current
            .iter()
            .zip(original)
            .map(|(current, original)| current != original)
            .collect();
        if !changed.contains(&true) {
            return Vec::new();
        }
        let mut slots = Vec::new();
        let mut consider = |slot: u32, start: usize, len: usize| {
            let begin = start.max(tail_start);
            let end = start.saturating_add(len).min(tail_start + changed.len());
            if begin < end && changed[begin - tail_start..end - tail_start].contains(&true) {
                slots.push(slot);
            }
        };
        match &self.origin {
            Origin::Mapped(library) => {
                for (slot, item) in library.items() {
                    if let Some(start) = library.content_offset(slot) {
                        consider(slot, start as usize, item.as_bytes().len());
                    }
                }
            }
            Origin::Inline { offsets, .. } => {
                for (&slot, item) in offsets {
                    consider(slot, item.offset as usize, item.len as usize);
                }
            }
            Origin::None => {}
        }
        slots
    }

    fn set_offset(&mut self, slot: u32, offset: u32) {
        self.touched.push(slot);
        // ADD, REMOVE and relocation take the conservative path. Finalization
        // discards removed/rejected slots; seeding checks the final delta.
        self.unigram_changes.push(slot);
        self.relocated.insert(slot);
        if self.content.slot_write_hits_mapping(slot)
            && let Some(old) = self.content.slot_offset(slot)
        {
            self.undo.push(Undo::Offset { slot, old });
        }
        self.content.set_slot_offset(slot, offset);
    }

    /// `SubPhraseIndex::get_phrase_item` for the slot.
    fn visible(&self, slot: u32) -> Found {
        let offset = self.slot_offset(slot);
        if offset == 0 {
            Found::Failed
        } else {
            self.content.header(offset as usize)
        }
    }

    fn add_total(&mut self, delta: u32) {
        self.running = self.running.wrapping_add(delta);
        self.total = self.total.map(|total| total.wrapping_add(delta));
    }

    fn subtract_total(&mut self, delta: u32) {
        self.running = self.running.wrapping_sub(delta);
        self.total = self.total.map(|total| total.wrapping_sub(delta));
    }

    pub(crate) fn apply(&mut self, record: LogRecord<&'l [u8]>) -> Result<(), Stop> {
        match record {
            LogRecord::Add { token, new_item } => self.add(token, new_item),
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
                // The header must land on the current total, as the merge has
                // moved it so far; a mismatch is merge's failure return
                // (`phrase_index.cpp:503-514`).
                let now = self.total.unwrap_or(self.running);
                if now != old_total {
                    return Err(Stop::Mismatch("header", 0));
                }
                self.running = new_total;
                self.total = self.total.map(|_| new_total);
                Ok(())
            }
        }
    }

    fn add(&mut self, token: u32, payload: &[u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        // `add_phrase_item` stores the payload as it stands (`:208-217`); it
        // does not read the header, then reads the payload's unigram
        // (`phrase_index.h:124-126`). A payload shorter than six bytes
        // aborts the pin at `memory_chunk.h:390` (row 108): the caller
        // returns the fault, and what the pin stored first dies with it.
        if payload.len() < PHRASE_ITEM_HEADER {
            return Err(Stop::ShortAdd(token));
        }
        let unigram = raw_unigram(payload);
        let offset = self.content.append(payload);
        self.appends += 1;
        self.set_offset(slot, offset);
        self.add_total(unigram);
        Ok(())
    }

    fn remove(&mut self, token: u32, old_item: &'l [u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        match self.visible(slot) {
            // The item's length ends past the content buffer: the pin copies
            // that many bytes (class (b), row 107). The subject stops.
            Found::Past => Err(Stop::Mismatch("REMOVE", token)),
            // `get_phrase_item` fails and `merge` dereferences the NULL item
            // (`phrase_index.cpp:468`, row 107): the removal of an item that is
            // not there is a no-op here, and an old chunk that is no item at
            // all is a mismatch.
            Found::Failed => {
                if visible_len(old_item).is_none() {
                    return Err(Stop::Mismatch("REMOVE", token));
                }
                self.set_offset(slot, 0);
                Ok(())
            }
            Found::Item { offset, len } => {
                let bytes = self
                    .content
                    .item(offset, len)
                    .ok_or(Stop::Mismatch("REMOVE", token))?;
                let unigram = raw_unigram(&bytes);
                let equal = bytes.as_ref() == old_item;
                // `remove_phrase_item` first: the item and its unigram leave,
                // then the chunks are compared and a mismatch keeps the
                // removal.
                self.set_offset(slot, 0);
                self.subtract_total(unigram);
                if equal {
                    Ok(())
                } else {
                    Err(Stop::Mismatch("REMOVE", token))
                }
            }
        }
    }

    fn modify(&mut self, token: u32, old_item: &'l [u8], new_item: &[u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let mismatch = || Stop::Mismatch("MODIFY", token);
        match self.visible(slot) {
            // The item's length ends past the content buffer: the compare or
            // the remove-and-add would read past it (class (b), row 107).
            Found::Past => Err(mismatch()),
            Found::Item { offset, len } => {
                self.stale_slot = Some(slot);
                self.stale = if self.content.items.owned.is_some() {
                    Stale::Owned {
                        offset,
                        len,
                        appends: self.appends,
                    }
                } else {
                    Stale::Shipped { offset, len }
                };
                let bytes = self.content.item(offset, len).ok_or_else(mismatch)?;
                if bytes.as_ref() != old_item {
                    return Err(mismatch());
                }
                // A short write may retain part of the previous unigram.
                // Keep that slot conservative instead of guessing its value.
                let unigram_changed = new_item
                    .get(2..6)
                    .is_none_or(|new| bytes.get(2..6).is_none_or(|old| old != new));
                if new_item.len() > len {
                    // A remove and an add of the whole new payload.
                    let old_unigram = raw_unigram(&bytes);
                    let new_unigram = raw_unigram(new_item);
                    self.subtract_total(old_unigram);
                    self.add_total(new_unigram);
                    let offset = self.content.append(new_item);
                    self.appends += 1;
                    self.set_offset(slot, offset);
                } else {
                    // `memmove` over the stored bytes; the item keeps its
                    // header-derived length for the next read. The finish
                    // validation reads the changed tail past the original
                    // payload and adds any overlapped slot.
                    let same_header = new_item.get(0..2) == bytes.get(0..2);
                    let overreach =
                        !same_header && declared_len(new_item).is_some_and(|new_len| new_len > len);
                    // The compare above made `old_item` the stored bytes, so its
                    // prefix is what this write overwrites.
                    if self
                        .content
                        .content_write_hits_mapping(offset, new_item.len())
                        && let Some(old) = old_item.get(..new_item.len())
                    {
                        self.undo.push(Undo::Content { offset, old });
                    }
                    if !self.content.write(offset, new_item) {
                        return Err(mismatch());
                    }
                    if overreach {
                        self.overreach = true;
                    }
                    self.touched.push(slot);
                    if unigram_changed {
                        self.unigram_changes.push(slot);
                    }
                }
                Ok(())
            }
            Found::Failed => self.modify_stale(old_item, new_item).ok_or_else(mismatch),
        }
    }

    fn stale_place(&self) -> Option<Place> {
        match &self.stale {
            Stale::Default(_) => Some(Place::Default),
            Stale::Shipped { offset, len } => Some(Place::Shipped {
                offset: *offset,
                len: *len,
            }),
            Stale::Owned {
                offset,
                len,
                appends,
            } => (*appends == self.appends).then_some(Place::Owned {
                offset: *offset,
                len: *len,
            }),
        }
    }

    fn stale_size(&self) -> usize {
        match &self.stale {
            Stale::Default(bytes) => bytes.len(),
            Stale::Shipped { len, .. } | Stale::Owned { len, .. } => *len,
        }
    }

    fn stale_read(&self, place: Place) -> Option<Cow<'_, [u8]>> {
        match place {
            Place::Default => match &self.stale {
                Stale::Default(bytes) => Some(Cow::Borrowed(bytes)),
                Stale::Shipped { .. } | Stale::Owned { .. } => None,
            },
            Place::Shipped { offset, len } => self.content.shipped_item(offset, len),
            Place::Owned { offset, len } => self.content.item(offset, len),
        }
    }

    fn stale_write(&mut self, place: Place, bytes: &[u8], old: &'l [u8]) -> bool {
        match place {
            Place::Default => match &mut self.stale {
                Stale::Default(buffer) => {
                    buffer[..bytes.len()].copy_from_slice(bytes);
                    true
                }
                Stale::Shipped { .. } | Stale::Owned { .. } => false,
            },
            Place::Shipped { offset, .. } => {
                // The stale compare made `old` the bytes at this place.
                if self.content.shipped_has(offset, bytes.len())
                    && let Some(old) = old.get(..bytes.len())
                {
                    self.undo.push(Undo::Content { offset, old });
                }
                self.content.write_shipped(offset, bytes)
            }
            Place::Owned { offset, .. } => self.content.write(offset, bytes),
        }
    }

    /// MODIFY of a token `get_phrase_item` does not find: the compare runs
    /// against the stale `item` (`phrase_index.cpp:478-483`). Before the first
    /// successful read that is the pin's default `PhraseItem`, six zero bytes
    /// (`phrase_index.h:81-84`), so an all-zero old chunk passes and a
    /// non-zero one differs. Equal bytes of the same size go on to `memmove`
    /// over that storage; a longer new chunk calls `remove_phrase_item` for the
    /// missing token and `assert(olditem == *tmpitem)` dereferences NULL
    /// (`:486-491`, row 107), which stops here.
    fn modify_stale(&mut self, old_item: &'l [u8], new_item: &[u8]) -> Option<()> {
        let place = self.stale_place()?;
        let size = self.stale_size();
        let old = self.stale_read(place);
        let equal = old.as_ref().is_some_and(|bytes| bytes.as_ref() == old_item);
        if !equal || new_item.len() > size {
            return None;
        }
        let same_header = old
            .as_ref()
            .is_none_or(|bytes| new_item.get(0..2) == bytes.get(0..2));
        let overreach =
            !same_header && declared_len(new_item).is_some_and(|new_len| new_len > size);
        drop(old);
        if !self.stale_write(place, new_item, old_item) {
            return None;
        }
        if overreach {
            self.overreach = true;
        }
        if let Some(slot) = self.stale_slot {
            self.unigram_changes.push(slot);
        }
        Some(())
    }

    /// The library's total and its overrides as typed items; `None` removes.
    /// Reverse every mutation of the mapping, last first, and drop any grown
    /// copy: the library keeps its pre-replay content. Only the payload
    /// over-read calls this; a mismatch or an unknown-record stop keeps the
    /// applied prefix and drops the entries instead.
    pub(crate) fn rollback(mut self) -> ReplayedLibrary {
        while let Some(entry) = self.undo.pop() {
            match entry {
                Undo::Offset { slot, old } => self.content.restore_slot(slot, old),
                Undo::Content { offset, old } => {
                    self.content.write_shipped(offset, old);
                }
            }
        }
        self.content.reset_growth();
        ReplayedLibrary {
            content: self.content,
            slots: Vec::new(),
            unigram_changes: Vec::new(),
            has_added_items: false,
        }
    }

    pub(crate) fn finish(
        mut self,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
    ) -> (Option<u32>, BTreeMap<u32, Option<ChunkItem>>) {
        self.sort_touched();
        self.expand_overlong();
        let mut typed = Vec::with_capacity(self.touched.len());
        for &slot in &self.touched {
            let offset = self.slot_offset(slot);
            if offset == 0 {
                typed.push((slot, None));
                continue;
            }
            match self.content.header(offset as usize) {
                // `get_phrase_item` fails for the live slot: the diff sees no
                // item there, as a removal.
                Found::Failed => typed.push((slot, None)),
                // The item's length ends past the content buffer: the pin's
                // diff reads past its allocation (row 107). The subject keeps
                // the library's own item.
                Found::Past => {
                    typed.push((slot, None));
                    skipped.push(format!("{name}: item {slot} ends past the phrase content"));
                }
                Found::Item { offset, len } => match self.content.item(offset, len) {
                    Some(bytes) => match decode_phrase_item(&bytes) {
                        Ok(item) => typed.push((slot, Some(item))),
                        Err(error) => skipped.push(format!("{name}: {error}")),
                    },
                    None => skipped.push(format!("{name}: item {slot} was not readable")),
                },
            }
        }
        // Ascending slots: the map is built in one pass.
        let overrides = typed.into_iter().collect();
        (self.total, overrides)
    }

    fn sort_touched(&mut self) {
        self.touched.sort_unstable();
        self.touched.dedup();
    }
}

/// A completed private sub-index view. It owns only the replay content;
/// finish resolves the retained slots' offsets while the baseline is borrowed.
/// No baseline byte owner survives the replay operation.
pub(crate) struct ReplayedLibrary {
    content: Content,
    slots: Vec<(u32, u32)>,
    unigram_changes: Vec<u32>,
    has_added_items: bool,
}

impl std::fmt::Debug for ReplayedLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayedLibrary")
            .field("slots", &self.slots.len())
            .finish()
    }
}

impl ReplayedLibrary {
    pub(crate) fn slots(&self) -> impl DoubleEndedIterator<Item = u32> + '_ {
        self.slots.iter().map(|&(slot, _)| slot)
    }
    pub(crate) fn add_candidates(&self) -> impl Iterator<Item = u32> + '_ {
        let slots = if self.has_added_items {
            self.slots.as_slice()
        } else {
            &[]
        };
        slots.iter().map(|&(slot, _)| slot)
    }
    pub(crate) fn unigram_slots(&self) -> impl Iterator<Item = u32> + '_ {
        self.unigram_changes.iter().copied()
    }
    pub(crate) fn contains(&self, slot: u32) -> bool {
        self.slots
            .binary_search_by_key(&slot, |&(slot, _)| slot)
            .is_ok()
    }
    pub(crate) fn bytes(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        let index = self
            .slots
            .binary_search_by_key(&slot, |&(slot, _)| slot)
            .ok()?;
        let offset = self.slots[index].1 as usize;
        let Found::Item { offset, len } = self.content.header(offset) else {
            return None;
        };
        self.content.item(offset, len)
    }
    pub(crate) fn item(&self, slot: u32) -> Option<ChunkItem> {
        decode_phrase_item(&self.bytes(slot)?).ok()
    }
    /// Whether the slot names an item that would end past the live buffer.
    pub(crate) fn item_past_buffer(&self, slot: u32) -> bool {
        self.content.slot_offset(slot).is_some_and(|offset| {
            offset != 0 && matches!(self.content.header(offset as usize), Found::Past)
        })
    }

    /// The live buffer of a library no journal touched: the shipped chunk's
    /// bytes in a private mapping, with no slot touched.
    pub(crate) fn pristine(original: SystemLibraryRef<'_>) -> Self {
        let (_, content) = origin_and_content(Some(original));
        Self {
            content,
            slots: Vec::new(),
            unigram_changes: Vec::new(),
            has_added_items: false,
        }
    }

    /// `PhraseItem::increase_pronunciation_possibility` over the buffer
    /// (074a2219 `phrase_index.cpp:114-143`): every reading in turn walks the
    /// item's entries in stored order, adds each entry's current count to a
    /// running `guint32` total, and adds `delta` to each entry whose keys
    /// match, stopping the walk at the first addition the total cannot take
    /// and keeping the entries already updated. The counts are written in
    /// place. Returns whether the buffer holds a readable item at `slot`;
    /// `readings` is not consumed otherwise.
    pub(crate) fn train(
        &mut self,
        slot: u32,
        readings: impl Iterator<Item = Vec<oxpinyin_core::ChewingKey>>,
        delta: u32,
    ) -> bool {
        let Some(offset) = self.content.slot_offset(slot).filter(|&offset| offset != 0) else {
            return false;
        };
        let Found::Item { offset, len } = self.content.header(offset as usize) else {
            return false;
        };
        let Some((length, entries)) = self
            .content
            .item(offset, len)
            .and_then(|bytes| Some((usize::from(*bytes.first()?), usize::from(*bytes.get(1)?))))
        else {
            return false;
        };
        let row = 2 * length + 4;
        let first = offset + PHRASE_ITEM_HEADER + 4 * length;
        for reading in readings {
            let mut total = 0_u32;
            for place in 0..entries {
                let keys_at = first + place * row;
                let count_at = keys_at + 2 * length;
                // The entry's last byte is in the buffer, so every byte before
                // it is; the walk reads the keys and the count where they are.
                if self.content.byte(count_at + 3).is_none() {
                    break;
                }
                let byte = |at: usize| self.content.byte(at).unwrap_or(0);
                let count = u32::from_le_bytes([
                    byte(count_at),
                    byte(count_at + 1),
                    byte(count_at + 2),
                    byte(count_at + 3),
                ]);
                total = total.wrapping_add(count);
                let words = (0..length).map(|key| {
                    u16::from_le_bytes([byte(keys_at + 2 * key), byte(keys_at + 2 * key + 1)])
                });
                if !crate::store_libpinyin::pronunciation_matches(&reading, words) {
                    continue;
                }
                let Some(next) = total.checked_add(delta) else {
                    break;
                };
                total = next;
                self.content
                    .write(count_at, &count.wrapping_add(delta).to_le_bytes());
            }
        }
        if let Err(at) = self.slots.binary_search_by_key(&slot, |&(slot, _)| slot) {
            self.slots.insert(at, (slot, offset as u32));
        }
        true
    }

    /// The item under `slot` as the live buffer holds it, whether or not the
    /// replay touched the slot, and whether it did. `None` where the buffer
    /// holds no readable item there.
    pub(crate) fn live_item(&self, slot: u32) -> Option<(Cow<'_, [u8]>, bool)> {
        let offset = self.content.slot_offset(slot)?;
        if offset == 0 {
            return None;
        }
        let Found::Item { offset, len } = self.content.header(offset as usize) else {
            return None;
        };
        Some((self.content.item(offset, len)?, self.contains(slot)))
    }
}

impl<'a> Merge<'a, '_> {
    /// Add every original slot whose bytes a finished over-length item
    /// changed. The eager path decodes all touched slots afterwards; the lazy
    /// path validates the added slots through its own loop.
    fn expand_overlong(&mut self) {
        let mut index = 0;
        while index < self.touched.len() {
            let slot = self.touched[index];
            index += 1;
            let offset = self.slot_offset(slot);
            if offset == 0 {
                continue;
            }
            let Found::Item { offset, len } = self.content.header(offset as usize) else {
                continue;
            };
            let Some((_unigram, original_extent)) = self.original_item(slot) else {
                continue;
            };
            if !self.relocated.contains(&slot) && len > original_extent {
                for extra in self.overlapped_slots_at_finish(offset, original_extent, len) {
                    if extra != slot && !self.touched.contains(&extra) {
                        self.touched.push(extra);
                    }
                }
            }
        }
        self.sort_touched();
    }

    /// Validate one slot for the lazy view. Returns whether the slot stays in
    /// the replayed view; `next` receives the original slots its over-length
    /// item changed.
    #[cold]
    #[inline(never)]
    fn finish_slot(
        &mut self,
        slot: u32,
        typed: &mut BTreeMap<u32, Option<ChunkItem>>,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
        next: &mut Vec<u32>,
        retain_adds: bool,
    ) -> bool {
        let relocated = self.relocated.contains(&slot);
        let offset = self.slot_offset(slot);
        if offset == 0 {
            typed.insert(slot, None);
            return false;
        }
        match self.content.header(offset as usize) {
            Found::Failed => {
                typed.insert(slot, None);
                false
            }
            Found::Past => {
                typed.insert(slot, None);
                skipped.push(format!("{name}: item {slot} ends past the phrase content"));
                false
            }
            Found::Item { offset, len } => match self.original_item(slot) {
                Some((original_unigram, original_extent)) => {
                    match self.content.item(offset, len) {
                        Some(bytes) => {
                            if let Err(error) = validate_phrase_item(&bytes) {
                                skipped.push(format!("{name}: {error}"));
                                return false;
                            }
                            // A malformed header can make another slot's write
                            // overlap this unigram. The validation pass already
                            // reads both views: retain that count change even
                            // if its own records only changed readings.
                            if raw_unigram(&bytes) != original_unigram {
                                self.unigram_changes.push(slot);
                            }
                            if !relocated && len > original_extent {
                                for extra in
                                    self.overlapped_slots_at_finish(offset, original_extent, len)
                                {
                                    if extra != slot && !next.contains(&extra) {
                                        next.push(extra);
                                    }
                                }
                            }
                            true
                        }
                        None => {
                            skipped.push(format!("{name}: item {slot} was not readable"));
                            false
                        }
                    }
                }
                None => {
                    if retain_adds {
                        if let Some(bytes) = self.content.item(offset, len) {
                            match validate_phrase_item(&bytes) {
                                Ok(()) => {
                                    self.has_added_items = true;
                                    return true;
                                }
                                Err(error) => skipped.push(format!("{name}: {error}")),
                            }
                        }
                        return false;
                    }
                    if let Some(bytes) = self.content.item(offset, len) {
                        match decode_phrase_item(&bytes) {
                            Ok(item) => {
                                typed.insert(slot, Some(item));
                            }
                            Err(error) => skipped.push(format!("{name}: {error}")),
                        }
                    }
                    false
                }
            },
        }
    }

    #[cold]
    #[inline(never)]
    fn finish_overreach(
        &mut self,
        typed: &mut BTreeMap<u32, Option<ChunkItem>>,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
        retain_adds: bool,
    ) {
        let mut slots = std::mem::take(&mut self.touched);
        let mut processed: BTreeSet<u32> = slots.iter().copied().collect();
        let mut extra = Vec::new();
        slots.retain(|&slot| self.finish_slot(slot, typed, skipped, name, &mut extra, retain_adds));
        extra.sort_unstable();
        extra.dedup();
        extra.retain(|slot| processed.insert(*slot));
        let mut added = false;
        while !extra.is_empty() {
            let mut next = Vec::new();
            for slot in extra {
                if self.finish_slot(slot, typed, skipped, name, &mut next, retain_adds) {
                    slots.push(slot);
                    added = true;
                }
            }
            next.sort_unstable();
            next.dedup();
            next.retain(|slot| processed.insert(*slot));
            extra = next;
        }
        if added {
            slots.sort_unstable();
        }
        self.touched = slots;
    }

    /// Keep original live slots as one private view. Added and removed slots
    /// still use the session's existing value/index/removal paths.
    #[cfg(test)]
    pub(crate) fn finish_lazy(
        self,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
    ) -> (
        Option<u32>,
        BTreeMap<u32, Option<ChunkItem>>,
        Option<ReplayedLibrary>,
    ) {
        self.finish_lazy_impl(skipped, name, false)
    }

    /// Retain ADD bodies as views too; the session seeds their rows through
    /// the same conversion as USER_FILE items.
    pub(crate) fn finish_views(
        self,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
    ) -> (
        Option<u32>,
        BTreeMap<u32, Option<ChunkItem>>,
        Option<ReplayedLibrary>,
    ) {
        self.finish_lazy_impl(skipped, name, true)
    }

    fn finish_lazy_impl(
        mut self,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
        retain_adds: bool,
    ) -> (
        Option<u32>,
        BTreeMap<u32, Option<ChunkItem>>,
        Option<ReplayedLibrary>,
    ) {
        self.sort_touched();
        let mut typed = BTreeMap::new();
        if !self.overreach {
            self.touched.retain(|&slot| {
                let offset = self.content.slot_offset(slot).unwrap_or(0);
                if offset == 0 {
                    typed.insert(slot, None);
                    return false;
                }
                match self.content.header(offset as usize) {
                    Found::Failed => {
                        typed.insert(slot, None);
                        false
                    }
                    Found::Past => {
                        // Row 107, option 1: the read leaves the buffer, so the
                        // item answers no item. The other changes stay.
                        typed.insert(slot, None);
                        skipped.push(format!("{name}: item {slot} ends past the phrase content"));
                        false
                    }
                    Found::Item { offset, len } => {
                        let original_unigram = match &self.origin {
                            Origin::Mapped(library) => {
                                library.item(slot).map(|item| item.unigram())
                            }
                            Origin::Inline { offsets, .. } => {
                                offsets.get(&slot).map(|item| item.unigram)
                            }
                            Origin::None => None,
                        };
                        if let Some(original_unigram) = original_unigram {
                            match self.content.item(offset, len) {
                                Some(bytes) => {
                                    if let Err(error) = validate_phrase_item(&bytes) {
                                        skipped.push(format!("{name}: {error}"));
                                        return false;
                                    }
                                    if raw_unigram(&bytes) != original_unigram {
                                        self.unigram_changes.push(slot);
                                    }
                                }
                                None => {
                                    skipped.push(format!("{name}: item {slot} was not readable"));
                                    return false;
                                }
                            }
                            return true;
                        }
                        if let Some(bytes) = self.content.item(offset, len) {
                            if retain_adds {
                                return match validate_phrase_item(&bytes) {
                                    Ok(()) => {
                                        self.has_added_items = true;
                                        true
                                    }
                                    Err(error) => {
                                        skipped.push(format!("{name}: {error}"));
                                        false
                                    }
                                };
                            }
                            match decode_phrase_item(&bytes) {
                                Ok(item) => {
                                    typed.insert(slot, Some(item));
                                }
                                Err(error) => skipped.push(format!("{name}: {error}")),
                            }
                        }
                        false
                    }
                }
            });
        } else {
            self.finish_overreach(&mut typed, skipped, name, retain_adds);
        }
        self.touched.shrink_to_fit();
        self.unigram_changes.sort_unstable();
        self.unigram_changes.dedup();
        self.unigram_changes
            .retain(|slot| self.touched.binary_search(slot).is_ok());
        self.unigram_changes.shrink_to_fit();
        let total = self.total;
        let retained = (!self.touched.is_empty()).then(|| {
            let slots = self
                .touched
                .iter()
                .map(|&slot| (slot, self.slot_offset(slot)))
                .collect();
            ReplayedLibrary {
                content: self.content,
                slots,
                unigram_changes: self.unigram_changes,
                has_added_items: self.has_added_items,
            }
        });
        (total, typed, retained)
    }
}

#[cfg(test)]
impl<'l> Merge<'_, 'l> {
    /// Apply a record whose buffers a test owns: they are leaked so the undo
    /// entries can borrow them for the length of the merge.
    pub(crate) fn apply_owned(&mut self, record: LogRecord<Vec<u8>>) -> Result<(), Stop> {
        let leak = |bytes: Vec<u8>| -> &'l [u8] { Box::leak(bytes.into_boxed_slice()) };
        self.apply(match record {
            LogRecord::Add { token, new_item } => LogRecord::Add {
                token,
                new_item: leak(new_item),
            },
            LogRecord::Remove { token, old_item } => LogRecord::Remove {
                token,
                old_item: leak(old_item),
            },
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => LogRecord::Modify {
                token,
                old_item: leak(old_item),
                new_item: leak(new_item),
            },
            LogRecord::ModifyHeader {
                old_total,
                new_total,
            } => LogRecord::ModifyHeader {
                old_total,
                new_total,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(phrase: &[u32], unigram: u32, prons: &[(Vec<u16>, u32)]) -> ChunkItem {
        ChunkItem {
            phrase: phrase.to_vec(),
            unigram,
            prons: prons.to_vec(),
        }
    }

    #[test]
    fn unigram_candidates_exclude_reading_only_edits_and_deduplicate_changes() {
        let original = library();
        let mut changed = two();
        changed[2..6].copy_from_slice(&70_u32.to_le_bytes());
        let mut restored = changed.clone();
        restored[2..6].copy_from_slice(&50_u32.to_le_bytes());
        let mut merge = Merge::new(Some(original.view()));
        for record in [
            modify(1, one(), with_freq(one(), 77)),
            modify(2, two(), changed.clone()),
            modify(2, changed, restored.clone()),
            modify(2, restored, with_freq(two(), 88)),
        ] {
            assert!(merge.apply_owned(record).is_ok());
        }
        let mut skipped = Vec::new();
        let (_, _, view) = merge.finish_lazy(&mut skipped, &"test");
        let view = view.expect("retained originals");
        assert!(skipped.is_empty());
        assert_eq!(view.slots().collect::<Vec<_>>(), [1, 2]);
        assert_eq!(view.unigram_slots().collect::<Vec<_>>(), [2]);
        assert_eq!(view.item(2).expect("restored item").unigram, 50);
    }

    #[test]
    fn add_candidates_skip_modify_only_and_original_slot_add_records() {
        let original = library();
        for replacement in [false, true] {
            let mut merge = Merge::new(Some(original.view()));
            let changed = with_freq(one(), 77);
            let record = if replacement {
                LogRecord::Add {
                    token: 1,
                    new_item: changed,
                }
            } else {
                modify(1, one(), changed)
            };
            assert!(merge.apply_owned(record).is_ok());
            let mut skipped = Vec::new();
            let (_, _, view) = merge.finish_views(&mut skipped, &"test");
            let view = view.expect("retained original");
            assert!(skipped.is_empty());
            assert_eq!(view.slots().collect::<Vec<_>>(), [1]);
            assert!(view.add_candidates().next().is_none());
            assert_eq!(view.item(1).unwrap().prons[0].1, 77);
        }
    }

    #[test]
    fn add_candidates_preserve_mixed_sparse_and_overreach_views() {
        let original = library();
        for overreach in [false, true] {
            let mut merge = if overreach {
                over_length_merge(&original)
            } else {
                Merge::new(Some(original.view()))
            };
            if !overreach {
                assert!(
                    merge
                        .apply_owned(modify(1, one(), with_freq(one(), 77)))
                        .is_ok()
                );
            }
            for record in [
                LogRecord::Add {
                    token: 1024,
                    new_item: one(),
                },
                LogRecord::Add {
                    token: 2048,
                    new_item: one(),
                },
                LogRecord::Remove {
                    token: 2048,
                    old_item: one(),
                },
            ] {
                assert!(merge.apply_owned(record).is_ok());
            }
            let mut skipped = Vec::new();
            let (_, _, view) = merge.finish_views(&mut skipped, &"test");
            let view = view.expect("retained originals and addition");
            assert!(skipped.is_empty());
            let expected: Vec<_> = view
                .slots()
                .filter(|&slot| !original.contains_item(slot))
                .collect();
            let actual: Vec<_> = view
                .add_candidates()
                .filter(|&slot| !original.contains_item(slot))
                .collect();
            assert_eq!(expected, [1024]);
            assert_eq!(actual, expected);
            assert_eq!(view.item(1024), decode_phrase_item(&one()).ok());
        }
    }

    #[test]
    fn add_candidates_ignore_rejected_additions_and_original_overreach() {
        let original = library();
        for rejected_add in [false, true] {
            let mut merge = over_length_merge(&original);
            if rejected_add {
                assert!(
                    merge
                        .apply_owned(LogRecord::Add {
                            token: 1024,
                            new_item: bytes_with(&[255, 255], 1),
                        })
                        .is_ok()
                );
            }
            let (_, _, view) = merge.finish_views(&mut Vec::new(), &"test");
            let view = view.expect("retained overreach originals");
            assert_eq!(view.slots().collect::<Vec<_>>(), [1, 2]);
            assert!(view.add_candidates().next().is_none());
        }
    }

    #[test]
    fn a_stale_unigram_write_marks_its_storage_slot_for_seeding() {
        let original = library();
        let reading_only = with_freq(one(), 77);
        let mut changed = reading_only.clone();
        changed[2..6].copy_from_slice(&150_u32.to_le_bytes());
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(1, one(), reading_only.clone()))
                .is_ok()
        );
        assert!(merge.apply_owned(modify(9, reading_only, changed)).is_ok());
        let (_, _, view) = merge.finish_lazy(&mut Vec::new(), &"test");
        let view = view.expect("retained original");
        assert_eq!(view.slots().collect::<Vec<_>>(), [1]);
        assert_eq!(view.unigram_slots().collect::<Vec<_>>(), [1]);
        assert_eq!(view.item(1).expect("stale storage").unigram, 150);
    }

    #[test]
    fn a_cross_item_write_marks_the_other_retained_unigram() {
        let original = library();
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(2, two(), with_freq(two(), 88)))
                .is_ok()
        );
        // A longer header in a short payload borrows the next item's bytes,
        // just as the existing cross-item replay cases require.
        let mut expanded = one();
        expanded[1] = 3;
        assert!(merge.apply_owned(modify(1, one(), expanded)).is_ok());
        let offset = merge.slot_offset(1) as usize;
        let Found::Item { len, .. } = merge.content.header(offset) else {
            panic!("expanded item");
        };
        let old = merge
            .content
            .item(offset, len)
            .expect("cross-item bytes")
            .into_owned();
        let mut new = old.clone();
        let next_unigram = merge.slot_offset(2) as usize - offset + 2;
        new[next_unigram..next_unigram + 4].copy_from_slice(&170_u32.to_le_bytes());
        // Slot 1's own unigram is unchanged; this write changes slot 2's.
        assert!(merge.apply_owned(modify(1, old, new)).is_ok());
        let mut skipped = Vec::new();
        let (_, _, view) = merge.finish_lazy(&mut skipped, &"test");
        let view = view.expect("retained originals");
        assert!(skipped.is_empty());
        assert_eq!(view.item(2).expect("overlapped item").unigram, 170);
        assert_eq!(view.unigram_slots().collect::<Vec<_>>(), [2]);
    }

    /// The over-length item's raw pronunciation words, built once: A's
    /// second character is A's first pronunciation key plus its frequency's
    /// low half, and both readings borrow B's bytes.
    fn over_length_merge(original: &SystemLibrary) -> Merge<'_, '_> {
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(1, one(), bytes_with(&[2, 2], 200)))
                .is_ok()
        );
        let offset = merge.slot_offset(1) as usize;
        let Found::Item { len, .. } = merge.content.header(offset) else {
            panic!("expanded item");
        };
        let old = merge
            .content
            .item(offset, len)
            .expect("cross-item bytes")
            .into_owned();
        let mut new = old.clone();
        let at = merge.slot_offset(2) as usize - offset + 2;
        new[at..at + 4].copy_from_slice(&170_u32.to_le_bytes());
        assert!(merge.apply_owned(modify(1, old, new)).is_ok());
        merge
    }

    #[test]
    fn an_unrecorded_overlapped_slot_is_visible_and_seeded() {
        let original = library();
        let merge = over_length_merge(&original);
        let (_, _, view) = merge.finish_lazy(&mut Vec::new(), &"test");
        let view = view.expect("retained originals");
        assert_eq!(view.slots().collect::<Vec<_>>(), [1, 2]);
        assert!(view.contains(2));
        assert_eq!(view.item(1).expect("over-length item").unigram, 200);
        assert_eq!(view.item(2).expect("overlapped item").unigram, 170);
        assert_eq!(view.unigram_slots().collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn an_over_length_item_reports_the_borrowed_keys() {
        let original = library();
        let merge = over_length_merge(&original);
        let (_, _, view) = merge.finish_lazy(&mut Vec::new(), &"test");
        let view = view.expect("retained originals");
        let item = view.item(1).expect("over-length item");
        assert_eq!(item.prons[0].0, [0x0000, 0x0201]);
        assert_eq!(item.prons[1].0, [0x597d, 0x0000]);
        assert_eq!(item.prons[0].1, 170);
    }

    #[test]
    fn an_in_place_write_within_the_original_item_adds_no_slot() {
        let original = library();
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(1, one(), with_freq(one(), 77)))
                .is_ok()
        );
        assert_eq!(merge.touched, vec![1]);
        let (_, _, view) = merge.finish_lazy(&mut Vec::new(), &"test");
        let view = view.expect("retained original");
        assert_eq!(view.slots().collect::<Vec<_>>(), [1]);
        assert!(view.unigram_slots().next().is_none());
    }

    #[test]
    fn rejected_unigram_candidates_do_not_seed_the_baseline_fallback() {
        let original = library();
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(2, two(), with_freq(two(), 88)))
                .is_ok()
        );
        assert!(
            merge
                .apply_owned(modify(1, one(), bytes_with(&[255, 255], 200)))
                .is_ok()
        );
        let mut skipped = Vec::new();
        let (_, _, view) = merge.finish_lazy(&mut skipped, &"test");
        let view = view.expect("valid original remains");
        assert_eq!(skipped.len(), 1);
        assert_eq!(view.slots().collect::<Vec<_>>(), [2]);
        assert!(view.unigram_slots().next().is_none());
        assert!(!view.contains(1));
        assert!(view.item(1).is_none());
    }

    /// Slot 1 holds a one-reading item (unigram 100), slot 2 a two-reading one
    /// (unigram 50): the library's total is their sum.
    fn library() -> SystemLibrary {
        SystemLibrary {
            total: 150,
            range_end: 3,
            items: BTreeMap::from([
                (1, item(&[0x4f60], 100, &[(vec![1], 100)])),
                (2, item(&[0x597d], 50, &[(vec![2], 30), (vec![3], 20)])),
            ]),
            mapped: None,
        }
    }

    fn one() -> Vec<u8> {
        journal_item_bytes(&library().items[&1])
    }

    fn two() -> Vec<u8> {
        journal_item_bytes(&library().items[&2])
    }

    fn with_last(mut item: Vec<u8>, byte: u8) -> Vec<u8> {
        *item.last_mut().expect("item") = byte;
        item
    }

    /// A six-byte item: the two header bytes and a unigram.
    fn bytes_with(header: &[u8; 2], unigram: u32) -> Vec<u8> {
        let mut bytes = header.to_vec();
        bytes.extend_from_slice(&unigram.to_le_bytes());
        bytes
    }

    #[test]
    fn an_overlong_header_past_the_area_keeps_the_last_changed_item() {
        let original = library();
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(1, one(), bytes_with(&[2, 3], 200)))
                .is_ok()
        );
        merge.content.append(&[0; 8]);
        let mut changed = two();
        changed[2..6].copy_from_slice(&170_u32.to_le_bytes());
        assert!(merge.apply_owned(modify(2, two(), changed)).is_ok());
        let mut skipped = Vec::new();
        let (_, _, retained) = merge.finish_lazy(&mut skipped, &"test");
        let retained = retained.expect("the over-length item stays");
        assert!(retained.contains(2));
        assert_eq!(retained.item(2).expect("two").unigram, 170);
    }

    #[test]
    fn an_overlapped_rejected_slot_is_reported_once() {
        let original = library();
        let invalid = bytes_with(&[255, 255], 200);
        let replay = || {
            let mut merge = Merge::new(Some(original.view()));
            assert!(
                merge
                    .apply_owned(modify(1, one(), bytes_with(&[2, 1], 200)))
                    .is_ok()
            );
            assert!(merge.apply_owned(modify(2, two(), invalid.clone())).is_ok());
            merge
        };
        let mut eager_skipped = Vec::new();
        let (_, eager) = replay().finish(&mut eager_skipped, &"test");
        let mut lazy_skipped = Vec::new();
        let (_, lazy, retained) = replay().finish_lazy(&mut lazy_skipped, &"test");
        assert_eq!(lazy_skipped, eager_skipped);
        assert_eq!(
            lazy_skipped
                .iter()
                .filter(|message| message.contains("item 2"))
                .count(),
            1
        );
        assert_eq!(eager.get(&2), Some(&None));
        assert_eq!(lazy.get(&2), Some(&None));
        assert!(retained.is_some());
    }

    #[test]
    fn overreach_tracks_only_a_growing_header() {
        let original = library();

        let mut merge = Merge::new(Some(original.view()));
        let same = with_last(one(), 0x7f);
        let old = one();
        assert!(merge.modify(1, &old, &same).is_ok());
        assert!(!merge.overreach);

        let mut merge = Merge::new(Some(original.view()));
        let grown = bytes_with(&[3, 1], 200);
        let old = one();
        assert!(merge.modify(1, &old, &grown).is_ok());
        assert!(merge.overreach);

        let mut merge = Merge::new(Some(original.view()));
        let shorter = bytes_with(&[0, 0], 0);
        let old = one();
        assert!(merge.modify(1, &old, &shorter).is_ok());
        assert!(!merge.overreach);
    }

    fn with_freq(mut item: Vec<u8>, frequency: u32) -> Vec<u8> {
        let end = item.len();
        item[end - 4..].copy_from_slice(&frequency.to_le_bytes());
        item
    }

    #[test]
    fn touched_slots_keep_final_values_after_duplicate_sparse_and_relocated_edits() {
        let original = library();
        let high = PHRASE_MASK - 1;
        let mut grown = with_freq(two(), 88);
        grown.extend_from_slice(&[1, 2, 3]);
        let replay = || {
            let mut merge = Merge::new(Some(original.view()));
            for record in [
                modify(2, two(), grown.clone()),
                modify(1, one(), with_freq(one(), 77)),
                modify(2, with_freq(two(), 88), with_freq(two(), 99)),
                LogRecord::Remove {
                    token: 1,
                    old_item: with_freq(one(), 77),
                },
                LogRecord::Add {
                    token: 1,
                    new_item: with_freq(one(), 66),
                },
                LogRecord::Add {
                    token: high,
                    new_item: one(),
                },
                LogRecord::Remove {
                    token: high,
                    old_item: one(),
                },
                LogRecord::Add {
                    token: high,
                    new_item: with_freq(one(), 55),
                },
            ] {
                assert!(merge.apply_owned(record).is_ok());
            }
            merge
        };
        let mut eager_skipped = Vec::new();
        let (eager_total, eager) = replay().finish(&mut eager_skipped, &"test");
        let mut lazy_skipped = Vec::new();
        let (lazy_total, lazy, retained) = replay().finish_lazy(&mut lazy_skipped, &"test");
        let retained = retained.expect("the two original slots stay lazy");
        assert_eq!(eager_total, Some(250));
        assert_eq!(lazy_total, eager_total);
        assert!(eager_skipped.is_empty());
        assert_eq!(lazy_skipped, eager_skipped);
        assert_eq!(eager.keys().copied().collect::<Vec<_>>(), [1, 2, high]);
        assert_eq!(retained.slots().collect::<Vec<_>>(), [1, 2]);
        assert_eq!(retained.slots().rev().collect::<Vec<_>>(), [2, 1]);
        assert!(retained.contains(1));
        assert!(retained.contains(2));
        assert!(!retained.contains(high));
        assert_eq!(retained.item(1), eager[&1]);
        assert_eq!(retained.item(2), eager[&2]);
        assert_eq!(lazy[&high], eager[&high]);
        assert_eq!(eager[&1].as_ref().expect("one").prons[0].1, 66);
        assert_eq!(eager[&2].as_ref().expect("two").prons[1].1, 99);
        assert_eq!(eager[&high].as_ref().expect("high").prons[0].1, 55);
    }

    #[test]
    fn duplicate_invalid_slots_report_once_and_keep_the_baseline() {
        let original = library();
        let invalid = bytes_with(&[255, 255], 200);
        let expected = format!("test: {}", decode_phrase_item(&invalid).unwrap_err());
        let replay = || {
            let mut merge = Merge::new(Some(original.view()));
            for record in [
                modify(2, two(), with_freq(two(), 88)),
                modify(1, one(), with_freq(one(), 77)),
                modify(2, with_freq(two(), 88), with_freq(two(), 99)),
                modify(1, with_freq(one(), 77), with_freq(one(), 66)),
            ] {
                assert!(merge.apply_owned(record).is_ok());
            }
            let offset = merge.slot_offset(1) as usize;
            merge.content.item_override = Some((offset, invalid.clone()));
            merge
        };
        let mut eager_skipped = Vec::new();
        let (_, eager) = replay().finish(&mut eager_skipped, &"test");
        let mut lazy_skipped = Vec::new();
        let (_, lazy, retained) = replay().finish_lazy(&mut lazy_skipped, &"test");
        let retained = retained.expect("the valid original slot keeps the view");
        assert_eq!(eager_skipped, [expected]);
        assert_eq!(lazy_skipped, eager_skipped);
        assert!(!eager.contains_key(&1));
        assert!(!lazy.contains_key(&1));
        assert_eq!(retained.slots().collect::<Vec<_>>(), [2]);
        assert!(!retained.contains(1));
        assert!(retained.item(1).is_none());
        assert_eq!(retained.item(2), eager[&2]);
        assert_eq!(
            retained.item(1).or_else(|| original.item(1)),
            original.item(1)
        );
    }

    #[test]
    fn many_invalid_slots_are_filtered_once_in_ascending_order() {
        let invalid_count = 256;
        let valid_slot = invalid_count + 1;
        let baseline = library().items[&1].clone();
        let original = SystemLibrary {
            total: valid_slot * 100,
            range_end: valid_slot + 1,
            items: (1..=valid_slot)
                .map(|slot| (slot, baseline.clone()))
                .collect(),
            mapped: None,
        };
        let invalid = bytes_with(&[255, 255], 200);
        let replay = || {
            let mut merge = Merge::new(Some(original.view()));
            assert!(
                merge
                    .apply_owned(modify(valid_slot, one(), with_freq(one(), 88)))
                    .is_ok()
            );
            for slot in (1..=invalid_count).rev() {
                assert!(
                    merge
                        .apply_owned(modify(slot, one(), with_freq(one(), 77)))
                        .is_ok()
                );
                assert!(
                    merge
                        .apply_owned(modify(slot, with_freq(one(), 77), invalid.clone()))
                        .is_ok()
                );
            }
            merge
        };
        let mut eager_skipped = Vec::new();
        let (_, eager) = replay().finish(&mut eager_skipped, &"test");
        let mut lazy_skipped = Vec::new();
        let (_, lazy, retained) = replay().finish_lazy(&mut lazy_skipped, &"test");
        let retained = retained.expect("one valid original remains");
        let expected: Vec<_> = (1..=invalid_count)
            .map(|slot| format!("test: item {slot} ends past the phrase content"))
            .collect();
        assert_eq!(eager_skipped, expected);
        assert_eq!(lazy_skipped, expected);
        for slot in 1..=invalid_count {
            assert_eq!(eager.get(&slot), Some(&None));
            assert_eq!(lazy.get(&slot), Some(&None));
        }
        assert_eq!(eager.len(), invalid_count as usize + 1);
        assert_eq!(lazy.len(), invalid_count as usize);
        assert_eq!(retained.slots().collect::<Vec<_>>(), [valid_slot]);
        assert_eq!(retained.item(valid_slot), eager[&valid_slot]);
        for slot in 1..=invalid_count {
            assert!(!retained.contains(slot));
            assert!(retained.item(slot).is_none());
        }
    }

    #[test]
    fn eager_and_lazy_finish_reject_invalid_payloads_identically() {
        let original = library();
        // The replacement header claims more bytes than the content holds.
        // A second, valid change keeps a lazy view alive beside the rejected
        // slot, so presence and reads must both fall back to the baseline.
        let invalid = bytes_with(&[255, 255], 200);
        assert!(decode_phrase_item(&invalid).is_err());
        let replay = || {
            let mut merge = Merge::new(Some(original.view()));
            assert!(merge.apply_owned(modify(1, one(), invalid.clone())).is_ok());
            assert!(
                merge
                    .apply_owned(modify(2, two(), with_freq(two(), 88)))
                    .is_ok()
            );
            merge
        };
        let mut eager_skipped = Vec::new();
        let (eager_total, eager_items) = replay().finish(&mut eager_skipped, &"test");
        let mut lazy_skipped = Vec::new();
        let (lazy_total, lazy_items, retained) = replay().finish_lazy(&mut lazy_skipped, &"test");
        let retained = retained.expect("the valid slot stays lazy");
        assert_eq!(lazy_total, eager_total);
        assert_eq!(lazy_skipped, eager_skipped);
        assert_eq!(lazy_skipped, ["test: item 1 ends past the phrase content"]);
        // Row 107, option 1: the item answers no item, and the other record's
        // change stays.
        assert_eq!(eager_items.get(&1), Some(&None));
        assert_eq!(lazy_items.get(&1), Some(&None));
        assert!(!retained.contains(1));
        assert!(retained.item(1).is_none());
        assert_eq!(retained.item(2), eager_items[&2]);
    }

    #[test]
    fn lazy_original_decode_failure_drops_touched_and_keeps_baseline() {
        let original = library();
        let invalid = bytes_with(&[255, 255], 200);
        let expected = format!("test: {}", decode_phrase_item(&invalid).unwrap_err());
        let replay = || {
            let mut merge = Merge::new(Some(original.view()));
            assert!(
                merge
                    .apply_owned(modify(1, one(), with_freq(one(), 77)))
                    .is_ok()
            );
            assert!(
                merge
                    .apply_owned(modify(2, two(), with_freq(two(), 88)))
                    .is_ok()
            );
            assert!(merge.touched.contains(&1));
            let offset = merge.slot_offset(1) as usize;
            assert!(matches!(merge.content.header(offset), Found::Item { .. }));
            // A structural error normally fails the header bounds check first.
            // Inject bad bytes at the decoder boundary to exercise the original
            // slot's defensive validation, with a valid slot keeping the view.
            merge.content.item_override = Some((offset, invalid.clone()));
            merge
        };
        let mut eager_skipped = Vec::new();
        let (eager_total, eager_items) = replay().finish(&mut eager_skipped, &"test");
        let mut lazy_skipped = Vec::new();
        let (lazy_total, lazy_items, retained) = replay().finish_lazy(&mut lazy_skipped, &"test");
        let retained = retained.expect("the valid original slot keeps the view");
        assert_eq!(lazy_total, eager_total);
        assert_eq!(lazy_skipped, eager_skipped);
        assert_eq!(lazy_skipped, [expected]);
        assert!(!eager_items.contains_key(&1));
        assert!(!lazy_items.contains_key(&1));
        assert!(!retained.slots.iter().any(|&(slot, _)| slot == 1));
        assert!(!retained.contains(1));
        assert!(retained.item(1).is_none());
        assert_eq!(retained.item(2), eager_items[&2]);

        let token = (1 << 24) | 1;
        let live =
            crate::system_items::SystemItems::new(BTreeMap::new(), BTreeMap::from([(1, retained)]));
        assert!(live.get(&token).is_none());
        let eager_visible = eager_items
            .get(&1)
            .cloned()
            .unwrap_or_else(|| original.item(1));
        let lazy_visible = live
            .get(&token)
            .map(std::borrow::Cow::into_owned)
            .or_else(|| original.item(1));
        assert_eq!(lazy_visible, eager_visible);
        assert_eq!(lazy_visible, original.item(1));
    }

    /// What a merge leaves: how it ended, the total and the overrides.
    type Merged = (
        Result<(), Stop>,
        Option<u32>,
        BTreeMap<u32, Option<ChunkItem>>,
    );

    fn merge_of(library: &SystemLibrary, records: Vec<LogRecord>) -> Merged {
        let mut merge = Merge::new(Some(library.view()));
        let mut outcome = Ok(());
        for record in records {
            if let Err(stop) = merge.apply_owned(record) {
                outcome = Err(stop);
                break;
            }
        }
        let (total, overrides) = merge.finish(&mut Vec::new(), &"test");
        (outcome, total, overrides)
    }

    fn modify(token: u32, old_item: Vec<u8>, new_item: Vec<u8>) -> LogRecord {
        LogRecord::Modify {
            token,
            old_item,
            new_item,
        }
    }

    #[test]
    fn a_failed_remove_keeps_the_removal_and_stops() {
        // Same size, other bytes: the item and its unigram are gone already.
        let records = vec![
            LogRecord::Remove {
                token: 1,
                old_item: with_last(one(), 0xff),
            },
            LogRecord::Add {
                token: 5,
                new_item: one(),
            },
        ];
        let (outcome, total, overrides) = merge_of(&library(), records);
        assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
        assert_eq!(total, Some(50));
        assert_eq!(overrides, BTreeMap::from([(1, None)]));
    }

    #[test]
    fn a_trailing_byte_makes_an_old_chunk_differ() {
        let mut trailing = one();
        trailing.push(0);
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![LogRecord::Remove {
                token: 1,
                old_item: trailing.clone(),
            }],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
        assert_eq!((total, overrides), (Some(50), BTreeMap::from([(1, None)])));
        // MODIFY compares first: nothing is changed.
        let (outcome, total, overrides) =
            merge_of(&library(), vec![modify(1, trailing, with_last(one(), 9))]);
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
        assert_eq!((total, overrides), (Some(150), BTreeMap::new()));
    }

    #[test]
    fn a_grown_item_is_stored_whole_but_is_as_long_as_its_header() {
        let mut grown = with_freq(one(), 5);
        grown.extend_from_slice(&[1, 2, 3]);
        let visible = grown[..grown.len() - 3].to_vec();
        // The next record's old chunk is the header-length bytes.
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(1, one(), grown.clone()),
                modify(1, visible.clone(), with_freq(visible.clone(), 7)),
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(150));
        assert_eq!(
            overrides[&1].as_ref().expect("item").prons,
            vec![(vec![1], 7)]
        );
        // With the stored trailing bytes it is another size: a mismatch.
        let (outcome, _, _) = merge_of(
            &library(),
            vec![
                modify(1, one(), grown.clone()),
                modify(1, grown, with_freq(visible, 7)),
            ],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
    }

    #[test]
    fn an_in_place_chunk_with_a_shorter_header_is_what_the_next_record_sees() {
        // Slot 2 has two readings; the new chunk has one and a tail that fits.
        let mut shorter = two()[..two().len() - 6].to_vec();
        shorter[1] = 1;
        let mut chunk = shorter.clone();
        chunk.extend_from_slice(&[9, 9, 9, 9]);
        assert!(chunk.len() <= two().len());
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(2, two(), chunk),
                modify(2, shorter.clone(), with_freq(shorter, 8)),
            ],
        );
        assert!(outcome.is_ok());
        // In place leaves the total alone.
        assert_eq!(total, Some(150));
        assert_eq!(
            overrides[&2].as_ref().expect("item").prons,
            vec![(vec![2], 8)]
        );
    }

    #[test]
    fn a_stale_item_takes_a_same_size_modify_of_an_absent_token() {
        // MODIFY of slot 2 leaves `item` on its storage; a MODIFY of a token
        // the library does not hold, whose old chunk equals it, is written
        // over that storage and the merge goes on.
        let first = with_freq(two(), 21);
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(2, two(), first.clone()),
                modify(9, first.clone(), with_freq(two(), 22)),
                LogRecord::Add {
                    token: 7,
                    new_item: one(),
                },
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(250));
        assert_eq!(
            overrides[&2].as_ref().expect("item").prons,
            vec![(vec![2], 30), (vec![3], 22)]
        );
        assert!(!overrides.contains_key(&9));
        assert!(overrides.contains_key(&7));
        // A longer new chunk would be the pin's NULL dereference: the merge ends.
        let mut longer = with_freq(two(), 22);
        longer.push(0);
        let (outcome, _, _) = merge_of(
            &library(),
            vec![modify(2, two(), first.clone()), modify(9, first, longer)],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
        // With no earlier fetch the stale item is the six zero bytes of the
        // pin's default `PhraseItem`: a two-character old chunk differs.
        let (outcome, _, _) = merge_of(&library(), vec![modify(9, two(), two())]);
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
    }

    #[test]
    fn the_default_stale_item_is_six_zero_bytes() {
        // An absent token twice: the first MODIFY compares against the six
        // zero bytes, writes its six-byte new chunk into that buffer and
        // creates no item; the second compares against what the first wrote.
        let first = bytes_with(&[0, 0], 50);
        let second = bytes_with(&[0, 0], 60);
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(9, vec![0; 6], first.clone()),
                modify(9, first, second),
                LogRecord::Add {
                    token: 7,
                    new_item: one(),
                },
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(250));
        assert!(!overrides.contains_key(&9), "no item is created");
        assert!(overrides.contains_key(&7), "the merge went on");
        // A non-zero old chunk differs from the default.
        let (outcome, _, overrides) = merge_of(
            &library(),
            vec![modify(9, vec![1; 6], bytes_with(&[0, 0], 50))],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 9))));
        assert!(overrides.is_empty());
    }

    #[test]
    fn a_short_new_chunk_is_written_in_place_without_its_own_header_check() {
        // The compare is on the old chunk: a six-byte new chunk for a
        // sixteen-byte item is `memmove`d over its start, the other ten bytes
        // and the total stay.
        let short = bytes_with(&[1, 1], 200);
        assert_eq!(short.len(), 6);
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(1, one(), short),
                LogRecord::Add {
                    token: 5,
                    new_item: one(),
                },
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(250));
        let item = overrides[&1].as_ref().expect("item");
        assert_eq!(item.unigram, 200);
        assert_eq!(item.phrase, vec![0x4f60]);
        assert_eq!(item.prons, vec![(vec![1], 100)]);
    }

    #[test]
    fn a_small_overlong_header_reads_the_following_payload() {
        // Six bytes whose header names twenty-two: the in-place write keeps
        // the item's stored sixteen bytes and changes its header. The next
        // read takes the six bytes that follow, inside the content buffer.
        let overlong = bytes_with(&[2, 1], 200);
        let visible = {
            let mut visible = overlong.clone();
            visible.extend_from_slice(&one()[6..]);
            visible.extend_from_slice(&two()[..6]);
            visible
        };
        assert_eq!(visible.len(), 22);
        let (outcome, total, overrides) = merge_of(&library(), vec![modify(1, one(), overlong)]);
        assert!(outcome.is_ok());
        assert_eq!(total, Some(150));
        let item = overrides[&1].as_ref().expect("item");
        assert_eq!(item.phrase.len(), 2);
        assert_eq!(item.unigram, 200);
        // The header-length bytes are what a later compare sees.
        let (outcome, _, overrides) = merge_of(
            &library(),
            vec![
                modify(1, one(), bytes_with(&[2, 1], 200)),
                modify(1, visible.clone(), with_freq(visible, 7)),
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(overrides[&1].as_ref().expect("item").prons[0].1, 7);
        // The stored sixteen bytes are no longer the item.
        let (outcome, _, _) = merge_of(
            &library(),
            vec![
                modify(1, one(), bytes_with(&[2, 1], 200)),
                modify(1, one(), with_freq(one(), 9)),
            ],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
    }

    #[test]
    fn a_stored_header_past_the_buffer_stops_a_later_read() {
        // A header naming 132,096 bytes ends past the content buffer; the pin
        // reads past its allocation (row 107): the item answers no item.
        let overlong = bytes_with(&[255, 255], 200);
        let (outcome, total, overrides) =
            merge_of(&library(), vec![modify(1, one(), overlong.clone())]);
        assert!(outcome.is_ok());
        assert_eq!(total, Some(150));
        assert_eq!(
            overrides.get(&1),
            Some(&None),
            "the over-read item answers no item"
        );
        let (outcome, _, _) = merge_of(
            &library(),
            vec![
                modify(1, one(), overlong.clone()),
                modify(1, one(), with_freq(one(), 9)),
            ],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 1))));
        let (outcome, _, _) = merge_of(
            &library(),
            vec![
                modify(1, one(), overlong),
                LogRecord::Remove {
                    token: 1,
                    old_item: one(),
                },
            ],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 1))));
    }

    #[test]
    fn an_add_payload_shorter_than_its_header_is_stored_and_stops_a_later_read() {
        // `add_phrase_item` stores the payload without reading its header.
        // A later read of it ends past the buffer (row 107) and stops.
        let payload = bytes_with(&[1, 1], 7);
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![LogRecord::Add {
                token: 5,
                new_item: payload.clone(),
            }],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(157));
        assert_eq!(
            overrides.get(&5),
            Some(&None),
            "the over-read item answers no item"
        );
        let (outcome, _, _) = merge_of(
            &library(),
            vec![
                LogRecord::Add {
                    token: 5,
                    new_item: payload,
                },
                LogRecord::Remove {
                    token: 5,
                    old_item: one(),
                },
            ],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("REMOVE", 5))));
    }

    #[test]
    fn a_short_add_is_reported_after_the_records_before_it() {
        // A payload shorter than the unigram the ADD reads aborts the pin
        // (`memory_chunk.h:390`, row 108). The merge reports the record by
        // its raw token after applying the earlier MODIFY; the caller returns
        // the fault, so nothing after it applies.
        let records = vec![
            modify(1, one(), with_freq(one(), 77)),
            LogRecord::Add {
                token: 5,
                new_item: vec![1],
            },
            LogRecord::Add {
                token: 5,
                new_item: one(),
            },
        ];
        let (outcome, total, overrides) = merge_of(&library(), records);
        assert!(matches!(outcome, Err(Stop::ShortAdd(5))));
        assert_eq!(total, Some(150));
        assert_eq!(overrides[&1].as_ref().expect("item").prons[0].1, 77);
        assert!(!overrides.contains_key(&5));
    }

    #[test]
    fn an_empty_phrase_is_an_item_of_its_header_length() {
        let empty = vec![0, 0, 3, 0, 0, 0];
        let (outcome, total, overrides) = merge_of(
            &library(),
            vec![
                modify(1, one(), empty.clone()),
                modify(1, empty, vec![0, 0, 4, 0, 0, 0]),
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(total, Some(150));
        assert_eq!(overrides[&1], Some(item(&[], 4, &[])));
    }

    /// The buffer a fresh merge over `library` starts from.
    fn pristine(library: &SystemLibrary) -> Content {
        Merge::new(Some(library.view())).content
    }

    /// Whether `a` holds exactly the bytes, sizes and capacities `b` does, with
    /// no grown copy of either chunk.
    fn same_buffer(a: &Content, b: &Content) -> bool {
        a.items.owned.is_none()
            && a.slots.owned.is_none()
            && a.items.data_end == b.items.data_end
            && a.items.capacity == b.items.capacity
            && a.slots.data_end == b.slots.data_end
            && a.slots.capacity == b.slots.capacity
            && a.backing.bytes(Area::Content) == b.backing.bytes(Area::Content)
            && a.backing.bytes(Area::Offsets) == b.backing.bytes(Area::Offsets)
    }

    /// Applies `records`, checks the buffer changed, rolls back and checks it
    /// is the pristine buffer again, bytes and offsets.
    fn assert_rollback_restores(library: &SystemLibrary, records: Vec<LogRecord>) {
        let mut merge = Merge::new(Some(library.view()));
        for record in records {
            assert!(merge.apply_owned(record).is_ok());
        }
        assert!(
            !same_buffer(&merge.content, &pristine(library)),
            "the replay changed nothing, so the check below proves nothing"
        );
        let rolled = merge.rollback();
        assert!(same_buffer(&rolled.content, &pristine(library)));
        assert_eq!(rolled.slots().count(), 0);
    }

    fn rollback_cases() -> Vec<Vec<LogRecord>> {
        vec![
            // Repeated MODIFY of one token.
            vec![
                modify(1, one(), with_freq(one(), 77)),
                modify(1, with_freq(one(), 77), with_freq(one(), 66)),
            ],
            // A longer MODIFY: a remove and an append, so the offset word is
            // overwritten and the content grows.
            vec![modify(1, one(), two())],
            // REMOVE, then ADD at the same slot.
            vec![
                LogRecord::Remove {
                    token: 1,
                    old_item: one(),
                },
                LogRecord::Add {
                    token: 1,
                    new_item: with_freq(one(), 5),
                },
            ],
            // A stale-item write: the absent token compares against the item
            // the previous MODIFY read and writes over it.
            vec![
                modify(1, one(), with_freq(one(), 77)),
                modify(9, with_freq(one(), 77), with_freq(one(), 66)),
            ],
            // Overlapping writes: the second is a prefix of the first.
            vec![
                modify(1, one(), bytes_with(&[0, 0], 0)),
                modify(1, bytes_with(&[0, 0], 0), bytes_with(&[0, 0], 9)),
            ],
            // An in-place write, then growth of the offset array and the
            // content (an ADD far past both), then a write into the grown copy.
            vec![
                modify(1, one(), with_freq(one(), 77)),
                LogRecord::Add {
                    token: 5000,
                    new_item: one(),
                },
                modify(2, two(), with_freq(two(), 88)),
            ],
        ]
    }

    #[test]
    fn rollback_restores_the_buffer_for_every_kind_of_mutation() {
        let original = library();
        for records in rollback_cases() {
            assert_rollback_restores(&original, records);
        }
    }

    #[test]
    fn rollback_restores_a_mapped_buffer() {
        let items = vec![
            (1, item(&[0x4f60], 100, &[(vec![1], 100)])),
            (2, item(&[0x597d], 50, &[(vec![2], 30), (vec![3], 20)])),
        ];
        let chunk = oxpinyin_data::chunk_write::build_chunk(&items).expect("chunk");
        let dir = std::env::temp_dir().join(format!("oxpinyin-rollback-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("rollback.bin");
        std::fs::write(&path, chunk).expect("write");
        let mapped = oxpinyin_data::phrase_library::PhraseLibrary::open(&path).expect("open");
        std::fs::remove_dir_all(&dir).expect("cleanup");
        let library = SystemLibrary {
            total: mapped.total_freq(),
            range_end: mapped.token_range().end,
            items: BTreeMap::new(),
            mapped: Some(mapped),
        };
        assert!(matches!(
            Merge::new(Some(library.view())).content.backing,
            Backing::Mapped(_)
        ));
        for records in rollback_cases() {
            assert_rollback_restores(&library, records);
        }
    }

    #[test]
    fn a_mismatch_stop_keeps_every_applied_record() {
        // Only the payload over-read reverses; a failed compare keeps its prefix.
        let original = library();
        let mut merge = Merge::new(Some(original.view()));
        assert!(
            merge
                .apply_owned(modify(1, one(), with_freq(one(), 77)))
                .is_ok()
        );
        assert!(matches!(
            merge.apply_owned(modify(2, one(), with_freq(one(), 5))),
            Err(Stop::Mismatch("MODIFY", 2))
        ));
        let (_, items, _) = merge.finish_lazy(&mut Vec::new(), &"test");
        assert!(items.is_empty());
        let (outcome, _, overrides) = merge_of(
            &original,
            vec![
                modify(1, one(), with_freq(one(), 77)),
                modify(2, one(), with_freq(one(), 5)),
            ],
        );
        assert!(matches!(outcome, Err(Stop::Mismatch("MODIFY", 2))));
        assert_eq!(overrides[&1].as_ref().expect("one").prons[0].1, 77);
    }

    #[test]
    fn valid_records_before_and_after_an_over_read_item_stay() {
        // Row 107, option 1: the item answers no item and the rest stay.
        let original = library();
        let overlong = bytes_with(&[255, 255], 200);
        let (outcome, _, overrides) = merge_of(
            &original,
            vec![
                modify(2, two(), with_freq(two(), 88)),
                modify(1, one(), overlong),
                LogRecord::Add {
                    token: 6,
                    new_item: with_freq(one(), 5),
                },
            ],
        );
        assert!(outcome.is_ok());
        assert_eq!(overrides.get(&1), Some(&None));
        assert_eq!(overrides[&2].as_ref().expect("two").prons[1].1, 88);
        assert!(overrides[&6].is_some());
    }

    /// The decoded-item algorithm the buffer replaces (`arena_train`'s loop
    /// before the readings moved into the buffer).
    fn train_decoded(
        item: &mut ChunkItem,
        readings: &[Vec<oxpinyin_core::ChewingKey>],
        delta: u32,
    ) {
        for reading in readings {
            let mut total = 0_u32;
            for (keys, frequency) in &mut item.prons {
                total = total.wrapping_add(*frequency);
                if !crate::store_libpinyin::pronunciation_matches(reading, keys.iter().copied()) {
                    continue;
                }
                let Some(next) = total.checked_add(delta) else {
                    break;
                };
                total = next;
                *frequency = frequency.wrapping_add(delta);
            }
        }
    }

    #[test]
    fn training_in_the_buffer_matches_training_the_decoded_item() {
        use oxpinyin_core::ChewingKey;
        let ni = ChewingKey::new(23, 0, 1, 0);
        let hao = ChewingKey::new(11, 3, 2, 0);
        let (ni_bits, hao_bits) = (ni.to_packed(), hao.to_packed());
        let max = u32::MAX;
        // Counts near the `guint32` limit, a repeated key, a key nothing
        // matches, and a delta that overflows the running total at different
        // places: the partial update must come out the same.
        let shapes: Vec<Vec<(Vec<u16>, u32)>> = vec![
            vec![(vec![ni_bits], 100)],
            vec![(vec![ni_bits], 100), (vec![ni_bits], 7)],
            vec![(vec![hao_bits], 5), (vec![ni_bits], 9)],
            vec![(vec![ni_bits], max - 5), (vec![ni_bits], 3)],
            vec![(vec![hao_bits], max - 5), (vec![ni_bits], 3)],
            vec![(vec![ni_bits], 0), (vec![ni_bits], max), (vec![ni_bits], 1)],
            vec![
                (vec![ni_bits], max - 1),
                (vec![hao_bits], 1),
                (vec![ni_bits], 1),
            ],
        ];
        let reading_sets: Vec<Vec<Vec<ChewingKey>>> = vec![
            vec![vec![ni]],
            vec![vec![ni], vec![ni]],
            vec![vec![hao], vec![ni], vec![ni]],
            vec![vec![ChewingKey::new(0, 0, 0, 0)]],
            vec![],
        ];
        for prons in &shapes {
            for readings in &reading_sets {
                for delta in [0_u32, 1, 7, 483, max - 6, max] {
                    let item = item(&[0x4f60], 9, prons);
                    let library = SystemLibrary {
                        total: 9,
                        range_end: 2,
                        items: BTreeMap::from([(1, item.clone())]),
                        mapped: None,
                    };
                    let mut buffer = ReplayedLibrary::pristine(library.view());
                    assert!(buffer.train(1, readings.clone().into_iter(), delta));
                    let mut expected = item;
                    train_decoded(&mut expected, readings, delta);
                    assert_eq!(
                        buffer.item(1),
                        Some(expected),
                        "prons {prons:?} readings {readings:?} delta {delta}"
                    );
                    assert!(buffer.contains(1), "a trained slot is a touched slot");
                }
            }
        }
    }

    #[test]
    fn training_leaves_an_absent_slot_alone_and_does_not_consume_the_readings() {
        use oxpinyin_core::ChewingKey;
        let library = library();
        let mut buffer = ReplayedLibrary::pristine(library.view());
        let mut consumed = false;
        let readings = std::iter::from_fn(|| {
            consumed = true;
            None::<Vec<ChewingKey>>
        });
        assert!(!buffer.train(9, readings, 5));
        assert!(!consumed);
        assert!(!buffer.contains(9));
    }
}
