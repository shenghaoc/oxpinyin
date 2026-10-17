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
//! Typed [`ChunkItem`]s are made once, at [`Merge::finish`].

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::BTreeMap;

use oxpinyin_data::chunk_write::{ChunkItem, PHRASE_MASK, decode_phrase_item};
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

/// The pin's `m_phrase_content`: the shipped item area, then the payloads an
/// `add_phrase_item` appends to it, in order.
struct Content<'a> {
    /// The shipped item area, exactly as loaded.
    shipped: Cow<'a, [u8]>,
    /// In-place writes into the shipped mapping before its first copy, as
    /// disjoint runs sorted by their start.
    writes: Vec<(usize, Vec<u8>)>,
    /// The run scan's cursor: the first run that starts past the last offset
    /// read. Replay reads slots in ascending order, so the scan is linear.
    cursor: Cell<usize>,
    /// The copy the first append makes, and every appended byte after it.
    owned: Option<Vec<u8>>,
    /// The copy's data end; in-place writes into its slack do not move it.
    data_end: usize,
    /// The copy's allocation size; bytes past the data end are zero.
    capacity: usize,
}

impl<'a> Content<'a> {
    fn shipped(area: Cow<'a, [u8]>) -> Self {
        Self {
            data_end: area.len(),
            capacity: area.len(),
            shipped: area,
            writes: Vec::new(),
            cursor: Cell::new(0),
            owned: None,
        }
    }

    fn data_len(&self) -> usize {
        self.owned
            .as_ref()
            .map_or(self.shipped.len(), |_| self.data_end)
    }

    fn buffer_capacity(&self) -> usize {
        self.owned
            .as_ref()
            .map_or(self.shipped.len(), |_| self.capacity)
    }

    /// The first run that starts past `pos`, advancing a one-way cursor.
    fn scan(&self, pos: usize) -> usize {
        let mut index = self.cursor.get();
        if index > self.writes.len() || (index > 0 && self.writes[index - 1].0 > pos) {
            index = 0;
        }
        while index < self.writes.len() && self.writes[index].0 <= pos {
            index += 1;
        }
        self.cursor.set(index);
        index
    }

    /// One byte of the shipped mapping with its in-place writes.
    fn shipped_byte(&self, pos: usize) -> Option<u8> {
        if pos >= self.shipped.len() {
            return None;
        }
        let index = self.scan(pos);
        if let Some((start, run)) = index.checked_sub(1).map(|index| &self.writes[index])
            && pos < start + run.len()
        {
            return Some(run[pos - *start]);
        }
        Some(self.shipped[pos])
    }

    /// One byte of the current content; zeros between the data end and the
    /// copy's allocation end, `None` past the allocation.
    fn byte(&self, pos: usize) -> Option<u8> {
        match &self.owned {
            Some(owned) => {
                if pos < owned.len() {
                    Some(owned[pos])
                } else if pos < self.capacity {
                    Some(0)
                } else {
                    None
                }
            }
            None => self.shipped_byte(pos),
        }
    }

    /// `SubPhraseIndex::get_phrase_item`'s read of the two header bytes and
    /// the item view they give (`phrase_index.cpp:180-206`).
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

    /// The current content's bytes at `[offset, offset + len)`, or `None`
    /// when the range ends past the allocation.
    fn read(&self, offset: usize, len: usize) -> Option<Vec<u8>> {
        let end = offset.checked_add(len)?;
        if end > self.buffer_capacity() {
            return None;
        }
        (offset..end).map(|pos| self.byte(pos)).collect()
    }

    /// The current content's item bytes, borrowed when no write overlays them.
    fn item(&self, offset: usize, len: usize) -> Option<Cow<'_, [u8]>> {
        let end = offset.checked_add(len)?;
        if end > self.buffer_capacity() {
            return None;
        }
        match &self.owned {
            Some(owned) if end <= owned.len() => Some(Cow::Borrowed(&owned[offset..end])),
            Some(_) => Some(Cow::Owned(self.read(offset, len)?)),
            None => self.shipped_item(offset, len),
        }
    }

    /// Bytes of the shipped mapping, with its in-place writes: what a stale
    /// view into the mapping still sees after the first append copied it.
    fn shipped_item(&self, offset: usize, len: usize) -> Option<Cow<'_, [u8]>> {
        let end = offset.checked_add(len)?;
        if end > self.shipped.len() {
            return None;
        }
        let index = self.scan(offset);
        let containing = index
            .checked_sub(1)
            .map(|index| &self.writes[index])
            .filter(|(start, run)| offset < start + run.len());
        let inside = self
            .writes
            .get(index)
            .is_some_and(|(start, _)| *start < end);
        if containing.is_none() && !inside {
            return Some(Cow::Borrowed(&self.shipped[offset..end]));
        }
        // A run that starts at or before the item and ends at or after it,
        // with nothing overlapping behind it, serves the item itself.
        if let Some((start, run)) = containing
            && start + run.len() >= end
            && !inside
        {
            return Some(Cow::Borrowed(&run[offset - *start..end - *start]));
        }
        let mut out = self.shipped[offset..end].to_vec();
        let mut apply = |start: &usize, run: &[u8]| {
            let run_end = *start + run.len();
            let from = std::cmp::max(*start, offset);
            let to = std::cmp::min(run_end, end);
            out[from - offset..to - offset].copy_from_slice(&run[from - *start..to - *start]);
        };
        if let Some((start, run)) = containing {
            apply(start, run);
        }
        for (start, run) in &self.writes[index..] {
            if *start >= end {
                break;
            }
            apply(start, run);
        }
        Some(Cow::Owned(out))
    }

    /// `add_phrase_item`: append `payload` at the content's end (at 8 for an
    /// empty buffer) and return the slot's new offset.
    fn append(&mut self, payload: &[u8]) -> u32 {
        let data_len = self.data_len();
        let offset = if data_len == 0 {
            FIRST_ITEM_OFFSET
        } else {
            data_len
        };
        if self.owned.is_some() {
            let needed = offset + payload.len();
            if needed > self.capacity {
                self.capacity = std::cmp::max(self.capacity.saturating_mul(2), needed);
            }
            if let Some(owned) = self.owned.as_mut() {
                if owned.len() < needed {
                    owned.resize(needed, 0);
                }
                owned[offset..needed].copy_from_slice(payload);
            }
            self.data_end = needed;
        } else {
            let mut owned = vec![0; offset];
            owned[..self.shipped.len()].copy_from_slice(&self.shipped);
            for (start, run) in &self.writes {
                let to = std::cmp::min(start + run.len(), owned.len());
                if *start < to {
                    owned[*start..to].copy_from_slice(&run[..to - *start]);
                }
            }
            owned.extend_from_slice(payload);
            self.data_end = owned.len();
            self.capacity = owned.len();
            self.owned = Some(owned);
        }
        u32::try_from(offset).unwrap_or(u32::MAX)
    }

    /// An in-place `memmove` into the current content.
    fn write(&mut self, offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else {
            return false;
        };
        if end > self.buffer_capacity() {
            return false;
        }
        match self.owned.as_mut() {
            Some(owned) => {
                if end > owned.len() {
                    owned.resize(end, 0);
                }
                owned[offset..end].copy_from_slice(bytes);
                true
            }
            None => self.write_run(offset, bytes),
        }
    }

    /// Adds one run to the shipped writes, trimming the runs it overlaps so
    /// the map stays disjoint and key order is content order.
    fn write_run(&mut self, offset: usize, bytes: &[u8]) -> bool {
        let Some(end) = offset.checked_add(bytes.len()) else {
            return false;
        };
        if end > self.shipped.len() {
            return false;
        }
        if bytes.is_empty() {
            return true;
        }
        if self
            .writes
            .last()
            .is_none_or(|(start, run)| start + run.len() <= offset)
        {
            self.writes.push((offset, bytes.to_vec()));
            return true;
        }
        let mut first = self.writes.partition_point(|(start, _)| *start < offset);
        if first > 0 {
            let (start, run) = &self.writes[first - 1];
            if start + run.len() > offset {
                first -= 1;
            }
        }
        let mut last = first;
        while last < self.writes.len() && self.writes[last].0 < end {
            last += 1;
        }
        let mut pieces: Vec<(usize, Vec<u8>)> = Vec::new();
        for (start, run) in &self.writes[first..last] {
            if *start < offset {
                pieces.push((*start, run[..offset - *start].to_vec()));
            }
            let run_end = start + run.len();
            if run_end > end {
                pieces.push((end, run[end - *start..].to_vec()));
            }
        }
        pieces.push((offset, bytes.to_vec()));
        pieces.sort_by_key(|(start, _)| *start);
        self.writes.splice(first..last, pieces);
        self.cursor.set(0);
        true
    }

    /// A write through a stale view into the shipped mapping: after its copy
    /// that memory is no longer the content, so it is kept as an overlay.
    fn write_shipped(&mut self, offset: usize, bytes: &[u8]) -> bool {
        self.write_run(offset, bytes)
    }
}

/// Where a mapped library's item offsets come from.
enum Origin<'a> {
    Mapped(&'a PhraseLibrary),
    /// A fixture `SystemLibrary`'s inline items, laid out as the pin's
    /// appended content would be.
    Inline(BTreeMap<u32, u32>),
    None,
}

/// The content and offsets a replay starts from.
fn origin_and_content<'a>(original: Option<&'a SystemLibrary>) -> (Origin<'a>, Content<'a>) {
    match original {
        Some(library) => match &library.mapped {
            Some(mapped) => (
                Origin::Mapped(mapped),
                Content::shipped(Cow::Borrowed(mapped.item_area())),
            ),
            None if !library.items.is_empty() => {
                let mut bytes = vec![0; FIRST_ITEM_OFFSET];
                let mut offsets = BTreeMap::new();
                for (slot, item) in &library.items {
                    offsets.insert(*slot, u32::try_from(bytes.len()).unwrap_or(u32::MAX));
                    bytes.extend_from_slice(&journal_item_bytes(item));
                }
                (Origin::Inline(offsets), Content::shipped(Cow::Owned(bytes)))
            }
            None => (Origin::None, Content::shipped(Cow::Borrowed(&[]))),
        },
        None => (Origin::None, Content::shipped(Cow::Borrowed(&[]))),
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
fn visible_len(bytes: &[u8]) -> Option<usize> {
    let (&length, &prons) = (bytes.first()?, bytes.get(1)?);
    let (length, prons) = (usize::from(length), usize::from(prons));
    let len = PHRASE_ITEM_HEADER + 4 * length + prons * (2 * length + 4);
    (len <= bytes.len()).then_some(len)
}

pub(crate) struct Merge<'a> {
    origin: Origin<'a>,
    content: Content<'a>,
    /// Slots a record changed, with their current offset; 0 is removed.
    /// `finish` walks this map in ascending slot order.
    touched: BTreeMap<u32, u32>,
    /// `SubPhraseIndex::m_total_freq`; `None` for a library with no original.
    total: Option<u32>,
    /// The same total for a library with no original to start from.
    running: u32,
    /// The `PhraseItem` MODIFY reads into.
    stale: Stale,
    /// `add_phrase_item` calls so far.
    appends: u64,
}

impl<'a> Merge<'a> {
    pub(crate) fn new(original: Option<&'a SystemLibrary>) -> Self {
        let (origin, content) = origin_and_content(original);
        Self {
            origin,
            content,
            touched: BTreeMap::new(),
            total: original.map(|library| library.total),
            running: original.map_or(0, |library| library.total),
            stale: Stale::Default(vec![0; PHRASE_ITEM_HEADER]),
            appends: 0,
        }
    }

    fn original_offset(&self, slot: u32) -> u32 {
        match &self.origin {
            Origin::Mapped(library) => library.content_offset(slot).unwrap_or(0),
            Origin::Inline(offsets) => offsets.get(&slot).copied().unwrap_or(0),
            Origin::None => 0,
        }
    }

    fn slot_offset(&self, slot: u32) -> u32 {
        self.touched
            .get(&slot)
            .copied()
            .unwrap_or_else(|| self.original_offset(slot))
    }

    fn set_offset(&mut self, slot: u32, offset: u32) {
        self.touched.insert(slot, offset);
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

    pub(crate) fn apply(&mut self, record: LogRecord) -> Result<(), Stop> {
        match record {
            LogRecord::Add { token, new_item } => self.add(token & PHRASE_MASK, new_item),
            LogRecord::Remove { token, old_item } => self.remove(token, &old_item),
            LogRecord::Modify {
                token,
                old_item,
                new_item,
            } => self.modify(token, &old_item, new_item),
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

    fn add(&mut self, slot: u32, payload: Vec<u8>) -> Result<(), Stop> {
        // `add_phrase_item` stores the payload as it stands (`:208-217`); it
        // does not read the header, then reads the payload's unigram
        // (`phrase_index.h:124-126`). A payload shorter than six bytes
        // aborts the pin at `memory_chunk.h:390` (class (c), row 108): the
        // record is not applied and the replay stops there.
        if payload.len() < PHRASE_ITEM_HEADER {
            return Err(Stop::ShortAdd(slot));
        }
        let unigram = raw_unigram(&payload);
        let offset = self.content.append(&payload);
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

    fn modify(&mut self, token: u32, old_item: &[u8], new_item: Vec<u8>) -> Result<(), Stop> {
        let slot = token & PHRASE_MASK;
        let mismatch = || Stop::Mismatch("MODIFY", token);
        match self.visible(slot) {
            // The item's length ends past the content buffer: the compare or
            // the remove-and-add would read past it (class (b), row 107).
            Found::Past => Err(mismatch()),
            Found::Item { offset, len } => {
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
                if new_item.len() > len {
                    // A remove and an add of the whole new payload.
                    let old_unigram = raw_unigram(&bytes);
                    let new_unigram = raw_unigram(&new_item);
                    self.subtract_total(old_unigram);
                    self.add_total(new_unigram);
                    let offset = self.content.append(&new_item);
                    self.appends += 1;
                    self.set_offset(slot, offset);
                } else {
                    // `memmove` over the stored bytes; the item keeps its
                    // header-derived length for the next read.
                    if !self.content.write(offset, &new_item) {
                        return Err(mismatch());
                    }
                    self.touched
                        .insert(slot, u32::try_from(offset).unwrap_or(u32::MAX));
                }
                Ok(())
            }
            Found::Failed => self.modify_stale(old_item, &new_item).ok_or_else(mismatch),
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
        let equal = self
            .stale_read(place)
            .is_some_and(|bytes| bytes.as_ref() == old_item);
        if !equal || new_item.len() > size {
            return None;
        }
        self.stale_write(place, new_item).then_some(())
    }

    /// The library's total and its overrides as typed items; `None` removes.
    pub(crate) fn finish(
        self,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
    ) -> (Option<u32>, BTreeMap<u32, Option<ChunkItem>>) {
        let mut typed = Vec::with_capacity(self.touched.len());
        for (&slot, &offset) in &self.touched {
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

    fn with_freq(mut item: Vec<u8>, frequency: u32) -> Vec<u8> {
        let end = item.len();
        item[end - 4..].copy_from_slice(&frequency.to_le_bytes());
        item
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
