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
//! Session opens structurally validate each retained payload, then keep its private
//! byte view. Typed [`ChunkItem`]s are cached on use; value-only callers use
//! [`Merge::finish`].

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use oxpinyin_data::chunk_write::{
    ChunkItem, PHRASE_MASK, decode_phrase_item, validate_phrase_item,
};
use oxpinyin_data::phrase_library::PhraseLibrary;
use oxpinyin_data::user_files::LogRecord;

use crate::persistence::{SystemLibrary, journal_item_bytes};

/// Why the merge ended at a record.
pub(crate) enum Stop {
    /// `merge`'s `return false` at a REMOVE, MODIFY or header record.
    Mismatch(&'static str, u32),
    /// An ADD payload shorter than the unigram `add_phrase_item` reads:
    /// the pin's `get_unigram_frequency` asserts (`memory_chunk.h:390`,
    /// class (c), row 108). The record is not applied.
    ShortAdd(u32),
}

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

/// The private mapping the pin edits, or an inline/fallback byte buffer.
enum Shipped {
    Mapped(oxpinyin_data::phrase_library::PhraseContent),
    Inline(Vec<u8>),
}

impl Shipped {
    fn bytes(&self) -> &[u8] {
        match self {
            Self::Mapped(map) => map.as_slice(),
            Self::Inline(bytes) => bytes,
        }
    }
    fn bytes_mut(&mut self) -> &mut [u8] {
        match self {
            Self::Mapped(map) => map.as_mut_slice(),
            Self::Inline(bytes) => bytes,
        }
    }
}

/// `m_phrase_content`: edit the private view until an append copies it.
struct Content {
    shipped: Shipped,
    owned: Option<Vec<u8>>,
    data_end: usize,
    capacity: usize,
    // Inject a failed decode independently of the earlier header bounds check.
    #[cfg(test)]
    item_override: Option<(usize, Vec<u8>)>,
}

impl Content {
    fn shipped(area: Shipped) -> Self {
        let len = area.bytes().len();
        Self {
            shipped: area,
            owned: None,
            data_end: len,
            capacity: len,
            #[cfg(test)]
            item_override: None,
        }
    }
    fn data_len(&self) -> usize {
        self.data_end
    }
    fn buffer_capacity(&self) -> usize {
        self.capacity
    }
    fn byte(&self, pos: usize) -> Option<u8> {
        match &self.owned {
            Some(owned) if pos < self.capacity => Some(owned.get(pos).copied().unwrap_or(0)),
            Some(_) => None,
            None => self.shipped.bytes().get(pos).copied(),
        }
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
        if end > self.capacity {
            return None;
        }
        match &self.owned {
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
            self.shipped.bytes().get(offset..offset.checked_add(len)?)?,
        ))
    }
    fn append(&mut self, payload: &[u8]) -> u32 {
        let offset = if self.data_len() == 0 {
            FIRST_ITEM_OFFSET
        } else {
            self.data_len()
        };
        let needed = offset + payload.len();
        if let Some(owned) = &mut self.owned {
            if needed > self.capacity {
                self.capacity = self.capacity.saturating_mul(2).max(needed);
            }
            if needed > owned.len() {
                owned.resize(needed, 0);
            }
            owned[offset..needed].copy_from_slice(payload);
        } else {
            let mut owned = self.shipped.bytes().to_vec();
            owned.resize(offset, 0);
            owned.extend_from_slice(payload);
            self.capacity = needed;
            self.owned = Some(owned);
        }
        self.data_end = needed;
        u32::try_from(offset).unwrap_or(u32::MAX)
    }
    fn write(&mut self, offset: usize, bytes: &[u8]) -> bool {
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
            None => self.write_shipped(offset, bytes),
        }
    }
    fn write_shipped(&mut self, offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else {
            return false;
        };
        let Some(target) = self.shipped.bytes_mut().get_mut(offset..end) else {
            return false;
        };
        target.copy_from_slice(bytes);
        true
    }
}

/// Where a mapped library's item offsets come from.
enum Origin {
    Mapped(PhraseLibrary),
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
fn origin_and_content(original: Option<&SystemLibrary>) -> (Origin, Content) {
    match original {
        Some(library) => match &library.mapped {
            Some(mapped) => (
                Origin::Mapped(mapped.clone()),
                Content::shipped(
                    mapped
                        .private_content()
                        .map(Shipped::Mapped)
                        .unwrap_or_else(|_| Shipped::Inline(mapped.item_area().to_vec())),
                ),
            ),
            None if !library.items.is_empty() => {
                let mut bytes = vec![0; FIRST_ITEM_OFFSET];
                let mut offsets = BTreeMap::new();
                for (slot, item) in &library.items {
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
                (
                    Origin::Inline {
                        offsets,
                        bytes: bytes.clone(),
                    },
                    Content::shipped(Shipped::Inline(bytes)),
                )
            }
            None => (Origin::None, Content::shipped(Shipped::Inline(Vec::new()))),
        },
        None => (Origin::None, Content::shipped(Shipped::Inline(Vec::new()))),
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

pub(crate) struct Merge {
    origin: Origin,
    content: Content,
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
    /// Relocated or removed slots only; in-place writes keep mapped offsets.
    offsets: BTreeMap<u32, u32>,
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

impl Merge {
    pub(crate) fn new(original: Option<&SystemLibrary>) -> Self {
        let (origin, content) = origin_and_content(original);
        Self {
            origin,
            content,
            touched: Vec::new(),
            unigram_changes: Vec::new(),
            has_added_items: false,
            overreach: false,
            offsets: BTreeMap::new(),
            total: original.map(|library| library.total),
            running: original.map_or(0, |library| library.total),
            stale: Stale::Default(vec![0; PHRASE_ITEM_HEADER]),
            stale_slot: None,
            appends: 0,
        }
    }

    fn original_offset(&self, slot: u32) -> u32 {
        match &self.origin {
            Origin::Mapped(library) => library.content_offset(slot).unwrap_or(0),
            Origin::Inline { offsets, .. } => offsets.get(&slot).map_or(0, |item| item.offset),
            Origin::None => 0,
        }
    }

    fn slot_offset(&self, slot: u32) -> u32 {
        self.offsets
            .get(&slot)
            .copied()
            .unwrap_or_else(|| self.original_offset(slot))
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
        self.offsets.insert(slot, offset);
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

    pub(crate) fn apply(&mut self, record: LogRecord<impl AsRef<[u8]>>) -> Result<(), Stop> {
        match record {
            LogRecord::Add { token, new_item } => self.add(token & PHRASE_MASK, new_item.as_ref()),
            LogRecord::Remove { token, old_item } => self.remove(token, old_item.as_ref()),
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => self.modify(token, old_item.as_ref(), new_item.as_ref()),
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

    fn add(&mut self, slot: u32, payload: &[u8]) -> Result<(), Stop> {
        // `add_phrase_item` stores the payload as it stands (`:208-217`); it
        // does not read the header, then reads the payload's unigram
        // (`phrase_index.h:124-126`). A payload shorter than six bytes
        // aborts the pin at `memory_chunk.h:390` (class (c), row 108): the
        // record is not applied and the replay stops there.
        if payload.len() < PHRASE_ITEM_HEADER {
            return Err(Stop::ShortAdd(slot));
        }
        let unigram = raw_unigram(payload);
        let offset = self.content.append(payload);
        self.appends += 1;
        self.set_offset(slot, offset);
        self.add_total(unigram);
        Ok(())
    }

    fn remove(&mut self, token: u32, old_item: &[u8]) -> Result<(), Stop> {
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

    fn modify(&mut self, token: u32, old_item: &[u8], new_item: &[u8]) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let mismatch = || Stop::Mismatch("MODIFY", token);
        match self.visible(slot) {
            // The item's length ends past the content buffer: the compare or
            // the remove-and-add would read past it (class (b), row 107).
            Found::Past => Err(mismatch()),
            Found::Item { offset, len } => {
                self.stale_slot = Some(slot);
                self.stale = if self.content.owned.is_some() {
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

    fn stale_write(&mut self, place: Place, bytes: &[u8]) -> bool {
        match place {
            Place::Default => match &mut self.stale {
                Stale::Default(buffer) => {
                    buffer[..bytes.len()].copy_from_slice(bytes);
                    true
                }
                Stale::Shipped { .. } | Stale::Owned { .. } => false,
            },
            Place::Shipped { offset, .. } => self.content.write_shipped(offset, bytes),
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
    fn modify_stale(&mut self, old_item: &[u8], new_item: &[u8]) -> Option<()> {
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
        if !self.stale_write(place, new_item) {
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
                    skipped.push(format!("{name}: item {slot} ends past the phrase content"))
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

/// A completed private sub-index view. Typed payloads are decoded on demand.
#[derive(Clone)]
pub(crate) struct ReplayedLibrary(std::sync::Arc<Merge>);

impl std::fmt::Debug for ReplayedLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReplayedLibrary")
            .field("slots", &self.0.touched.len())
            .finish()
    }
}

impl ReplayedLibrary {
    pub(crate) fn slots(&self) -> impl DoubleEndedIterator<Item = u32> + '_ {
        self.0.touched.iter().copied()
    }
    /// Skip ADD discovery for MODIFY-only views. Mixed views keep the existing
    /// original-item filter at the caller, including inline fixture overrides.
    pub(crate) fn add_candidates(&self) -> impl Iterator<Item = u32> + '_ {
        let slots = if self.0.has_added_items {
            self.0.touched.as_slice()
        } else {
            &[]
        };
        slots.iter().copied()
    }
    /// Only slots that may need a stored unigram delta. Repeated changes are
    /// visited once, and rejected/removed slots never reach this iterator.
    pub(crate) fn unigram_slots(&self) -> impl Iterator<Item = u32> + '_ {
        self.0.unigram_changes.iter().copied()
    }
    pub(crate) fn contains(&self, slot: u32) -> bool {
        self.0.touched.binary_search(&slot).is_ok()
    }
    pub(crate) fn bytes(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        if !self.contains(slot) {
            return None;
        }
        let offset = self.0.slot_offset(slot) as usize;
        let Found::Item { offset, len } = self.0.content.header(offset) else {
            return None;
        };
        self.0.content.item(offset, len)
    }
    pub(crate) fn item(&self, slot: u32) -> Option<ChunkItem> {
        decode_phrase_item(&self.bytes(slot)?).ok()
    }
}

impl Merge {
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
            if !self.offsets.contains_key(&slot) && len > original_extent {
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
        let relocated = self.offsets.get(&slot).copied();
        let offset = relocated.unwrap_or_else(|| self.original_offset(slot));
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
                            if relocated.is_none() && len > original_extent {
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
            let origin = &self.origin;
            let offsets_map = &self.offsets;
            self.touched.retain(|&slot| {
                let offset = offsets_map
                    .get(&slot)
                    .copied()
                    .unwrap_or_else(|| match origin {
                        Origin::Mapped(library) => library.content_offset(slot).unwrap_or(0),
                        Origin::Inline { offsets, .. } => {
                            offsets.get(&slot).map_or(0, |item| item.offset)
                        }
                        Origin::None => 0,
                    });
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
        let retained =
            (!self.touched.is_empty()).then(|| ReplayedLibrary(std::sync::Arc::new(self)));
        (total, typed, retained)
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
        let mut merge = Merge::new(Some(&original));
        for record in [
            modify(1, one(), with_freq(one(), 77)),
            modify(2, two(), changed.clone()),
            modify(2, changed, restored.clone()),
            modify(2, restored, with_freq(two(), 88)),
        ] {
            assert!(merge.apply(record).is_ok());
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
            let mut merge = Merge::new(Some(&original));
            let changed = with_freq(one(), 77);
            let record = if replacement {
                LogRecord::Add {
                    token: 1,
                    new_item: changed,
                }
            } else {
                modify(1, one(), changed)
            };
            assert!(merge.apply(record).is_ok());
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
                over_length_merge()
            } else {
                Merge::new(Some(&original))
            };
            if !overreach {
                assert!(merge.apply(modify(1, one(), with_freq(one(), 77))).is_ok());
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
                assert!(merge.apply(record).is_ok());
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
        for rejected_add in [false, true] {
            let mut merge = over_length_merge();
            if rejected_add {
                assert!(
                    merge
                        .apply(LogRecord::Add {
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
        let mut merge = Merge::new(Some(&original));
        assert!(merge.apply(modify(1, one(), reading_only.clone())).is_ok());
        assert!(merge.apply(modify(9, reading_only, changed)).is_ok());
        let (_, _, view) = merge.finish_lazy(&mut Vec::new(), &"test");
        let view = view.expect("retained original");
        assert_eq!(view.slots().collect::<Vec<_>>(), [1]);
        assert_eq!(view.unigram_slots().collect::<Vec<_>>(), [1]);
        assert_eq!(view.item(1).expect("stale storage").unigram, 150);
    }

    #[test]
    fn a_cross_item_write_marks_the_other_retained_unigram() {
        let original = library();
        let mut merge = Merge::new(Some(&original));
        assert!(merge.apply(modify(2, two(), with_freq(two(), 88))).is_ok());
        // A longer header in a short payload borrows the next item's bytes,
        // just as the existing cross-item replay cases require.
        let mut expanded = one();
        expanded[1] = 3;
        assert!(merge.apply(modify(1, one(), expanded)).is_ok());
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
        assert!(merge.apply(modify(1, old, new)).is_ok());
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
    fn over_length_merge() -> Merge {
        let original = library();
        let mut merge = Merge::new(Some(&original));
        assert!(
            merge
                .apply(modify(1, one(), bytes_with(&[2, 2], 200)))
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
        assert!(merge.apply(modify(1, old, new)).is_ok());
        merge
    }

    #[test]
    fn an_unrecorded_overlapped_slot_is_visible_and_seeded() {
        let merge = over_length_merge();
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
        let merge = over_length_merge();
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
        let mut merge = Merge::new(Some(&original));
        assert!(merge.apply(modify(1, one(), with_freq(one(), 77))).is_ok());
        assert_eq!(merge.touched, vec![1]);
        let (_, _, view) = merge.finish_lazy(&mut Vec::new(), &"test");
        let view = view.expect("retained original");
        assert_eq!(view.slots().collect::<Vec<_>>(), [1]);
        assert!(view.unigram_slots().next().is_none());
    }

    #[test]
    fn rejected_unigram_candidates_do_not_seed_the_baseline_fallback() {
        let original = library();
        let mut merge = Merge::new(Some(&original));
        assert!(merge.apply(modify(2, two(), with_freq(two(), 88))).is_ok());
        assert!(
            merge
                .apply(modify(1, one(), bytes_with(&[255, 255], 200)))
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
        let mut merge = Merge::new(Some(&original));
        assert!(
            merge
                .apply(modify(1, one(), bytes_with(&[2, 3], 200)))
                .is_ok()
        );
        merge.content.append(&[0; 8]);
        let mut changed = two();
        changed[2..6].copy_from_slice(&170_u32.to_le_bytes());
        assert!(merge.apply(modify(2, two(), changed)).is_ok());
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
            let mut merge = Merge::new(Some(&original));
            assert!(
                merge
                    .apply(modify(1, one(), bytes_with(&[2, 1], 200)))
                    .is_ok()
            );
            assert!(merge.apply(modify(2, two(), invalid.clone())).is_ok());
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
        assert!(!eager.contains_key(&2));
        assert!(!lazy.contains_key(&2));
        assert!(retained.is_some());
    }

    #[test]
    fn overreach_tracks_only_a_growing_header() {
        let original = library();

        let mut merge = Merge::new(Some(&original));
        let same = with_last(one(), 0x7f);
        assert!(merge.modify(1, &one(), &same).is_ok());
        assert!(!merge.overreach);

        let mut merge = Merge::new(Some(&original));
        let grown = bytes_with(&[3, 1], 200);
        assert!(merge.modify(1, &one(), &grown).is_ok());
        assert!(merge.overreach);

        let mut merge = Merge::new(Some(&original));
        let shorter = bytes_with(&[0, 0], 0);
        assert!(merge.modify(1, &one(), &shorter).is_ok());
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
            let mut merge = Merge::new(Some(&original));
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
                assert!(merge.apply(record).is_ok());
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
            let mut merge = Merge::new(Some(&original));
            for record in [
                modify(2, two(), with_freq(two(), 88)),
                modify(1, one(), with_freq(one(), 77)),
                modify(2, with_freq(two(), 88), with_freq(two(), 99)),
                modify(1, with_freq(one(), 77), with_freq(one(), 66)),
            ] {
                assert!(merge.apply(record).is_ok());
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
            let mut merge = Merge::new(Some(&original));
            assert!(
                merge
                    .apply(modify(valid_slot, one(), with_freq(one(), 88)))
                    .is_ok()
            );
            for slot in (1..=invalid_count).rev() {
                assert!(
                    merge
                        .apply(modify(slot, one(), with_freq(one(), 77)))
                        .is_ok()
                );
                assert!(
                    merge
                        .apply(modify(slot, with_freq(one(), 77), invalid.clone()))
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
        assert_eq!(eager.keys().copied().collect::<Vec<_>>(), [valid_slot]);
        assert!(lazy.is_empty());
        assert_eq!(retained.slots().collect::<Vec<_>>(), [valid_slot]);
        assert_eq!(retained.item(valid_slot), eager[&valid_slot]);
        for slot in 1..=invalid_count {
            assert!(!retained.contains(slot));
            assert!(retained.item(slot).is_none());
            assert_eq!(
                retained.item(slot).or_else(|| original.item(slot)),
                original.item(slot)
            );
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
            let mut merge = Merge::new(Some(&original));
            assert!(merge.apply(modify(1, one(), invalid.clone())).is_ok());
            assert!(merge.apply(modify(2, two(), with_freq(two(), 88))).is_ok());
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
        assert!(!eager_items.contains_key(&1));
        assert!(!lazy_items.contains_key(&1));
        assert!(!retained.contains(1));
        assert!(retained.item(1).is_none());
        assert_eq!(retained.item(2), eager_items[&2]);

        let token = (1 << 24) | 1;
        let mut live =
            crate::system_items::SystemItems::new(BTreeMap::new(), BTreeMap::from([(1, retained)]));
        assert!(!live.contains_key(&token));
        assert!(live.get(&token).is_none());
        let eager_visible = eager_items
            .get(&1)
            .cloned()
            .unwrap_or_else(|| original.item(1));
        let lazy_visible = live.get(&token).cloned().or_else(|| original.item(1));
        assert_eq!(lazy_visible, eager_visible);
        assert_eq!(lazy_visible, original.item(1));
    }

    #[test]
    fn lazy_original_decode_failure_drops_touched_and_keeps_baseline() {
        let original = library();
        let invalid = bytes_with(&[255, 255], 200);
        let expected = format!("test: {}", decode_phrase_item(&invalid).unwrap_err());
        let replay = || {
            let mut merge = Merge::new(Some(&original));
            assert!(merge.apply(modify(1, one(), with_freq(one(), 77))).is_ok());
            assert!(merge.apply(modify(2, two(), with_freq(two(), 88))).is_ok());
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
        assert!(!retained.0.touched.contains(&1));
        assert!(!retained.contains(1));
        assert!(retained.item(1).is_none());
        assert_eq!(retained.item(2), eager_items[&2]);

        let token = (1 << 24) | 1;
        let mut live =
            crate::system_items::SystemItems::new(BTreeMap::new(), BTreeMap::from([(1, retained)]));
        assert!(!live.contains_key(&token));
        assert!(live.get(&token).is_none());
        let eager_visible = eager_items
            .get(&1)
            .cloned()
            .unwrap_or_else(|| original.item(1));
        let lazy_visible = live.get(&token).cloned().or_else(|| original.item(1));
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
        let mut merge = Merge::new(Some(library));
        let mut outcome = Ok(());
        for record in records {
            if let Err(stop) = merge.apply(record) {
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
        // reads past its allocation (row 107), so the subject stops.
        let overlong = bytes_with(&[255, 255], 200);
        let (outcome, total, overrides) =
            merge_of(&library(), vec![modify(1, one(), overlong.clone())]);
        assert!(outcome.is_ok());
        assert_eq!(total, Some(150));
        assert!(!overrides.contains_key(&1), "the over-read item is skipped");
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
        assert!(!overrides.contains_key(&5), "the over-read item is skipped");
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
    fn a_short_add_stops_the_replay_before_it_applies() {
        // A payload shorter than the unigram the ADD reads aborts the pin
        // (`memory_chunk.h:390`, row 108). The replay stops at that record:
        // the earlier MODIFY stays, the short ADD and everything after it
        // do not apply.
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
}
