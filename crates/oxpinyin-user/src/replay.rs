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
    /// appended content would be.
    Inline(BTreeMap<u32, u32>),
    None,
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
                    offsets.insert(*slot, u32::try_from(bytes.len()).unwrap_or(u32::MAX));
                    bytes.extend_from_slice(&journal_item_bytes(item));
                }
                (
                    Origin::Inline(offsets),
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
fn visible_len(bytes: &[u8]) -> Option<usize> {
    let (&length, &prons) = (bytes.first()?, bytes.get(1)?);
    let (length, prons) = (usize::from(length), usize::from(prons));
    let len = PHRASE_ITEM_HEADER + 4 * length + prons * (2 * length + 4);
    (len <= bytes.len()).then_some(len)
}

pub(crate) struct Merge {
    origin: Origin,
    content: Content,
    /// Changed slots, walked in ascending order without copying item bytes.
    touched: BTreeSet<u32>,
    /// Relocated or removed slots only; in-place writes keep mapped offsets.
    offsets: BTreeMap<u32, u32>,
    /// `SubPhraseIndex::m_total_freq`; `None` for a library with no original.
    total: Option<u32>,
    /// The same total for a library with no original to start from.
    running: u32,
    /// The `PhraseItem` MODIFY reads into.
    stale: Stale,
    /// `add_phrase_item` calls so far.
    appends: u64,
}

impl Merge {
    pub(crate) fn new(original: Option<&SystemLibrary>) -> Self {
        let (origin, content) = origin_and_content(original);
        Self {
            origin,
            content,
            touched: BTreeSet::new(),
            offsets: BTreeMap::new(),
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
        self.offsets
            .get(&slot)
            .copied()
            .unwrap_or_else(|| self.original_offset(slot))
    }

    fn set_offset(&mut self, slot: u32, offset: u32) {
        self.touched.insert(slot);
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
                    let new_unigram = raw_unigram(new_item);
                    self.subtract_total(old_unigram);
                    self.add_total(new_unigram);
                    let offset = self.content.append(new_item);
                    self.appends += 1;
                    self.set_offset(slot, offset);
                } else {
                    // `memmove` over the stored bytes; the item keeps its
                    // header-derived length for the next read.
                    if !self.content.write(offset, new_item) {
                        return Err(mismatch());
                    }
                    self.touched.insert(slot);
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
    pub(crate) fn contains(&self, slot: u32) -> bool {
        self.0.touched.contains(&slot)
    }
    pub(crate) fn bytes(&self, slot: u32) -> Option<Cow<'_, [u8]>> {
        if !self.0.touched.contains(&slot) {
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
    /// Keep original live slots as one private view. Added and removed slots
    /// still use the session's existing value/index/removal paths.
    pub(crate) fn finish_lazy(
        mut self,
        skipped: &mut Vec<String>,
        name: &impl std::fmt::Display,
    ) -> (
        Option<u32>,
        BTreeMap<u32, Option<ChunkItem>>,
        Option<ReplayedLibrary>,
    ) {
        let mut typed = BTreeMap::new();
        let origin = &self.origin;
        let offsets = &self.offsets;
        self.touched.retain(|&slot| {
            let offset = offsets.get(&slot).copied().unwrap_or_else(|| match origin {
                Origin::Mapped(library) => library.content_offset(slot).unwrap_or(0),
                Origin::Inline(offsets) => offsets.get(&slot).copied().unwrap_or(0),
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
                    let original = match &self.origin {
                        Origin::Mapped(library) => library.item(slot).is_some(),
                        Origin::Inline(offsets) => offsets.contains_key(&slot),
                        Origin::None => false,
                    };
                    if original {
                        match self.content.item(offset, len) {
                            Some(bytes) => {
                                if let Err(error) = validate_phrase_item(&bytes) {
                                    skipped.push(format!("{name}: {error}"));
                                    return false;
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
