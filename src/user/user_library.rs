//! Owned session sub-index: packed item bodies and independent index membership.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

use crate::data::chunk_format::CHUNK_HEADER_SIZE;
use crate::data::phrase_library::PhraseItemView;
use crate::data::phrase_library::sub_phrase_index_sections;
use crate::data::phrase_library::{
    CHEWING_KEY_SIZE, FIRST_ITEM_OFFSET, INDEX_ONE, ITEM_HEADER_SIZE, SEPARATOR,
};

use crate::user::user_index::UserIndex;
use crate::user::user_phrase_table::UserPhraseTable;

/// Train a complete item's header-bounded readings (`phonetic_lookup.h:913-929`
/// at 074a2219). Earlier increments survive a later prefix-total refusal.
pub(super) fn train_readings(
    bytes: &mut [u8],
    readings: impl Iterator<Item = Vec<crate::parser::ChewingKey>>,
    delta: u32,
) {
    let length = usize::from(bytes[0]);
    let first = ITEM_HEADER_SIZE + 4 * length;
    let keys_len = CHEWING_KEY_SIZE * length;
    for reading in readings {
        if reading.len() != length {
            continue;
        }
        let mut total = 0_u32;
        for entry in bytes[first..].chunks_exact_mut(keys_len + 4) {
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
}

/// The pin's SubPhraseIndex (phrase_index.h:257-430 at 074a2219), owned.
#[derive(Clone, Debug, Default)]
pub(crate) struct UserLibrary {
    total: u32,
    slots: BTreeMap<u32, Range<usize>>,
    content: Vec<u8>,
    shared_starts: BTreeSet<usize>,
}

/// `MemoryChunk::ensure_has_space` narrows a growth to `int`
/// (`memory_chunk.h:95-98`): a bulk copy that grows a fresh chunk by 2^31
/// bytes or more allocates too little.
const COPY_LIMIT: u64 = 1 << 31;

#[cfg(test)]
thread_local! {
    static TEST_LIMITS: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}

/// The growth at which a fresh chunk's bulk copy is unsafe in the pin.
pub(crate) fn copy_limit() -> u64 {
    #[cfg(test)]
    if let Some(limit) = TEST_LIMITS.with(std::cell::Cell::get) {
        return limit;
    }
    COPY_LIMIT
}

/// Test-only: replace the copy limit on this thread
/// until the guard drops.
#[cfg(test)]
pub(crate) struct TestLimits;

#[cfg(test)]
impl TestLimits {
    pub(crate) fn set(copy_limit: u64) -> Self {
        TEST_LIMITS.with(|limits| limits.set(Some(copy_limit)));
        Self
    }
}

#[cfg(test)]
impl Drop for TestLimits {
    fn drop(&mut self) {
        TEST_LIMITS.with(|limits| limits.set(None));
    }
}

struct Replacement {
    slot: u32,
    length: usize,
    unigram: u32,
    /// Whether the item the slot names leaves the total first:
    /// `remove_phrase_item` then `add_phrase_item`, or `add_phrase_item`
    /// alone, which only moves the offset word.
    removes_old: bool,
}

/// The session owns item bodies once and keeps pinyin-index membership
/// independently, as the pin's two structures do.
#[derive(Debug, Default)]
pub(crate) struct SessionPhrases {
    pub(crate) libraries: crate::user::LibraryTable<UserLibrary>,
    pub(crate) index: UserIndex,
    /// Explicitly unloaded system and user sub-indices; a successful load
    /// clears the entry. Originals metadata alone does not imply visibility.
    pub(crate) unloaded: crate::user::LibrarySet,
    pub(crate) phrase_table: UserPhraseTable,
}

impl SessionPhrases {
    /// The arena of `nibble`, created if it is absent. A library with no
    /// arena keeps the total its chunk stored (`phrase_index.cpp:237-248`
    /// reads `m_total_freq` at load, items or none) as its retained offset;
    /// the new arena starts from it, since its own total is authoritative
    /// from then on and every later add and overflow check begins there.
    ///
    /// `None` for an index that names no library.
    pub(crate) fn arena(&mut self, nibble: u8, retained_total: u32) -> Option<&mut UserLibrary> {
        self.libraries
            .get_or_insert_with(nibble, || UserLibrary::empty_with_total(retained_total))
    }

    pub(crate) fn item(&self, token: u32) -> Option<PhraseItemView<'_>> {
        self.libraries
            .get(&crate::vocab::phrase_index_library_index(token))?
            .item(token & crate::vocab::PHRASE_MASK)
    }

    pub(crate) fn item_mut(&mut self, token: u32) -> Option<&mut UserLibrary> {
        self.libraries
            .get_mut(&crate::vocab::phrase_index_library_index(token))
    }

    pub(crate) fn phrase(&self, token: u32, item: PhraseItemView<'_>) -> crate::user::UserPhrase {
        let text = item.as_bytes()[ITEM_HEADER_SIZE..ITEM_HEADER_SIZE + 4 * item.phrase_length()]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let pronunciations = item
            .pronunciations()
            .filter_map(|reading| {
                let packed: Vec<_> = reading
                    .keys
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes(*b))
                    .collect();
                let keys = crate::user::store_libpinyin::packed_to_pinyin_keys(&packed)?;
                Some(crate::user::UserPronunciation::new(
                    keys,
                    u64::from(reading.freq),
                    self.index.contains_reading(token, &packed),
                ))
            })
            .collect();
        crate::user::UserPhrase::new(token, text, pronunciations)
    }
}

impl UserLibrary {
    pub(crate) fn empty_with_total(total: u32) -> Self {
        Self {
            total,
            ..Self::default()
        }
    }

    pub(crate) fn token_end(&self) -> u32 {
        self.slots
            .last_key_value()
            .map_or(1, |(&last, _)| last.saturating_add(1))
    }

    /// Replace one body: the pin's remove and add (`pinyin.cpp:563-605`).
    pub(crate) fn replace(&mut self, slot: u32, bytes: &[u8]) -> Option<()> {
        let change = self.replacement(slot, bytes)?;
        self.apply_replacement(change, bytes);
        Some(())
    }

    /// `add_phrase_item` alone, as `_add_phrase`'s new-phrase branch
    /// (`pinyin.cpp:597-603` at 074a2219) and the addon promotion
    /// (`:2546-2556`) call it: the slot's offset word is overwritten and an
    /// item it named keeps its bytes and its share of `m_total_freq`
    /// (`phrase_index.cpp:208-217`). A full library wraps its allocation
    /// onto a live slot this way.
    ///
    /// Only a malformed body or a slot past `PHRASE_MASK` refuses, before
    /// anything changes. Both callers pass a slot masked
    /// by `PHRASE_MASK` and the bytes `encode_phrase_item` made, which
    /// `PhraseItemView::from_bytes` accepts, so neither refusal reaches them.
    pub(crate) fn add(&mut self, slot: u32, bytes: &[u8]) -> Option<()> {
        let mut change = self.replacement(slot, bytes)?;
        change.removes_old = false;
        self.apply_replacement(change, bytes);
        Some(())
    }

    fn replacement(&self, slot: u32, bytes: &[u8]) -> Option<Replacement> {
        if slot > crate::vocab::PHRASE_MASK {
            return None;
        }
        let view = PhraseItemView::from_bytes(bytes)?;
        Some(Self::replacement_for(slot, view))
    }

    fn replacement_for(slot: u32, view: PhraseItemView<'_>) -> Replacement {
        Replacement {
            slot,
            length: view.as_bytes().len(),
            unigram: view.unigram(),
            removes_old: true,
        }
    }

    /// Append the new body and assign that extent to the slot.
    fn apply_replacement(&mut self, change: Replacement, bytes: &[u8]) {
        let slot = change.slot;
        let bytes = &bytes[..change.length];
        if change.removes_old
            && let Some(unigram) = self.item(slot).map(|item| item.unigram())
        {
            self.slots.remove(&slot);
            self.total = self.total.wrapping_sub(unigram);
        }
        if self.content.len() < FIRST_ITEM_OFFSET as usize {
            self.content.resize(FIRST_ITEM_OFFSET as usize, 0);
        }
        let start = self.content.len();
        self.content.extend_from_slice(bytes);
        self.slots.insert(slot, start..self.content.len());
        self.total = self.total.wrapping_add(change.unigram);
    }

    fn remove_body(&mut self, slot: u32) -> bool {
        let Some(unigram) = self.item(slot).map(|item| item.unigram()) else {
            return false;
        };
        self.slots.remove(&slot);
        self.total = self.total.wrapping_sub(unigram);
        true
    }

    /// `remove_phrase_item` alone: the pin never compacts here.
    pub(crate) fn remove(&mut self, slot: u32) -> bool {
        self.remove_body(slot)
    }

    pub(crate) fn remove_matching(&mut self, selected: impl Fn(u32) -> bool) {
        self.slots.retain(|&slot, range| {
            let Some(item) = self
                .content
                .get(range.clone())
                .and_then(PhraseItemView::from_bytes)
            else {
                return true;
            };
            if !selected(slot) {
                return true;
            }
            self.total = self.total.wrapping_sub(item.unigram());
            false
        });
    }

    #[cfg(test)]
    pub(crate) fn add_unigram(&mut self, slot: u32, delta: u32) -> bool {
        if self.total.checked_add(delta).is_none() {
            return false;
        }
        self.increment_unigram(slot, delta)
    }

    /// The caller has checked the whole sub-index total, including mapped
    /// system items when this arena holds only appended slots.
    pub(crate) fn increment_unigram(&mut self, slot: u32, delta: u32) -> bool {
        let Some(item) = self.item(slot) else {
            return false;
        };
        let unigram = item.unigram().wrapping_add(delta);
        let Some(start) = self.editable_start(slot) else {
            return false;
        };
        self.content[start + 2..start + ITEM_HEADER_SIZE].copy_from_slice(&unigram.to_le_bytes());
        self.total = self.total.wrapping_add(delta);
        true
    }

    /// `_add_phrase` on a phrase the library already holds
    /// (`pinyin.cpp:563-582` at 074a2219): `remove_phrase_item` copies the
    /// body out and zeroes the slot, `add_pronunciation` edits the copy, and
    /// `add_phrase_item` stores the copy at the content's end and points
    /// the slot at it (`phrase_index.cpp:56-93`, `:208-235`). The old bytes
    /// stay until compaction, and the item is added back even when the count
    /// is refused.
    pub(crate) fn add_reading(&mut self, slot: u32, keys: &[u16], delta: u32) -> Option<()> {
        let item = self.item(slot)?;
        let bytes = Self::merged_reading(item, keys, delta)?;
        self.replace(slot, &bytes)
    }

    /// `add_reading`'s result for an item held elsewhere: the item's bytes
    /// after `keys` take `delta`, for a caller that writes them back itself.
    /// An unchanged item answers its own bytes.
    pub(crate) fn merged_reading(
        item: PhraseItemView<'_>,
        keys: &[u16],
        delta: u32,
    ) -> Option<Vec<u8>> {
        if item.phrase_length() != keys.len() {
            return None;
        }
        let mut bytes = item.as_bytes().to_vec();
        let mut total = 0_u32;
        for (i, reading) in item.pronunciations().enumerate() {
            total = total.wrapping_add(reading.freq);
            let matches = reading
                .keys
                .as_chunks::<2>()
                .0
                .iter()
                .zip(keys)
                .all(|(bytes, &key)| u16::from_le_bytes(*bytes) & 0x7fff == key & 0x7fff);
            if matches {
                if total.checked_add(delta).is_some() {
                    let keys_len = CHEWING_KEY_SIZE * keys.len();
                    let position =
                        ITEM_HEADER_SIZE + 4 * keys.len() + i * (keys_len + 4) + keys_len;
                    bytes[position..position + 4]
                        .copy_from_slice(&reading.freq.wrapping_add(delta).to_le_bytes());
                }
                return Some(bytes);
            }
        }
        bytes[1] = bytes[1].wrapping_add(1);
        if bytes[1] == 0 {
            // The next get_phrase_item sizes the body from the wrapped header
            // (074a2219 phrase_index.cpp:195-204); no reading is then visible.
            bytes.truncate(ITEM_HEADER_SIZE + keys.len() * 4);
        } else {
            for key in keys {
                bytes.extend_from_slice(&key.to_le_bytes());
            }
            bytes.extend_from_slice(&delta.to_le_bytes());
        }
        Some(bytes)
    }

    pub(crate) fn train(
        &mut self,
        slot: u32,
        readings: impl Iterator<Item = Vec<crate::parser::ChewingKey>>,
        delta: u32,
    ) {
        let Some(item) = self.item(slot) else { return };
        let length = item.as_bytes().len();
        let Some(start) = self.editable_start(slot) else {
            return;
        };
        train_readings(&mut self.content[start..start + length], readings, delta);
    }

    /// `SubPhraseIndex::compact` (`phrase_index.cpp:624-675`): a fresh
    /// sub-index takes every readable slot of the range in slot order
    /// through the same add, rebuilding the total from the items copied. The range begins at slot 1
    /// (`get_range`, `:624-645`), so an item at the reserved slot 0 is not
    /// copied. The pin does this only at end-import, save and mask-out.
    pub(crate) fn compact(&mut self) {
        *self = self.compacted();
    }

    fn compacted(&self) -> Self {
        let mut compacted = Self::default();
        for (slot, item) in self.items().filter(|&(slot, _)| slot != 0) {
            let bytes = item.as_bytes();
            let change = Self::replacement_for(slot, item);
            compacted.apply_replacement(change, bytes);
        }
        compacted
    }

    #[cfg(test)]
    pub(crate) fn load(file: &[u8]) -> Option<Self> {
        Self::load_payload(crate::data::user_files::read_chunk_payload(file)?)
    }

    /// Load each slot with an extent bounded by the next distinct offset.
    pub(crate) fn load_payload(payload: &[u8]) -> Option<Self> {
        let (total, offsets, content) = sub_phrase_index_sections(payload)?;
        let offsets: Vec<_> = offsets
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
            .filter_map(|(slot, bytes)| {
                let start = u32::from_le_bytes(*bytes) as usize;
                (start != 0).then_some((slot as u32, start))
            })
            .collect();
        let mut starts: Vec<_> = offsets.iter().map(|&(_, start)| start).collect();
        starts.sort_unstable();
        let shared_starts = starts
            .windows(2)
            .filter_map(|pair| (pair[0] == pair[1]).then_some(pair[0]))
            .collect();
        starts.dedup();

        let content_len = content.len();
        let slots = offsets
            .into_iter()
            .filter_map(|(slot, start)| {
                let index = starts.binary_search(&start).ok()?;
                let end = starts
                    .get(index + 1)
                    .copied()
                    .unwrap_or(content_len)
                    .min(content_len);
                Some((slot, start.min(end)..end))
            })
            .collect();
        Some(Self {
            total,
            slots,
            content: content.to_vec(),
            shared_starts,
        })
    }

    pub(crate) fn pronunciation_count(&self, slot: u32) -> Option<u32> {
        let range = self.slots.get(&slot)?.clone();
        self.content.get(range)?.get(1).copied().map(u32::from)
    }

    pub(crate) const fn total(&self) -> u32 {
        self.total
    }
    /// The content size `m_phrase_content.size()`.
    #[cfg(test)]
    pub(crate) fn content_len(&self) -> usize {
        self.content.len()
    }

    /// Whether no slot names an item: the pin's emptied sub-index.
    pub(crate) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub(crate) fn item(&self, slot: u32) -> Option<PhraseItemView<'_>> {
        let range = self.slots.get(&slot)?.clone();
        PhraseItemView::from_bytes(self.content.get(range)?)
    }

    /// Slots with duplicate on-disk offsets read the same original body.
    /// Copy a body before changing one slot so the other slots keep it.
    fn editable_start(&mut self, slot: u32) -> Option<usize> {
        let start = self.slots.get(&slot)?.start;
        if !self.shared_starts.contains(&start) {
            return Some(start);
        }
        let length = self.item(slot)?.as_bytes().len();
        let copy = self.content.len();
        self.content.extend_from_within(start..start + length);
        self.slots.insert(slot, copy..self.content.len());
        Some(copy)
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = (u32, PhraseItemView<'_>)> + '_ {
        self.slots.iter().filter_map(|(&slot, range)| {
            Some((
                slot,
                PhraseItemView::from_bytes(self.content.get(range.clone())?)?,
            ))
        })
    }

    /// The size of the content a compaction would leave: the first eight
    /// bytes and every readable item once. This is the `C` that `store`
    /// copies into a fresh chunk (`phrase_index.cpp:384`).
    pub(crate) fn compacted_content_len(&self) -> usize {
        self.items()
            .fold(FIRST_ITEM_OFFSET as usize, |len, (_, item)| {
                len.saturating_add(item.as_bytes().len())
            })
    }

    /// Whether `store`'s content copy grows a fresh chunk by `C` bytes, and
    /// `ensure_has_space` narrows that growth to `int` (`memory_chunk.h:
    /// 95-98`): the unsafe copy, from 2^31 bytes.
    pub(crate) fn store_copy_is_unsafe(&self) -> bool {
        !self.is_empty() && self.compacted_content_len() as u64 >= copy_limit()
    }

    /// The pin's store layout after compaction (074a2219
    /// phrase_index.cpp:365-390): total, three offsets, then the offset
    /// array and packed bodies, with `#` separators. Copy bodies directly
    /// into the output; no decoded phrases or temporary content arena.
    pub(crate) fn store(&self) -> Option<Vec<u8>> {
        let slots = if self.is_empty() {
            0
        } else {
            self.token_end() as usize
        };
        let index_two = (INDEX_ONE + 1).checked_add(u32::try_from(slots * 4).ok()?)?;
        let content_len = if self.is_empty() {
            0
        } else {
            self.compacted_content_len()
        };
        let index_three = index_two
            .checked_add(u32::try_from(content_len).ok()?)
            .and_then(|end| end.checked_add(1))?;
        // Buffer positions include the file header; stored section offsets do not.
        let entry_start = CHUNK_HEADER_SIZE + index_two as usize;
        let mut file = Vec::with_capacity(index_three as usize + CHUNK_HEADER_SIZE);
        file.resize(entry_start, 0);
        let payload = &mut file[CHUNK_HEADER_SIZE..];
        payload[4..8].copy_from_slice(&INDEX_ONE.to_le_bytes());
        payload[8..12].copy_from_slice(&index_two.to_le_bytes());
        payload[12..16].copy_from_slice(&index_three.to_le_bytes());
        payload[INDEX_ONE as usize - 1] = SEPARATOR;
        payload[index_two as usize - 1] = SEPARATOR;
        if !self.is_empty() {
            file.resize(file.len() + FIRST_ITEM_OFFSET as usize, 0);
        }
        // `m_total_freq` is a `guint32` that `add_phrase_item` adds to with
        // no overflow check (`phrase_index.cpp:208-217`), and `store` writes
        // it as it stands (`:367`): a journal ADD can wrap it, so the file
        // holds the wrapped sum of the items a compaction copied.
        let mut total = 0_u32;
        for (slot, view) in self.items() {
            if slot > crate::vocab::PHRASE_MASK {
                return None;
            }
            // Item offsets count from the entry area, including its initial hole.
            let offset = u32::try_from(file.len() - entry_start).ok()?;
            let at = CHUNK_HEADER_SIZE + INDEX_ONE as usize + slot as usize * 4;
            file[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            file.extend_from_slice(view.as_bytes());
            total = total.wrapping_add(view.unigram());
        }
        file[CHUNK_HEADER_SIZE..CHUNK_HEADER_SIZE + 4].copy_from_slice(&total.to_le_bytes());
        file.push(SEPARATOR);
        crate::data::chunk_format::finish_memory_chunk(file)
    }
}

#[cfg(test)]
mod tests;
