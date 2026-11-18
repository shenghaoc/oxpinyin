//! Owned session sub-index: packed item bodies and independent index membership.

use crate::data::chunk_write::sub_phrase_index_sections;
use crate::data::phrase_library::PhraseItemView;

use crate::user::user_index::UserIndex;
use crate::user::user_phrase_table::UserPhraseTable;

/// The pin's SubPhraseIndex (phrase_index.h:257-430 at 074a2219), owned.
#[derive(Clone, Debug, Default)]
pub(crate) struct UserLibrary {
    total: u32,
    slots: Slots,
    content: Vec<u8>,
}

/// `table_offset_t` is `guint32` (`novel_types.h:128` at 074a2219), so a
/// content offset is the content size modulo 2^32.
const OFFSET_MODULUS: u64 = 1 << 32;

/// `MemoryChunk::ensure_has_space` narrows a growth to `int`
/// (`memory_chunk.h:95-98`): a bulk copy that grows a fresh chunk by 2^31
/// bytes or more allocates too little (#1514, register row 112).
const COPY_LIMIT: u64 = 1 << 31;

#[cfg(test)]
thread_local! {
    static TEST_LIMITS: std::cell::Cell<Option<(u64, u64)>> = const { std::cell::Cell::new(None) };
}

/// The modulus of a content offset: 2^32, or a test's small stand-in, so a
/// test reaches the wrap without gigabytes. One code path serves both.
pub(crate) fn offset_modulus() -> u64 {
    #[cfg(test)]
    if let Some((modulus, _)) = TEST_LIMITS.with(std::cell::Cell::get) {
        return modulus;
    }
    OFFSET_MODULUS
}

/// The growth at which a fresh chunk's bulk copy is unsafe in the pin.
pub(crate) fn copy_limit() -> u64 {
    #[cfg(test)]
    if let Some((_, limit)) = TEST_LIMITS.with(std::cell::Cell::get) {
        return limit;
    }
    COPY_LIMIT
}

/// Test-only: replace the offset modulus and the copy limit on this thread
/// until the guard drops.
#[cfg(test)]
pub(crate) struct TestLimits;

#[cfg(test)]
impl TestLimits {
    pub(crate) fn set(modulus: u64, copy_limit: u64) -> Self {
        TEST_LIMITS.with(|limits| limits.set(Some((modulus, copy_limit))));
        Self
    }
}

#[cfg(test)]
impl Drop for TestLimits {
    fn drop(&mut self) {
        TEST_LIMITS.with(|limits| limits.set(None));
    }
}

/// `add_phrase_item`'s `table_offset_t offset = m_phrase_content.size()`
/// (`phrase_index.cpp:208-210`): the size modulo 2^32, and 8 for zero.
pub(crate) fn append_offset(content_len: usize) -> u32 {
    match (content_len as u64 % offset_modulus()) as u32 {
        0 => 8,
        offset => offset,
    }
}

struct Replacement {
    slot: u32,
    offset: u32,
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
            .get(&crate::user::phrase_index_library_index(token))?
            .item(token & crate::user::PHRASE_MASK)
    }

    pub(crate) fn item_mut(&mut self, token: u32) -> Option<&mut UserLibrary> {
        self.libraries
            .get_mut(&crate::user::phrase_index_library_index(token))
    }

    pub(crate) fn phrase(&self, token: u32, item: PhraseItemView<'_>) -> crate::user::UserPhrase {
        let text = item.as_bytes()[6..6 + 4 * item.phrase_length()]
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

/// One offset word per slot. Zero marks an empty slot
/// (phrase_index.cpp:180–193, 208–217 and 230–232 at 074a2219).
#[derive(Clone, Debug, Default)]
struct Slots(Vec<u32>);

impl Slots {
    fn set(&mut self, slot: u32, offset: u32) {
        let slot = slot as usize;
        if offset != 0 && slot >= self.0.len() {
            self.0.resize(slot + 1, 0);
        }
        if let Some(value) = self.0.get_mut(slot) {
            *value = offset;
        }
    }

    fn offset(&self, slot: u32) -> Option<u32> {
        self.0
            .get(slot as usize)
            .copied()
            .filter(|offset| *offset != 0)
    }

    fn entries(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.0
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, offset)| *offset != 0)
            .map(|(slot, offset)| (slot as u32, offset))
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
            .0
            .iter()
            .rposition(|offset| *offset != 0)
            .map_or(1, |last| (last as u32).saturating_add(1))
    }

    /// Replace one body: the pin's remove and add (`pinyin.cpp:563-605`).
    pub(crate) fn replace(&mut self, slot: u32, bytes: &[u8]) -> Result<(), String> {
        let change = self.replacement(slot, bytes)?;
        self.apply_replacement(change, bytes);
        Ok(())
    }

    /// `add_phrase_item` alone, as `_add_phrase`'s new-phrase branch
    /// (`pinyin.cpp:597-603` at 074a2219) and the addon promotion
    /// (`:2546-2556`) call it: the slot's offset word is overwritten and an
    /// item it named keeps its bytes and its share of `m_total_freq`
    /// (`phrase_index.cpp:208-217`). A full library wraps its allocation
    /// onto a live slot this way (register row 119).
    ///
    /// Only a malformed body or a slot past `PHRASE_MASK` refuses, before
    /// anything changes; the offset wraps. Both callers pass a slot masked
    /// by `PHRASE_MASK` and the bytes `encode_phrase_item` made, which
    /// `validate_phrase_item` accepts, so neither refusal reaches them.
    pub(crate) fn add(&mut self, slot: u32, bytes: &[u8]) -> Result<(), String> {
        let mut change = self.replacement(slot, bytes)?;
        change.removes_old = false;
        self.apply_replacement(change, bytes);
        Ok(())
    }

    fn replacement(&self, slot: u32, bytes: &[u8]) -> Result<Replacement, String> {
        if slot > crate::data::chunk_write::PHRASE_MASK {
            return Err("user phrase slot exceeds PHRASE_MASK".into());
        }
        let view = PhraseItemView::from_bytes(bytes).ok_or("invalid user phrase body")?;
        Ok(self.placement(slot, view))
    }

    /// Where `add_phrase_item` writes `view`: at the content size narrowed
    /// to `table_offset_t`, which is 8 for zero (`phrase_index.cpp:208-217`).
    fn placement(&self, slot: u32, view: PhraseItemView<'_>) -> Replacement {
        Replacement {
            slot,
            offset: append_offset(self.content.len()),
            length: view.as_bytes().len(),
            unigram: view.unigram(),
            removes_old: true,
        }
    }

    /// `remove_phrase_item` then `add_phrase_item` (`phrase_index.cpp:
    /// 208-234`). The write is `set_content` at the wrapped offset: over
    /// earlier bytes without growing the size, or past the end as an append.
    /// Nothing else is adjusted for the bytes it overwrites.
    fn apply_replacement(&mut self, change: Replacement, bytes: &[u8]) {
        let slot = change.slot;
        let bytes = &bytes[..change.length];
        if change.removes_old
            && let Some(unigram) = self.item(slot).map(|item| item.unigram())
        {
            self.slots.set(slot, 0);
            self.total = self.total.wrapping_sub(unigram);
        }
        if self.content.len() < 8 {
            self.content.resize(8, 0);
        }
        let start = change.offset as usize;
        let end = start + bytes.len();
        if end > self.content.len() {
            self.content.resize(end, 0);
        }
        self.content[start..end].copy_from_slice(bytes);
        self.slots.set(slot, change.offset);
        self.total = self.total.wrapping_add(change.unigram);
    }

    fn remove_body(&mut self, slot: u32) -> bool {
        let Some(unigram) = self.item(slot).map(|item| item.unigram()) else {
            return false;
        };
        self.slots.set(slot, 0);
        self.total = self.total.wrapping_sub(unigram);
        true
    }

    /// `remove_phrase_item` alone: the pin never compacts here.
    pub(crate) fn remove(&mut self, slot: u32) -> bool {
        self.remove_body(slot)
    }

    pub(crate) fn remove_matching(&mut self, selected: impl Fn(u32) -> bool) {
        let slots: Vec<_> = self
            .items()
            .filter_map(|(slot, _)| selected(slot).then_some(slot))
            .collect();
        for slot in slots {
            self.remove_body(slot);
        }
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
        let Some(offset) = self.slots.offset(slot) else {
            return false;
        };
        self.content[offset as usize + 2..offset as usize + 6]
            .copy_from_slice(&unigram.to_le_bytes());
        self.total = self.total.wrapping_add(delta);
        true
    }

    /// `_add_phrase` on a phrase the library already holds
    /// (`pinyin.cpp:563-582` at 074a2219): `remove_phrase_item` copies the
    /// body out and zeroes the slot, `add_pronunciation` edits the copy, and
    /// `add_phrase_item` stores the copy at the content's end and points
    /// the slot at it (`phrase_index.cpp:56-93`, `:208-235`). The old bytes
    /// stay as they were, for any other slot that names them, and the item
    /// is added back even when the count is refused.
    pub(crate) fn add_reading(
        &mut self,
        slot: u32,
        keys: &[u16],
        delta: u32,
    ) -> Result<(), String> {
        let item = self.item(slot).ok_or("missing user phrase")?;
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
    ) -> Result<Vec<u8>, String> {
        if item.phrase_length() != keys.len() {
            return Err("user phrase reading length differs".into());
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
                    let position = 6 + 4 * keys.len() + i * (2 * keys.len() + 4) + 2 * keys.len();
                    bytes[position..position + 4]
                        .copy_from_slice(&reading.freq.wrapping_add(delta).to_le_bytes());
                }
                return Ok(bytes);
            }
        }
        bytes[1] = bytes[1].wrapping_add(1);
        if bytes[1] == 0 {
            // The next get_phrase_item sizes the body from the wrapped header
            // (074a2219 phrase_index.cpp:195-204); no reading is then visible.
            bytes.truncate(6 + keys.len() * 4);
        } else {
            for key in keys {
                bytes.extend_from_slice(&key.to_le_bytes());
            }
            bytes.extend_from_slice(&delta.to_le_bytes());
        }
        Ok(bytes)
    }

    pub(crate) fn train(
        &mut self,
        slot: u32,
        readings: impl Iterator<Item = Vec<crate::ChewingKey>>,
        delta: u32,
    ) {
        let Some(offset) = self.slots.offset(slot) else {
            return;
        };
        let Some(item) = self.item(slot) else { return };
        let length = item.phrase_length();
        let count = item.n_pronunciations();
        let start = offset as usize + 6 + 4 * length;
        for query in readings {
            if query.len() != length {
                continue;
            }
            let mut total = 0_u32;
            for i in 0..count {
                let position = start + i * (2 * length + 4);
                let frequency = position + 2 * length;
                let bytes = &self.content[frequency..frequency + 4];
                let freq = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                total = total.wrapping_add(freq);
                let matches = crate::user::store_libpinyin::pronunciation_matches(
                    &query,
                    self.content[position..frequency]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|bytes| u16::from_le_bytes(*bytes)),
                );
                if !matches {
                    continue;
                }
                let Some(next) = total.checked_add(delta) else {
                    break;
                };
                total = next;
                self.content[frequency..frequency + 4]
                    .copy_from_slice(&freq.wrapping_add(delta).to_le_bytes());
            }
        }
    }

    /// `SubPhraseIndex::compact` (`phrase_index.cpp:624-675`): a fresh
    /// sub-index takes every readable slot of the range in slot order
    /// through the same add, so its content can wrap in turn and its total
    /// is the sum of the items copied. The range begins at slot 1
    /// (`get_range`, `:624-645`), so an item at the reserved slot 0 is not
    /// copied. The pin does this only at end-import, save and mask-out.
    pub(crate) fn compact(&mut self) {
        *self = self.compacted();
    }

    fn compacted(&self) -> Self {
        let mut compacted = Self::default();
        for (slot, item) in self.items().filter(|&(slot, _)| slot != 0) {
            let bytes = item.as_bytes();
            let change = compacted.placement(slot, item);
            compacted.apply_replacement(change, bytes);
        }
        compacted
    }

    #[cfg(test)]
    pub(crate) fn load(file: &[u8]) -> Result<Self, String> {
        let payload =
            crate::data::user_files::read_chunk_payload(file).map_err(|e| e.to_string())?;
        Self::load_payload(payload)
    }

    /// Load the sections and stored total. The pin reads individual offsets
    /// and headers only in `get_phrase_item` (074a2219
    /// `phrase_index.cpp:180-205,335-362`).
    pub(crate) fn load_payload(payload: &[u8]) -> Result<Self, String> {
        let (total, offsets, content) =
            sub_phrase_index_sections(payload).map_err(|e| e.to_string())?;
        let entries = offsets
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b));
        let slots = Slots(entries.collect());
        Ok(Self {
            total,
            slots,
            content: content.to_vec(),
        })
    }

    pub(crate) fn pronunciation_count(&self, slot: u32) -> Option<u32> {
        let offset = self.slots.offset(slot)? as usize;
        self.content.get(offset..)?.get(1).copied().map(u32::from)
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
        self.slots.entries().next().is_none()
    }

    pub(crate) fn item(&self, slot: u32) -> Option<PhraseItemView<'_>> {
        let offset = self.slots.offset(slot)?;
        PhraseItemView::from_bytes(self.content.get(offset as usize..)?)
    }

    pub(crate) fn items(&self) -> impl Iterator<Item = (u32, PhraseItemView<'_>)> + '_ {
        self.slots
            .entries()
            .filter_map(|(slot, _)| Some((slot, self.item(slot)?)))
    }

    /// The size of the content a compaction would leave: the first eight
    /// bytes and every readable item once. This is the `C` that `store`
    /// copies into a fresh chunk (`phrase_index.cpp:384`).
    pub(crate) fn compacted_content_len(&self) -> usize {
        self.items().fold(8, |len, (_, item)| {
            len.saturating_add(item.as_bytes().len())
        })
    }

    /// Whether `store`'s content copy grows a fresh chunk by `C` bytes, and
    /// `ensure_has_space` narrows that growth to `int` (`memory_chunk.h:
    /// 95-98`): the unsafe copy of register row 112, from 2^31 bytes.
    pub(crate) fn store_copy_is_unsafe(&self) -> bool {
        !self.is_empty() && self.compacted_content_len() as u64 >= copy_limit()
    }

    /// The pin's store layout after compaction (074a2219
    /// phrase_index.cpp:365-390): total, three offsets, then the offset
    /// array and packed bodies, with `#` separators. Copy bodies directly
    /// into the output; no decoded phrases or temporary content arena.
    pub(crate) fn store(&self) -> Result<Vec<u8>, String> {
        let slots = if self.is_empty() {
            0
        } else {
            self.token_end() as usize
        };
        let index_two = 18_u32
            .checked_add(u32::try_from(slots * 4).map_err(|e| e.to_string())?)
            .ok_or("offset array too large")?;
        let content_len = if self.is_empty() {
            0
        } else {
            self.compacted_content_len()
        };
        let index_three = index_two
            .checked_add(u32::try_from(content_len).map_err(|e| e.to_string())?)
            .and_then(|end| end.checked_add(1))
            .ok_or("arena too large")?;
        let mut payload = Vec::with_capacity(index_three as usize + 8);
        payload.resize(index_two as usize, 0);
        payload[4..8].copy_from_slice(&17_u32.to_le_bytes());
        payload[8..12].copy_from_slice(&index_two.to_le_bytes());
        payload[12..16].copy_from_slice(&index_three.to_le_bytes());
        payload[16] = b'#';
        payload[index_two as usize - 1] = b'#';
        if !self.is_empty() {
            payload.resize(payload.len() + 8, 0);
        }
        // `m_total_freq` is a `guint32` that `add_phrase_item` adds to with
        // no overflow check (`phrase_index.cpp:208-217`), and `store` writes
        // it as it stands (`:367`): a journal ADD can wrap it, so the file
        // holds the wrapped sum of the items a compaction copied.
        let mut total = 0_u32;
        for (slot, view) in self.items() {
            if slot > crate::data::chunk_write::PHRASE_MASK {
                return Err(format!(
                    "chunk write error: chunk slot {slot:#010x} exceeds PHRASE_MASK"
                ));
            }
            let offset =
                u32::try_from(payload.len() - index_two as usize).map_err(|e| e.to_string())?;
            let at = 17 + slot as usize * 4;
            payload[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            payload.extend_from_slice(view.as_bytes());
            total = total.wrapping_add(view.unigram());
        }
        payload[..4].copy_from_slice(&total.to_le_bytes());
        payload.push(b'#');
        crate::data::chunk_format::build_memory_chunk_owned(payload).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
