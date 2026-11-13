//! Owned session sub-index: packed item bodies and independent index membership.

use std::collections::{BTreeMap, BTreeSet};

use oxpinyin_data::chunk_write::{sub_phrase_index_sections, validate_phrase_item};
use oxpinyin_data::phrase_library::PhraseItemView;

use crate::user_index::UserIndex;
use crate::user_phrase_table::UserPhraseTable;

/// The pin's SubPhraseIndex (phrase_index.h:257-430 at 074a2219), owned.
#[derive(Clone, Debug, Default)]
pub(crate) struct UserLibrary {
    total: u32,
    slots: Slots,
    content: Vec<u8>,
    headers_only: bool,
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

/// A checked replacement, kept separate from publication of database metadata.
pub(crate) struct PreparedReplacement {
    bytes: Vec<u8>,
    change: Replacement,
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

impl PreparedReplacement {
    /// The caller holds the session's phrase borrow from preparation to apply.
    pub(crate) fn apply(self, library: &mut UserLibrary) {
        library.apply_replacement(self.change, &self.bytes);
    }
}

/// The session owns item bodies once and keeps pinyin-index membership
/// independently, as the pin's two structures do.
#[derive(Debug, Default)]
pub(crate) struct SessionPhrases {
    pub(crate) libraries: BTreeMap<u8, UserLibrary>,
    pub(crate) index: UserIndex,
    /// Explicitly unloaded system and user sub-indices; a successful load
    /// clears the entry. Originals metadata alone does not imply visibility.
    pub(crate) unloaded: BTreeSet<u8>,
    pub(crate) phrase_table: UserPhraseTable,
}

impl SessionPhrases {
    /// The arena of `nibble`, created if it is absent. A library with no
    /// arena keeps the total its chunk stored (`phrase_index.cpp:237-248`
    /// reads `m_total_freq` at load, items or none) as its retained offset;
    /// the new arena starts from it, since its own total is authoritative
    /// from then on and every later add and overflow check begins there.
    pub(crate) fn arena(&mut self, nibble: u8, retained_total: u32) -> &mut UserLibrary {
        self.libraries
            .entry(nibble)
            .or_insert_with(|| UserLibrary::empty_with_total(retained_total))
    }

    pub(crate) fn item(&self, token: u32) -> Option<PhraseItemView<'_>> {
        self.libraries
            .get(&crate::phrase_index_library_index(token))?
            .item(token & crate::PHRASE_MASK)
    }

    pub(crate) fn item_mut(&mut self, token: u32) -> Option<&mut UserLibrary> {
        self.libraries
            .get_mut(&crate::phrase_index_library_index(token))
    }

    pub(crate) fn phrase(&self, token: u32, item: PhraseItemView<'_>) -> crate::UserPhrase {
        let text = item.as_bytes()[6..6 + 4 * item.phrase_length()]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let pronunciations = item
            .pronunciations()
            .filter_map(|reading| {
                let packed: Vec<_> = reading
                    .keys
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                let keys = crate::store_libpinyin::packed_to_pinyin_keys(&packed)?;
                Some(crate::UserPronunciation::new(
                    keys,
                    u64::from(reading.freq),
                    self.index.contains_reading(token, &packed),
                ))
            })
            .collect();
        crate::UserPhrase::new(token, text, pronunciations)
    }
}

#[derive(Clone, Debug)]
enum Slots {
    Dense(Vec<u32>),
    Sparse(BTreeMap<u32, u32>),
}

impl Default for Slots {
    fn default() -> Self {
        Self::Dense(Vec::new())
    }
}

impl Slots {
    fn set(&mut self, slot: u32, offset: u32) {
        if offset != 0
            && let Self::Dense(offsets) = self
            && slot as usize > offsets.len().saturating_mul(4).saturating_add(256)
        {
            *self = Self::Sparse(
                offsets
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(_, offset)| *offset != 0)
                    .map(|(slot, offset)| (slot as u32, offset))
                    .collect(),
            );
        }
        match self {
            Self::Dense(offsets) => {
                if offset != 0 {
                    offsets.resize(offsets.len().max(slot as usize + 1), 0);
                }
                if let Some(value) = offsets.get_mut(slot as usize) {
                    *value = offset;
                }
            }
            Self::Sparse(offsets) => {
                if offset == 0 {
                    offsets.remove(&slot);
                } else {
                    offsets.insert(slot, offset);
                }
            }
        }
    }

    fn offset(&self, slot: u32) -> Option<u32> {
        match self {
            Self::Dense(offsets) => offsets.get(slot as usize).copied(),
            Self::Sparse(offsets) => offsets.get(&slot).copied(),
        }
        .filter(|offset| *offset != 0)
    }

    fn entries(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let (dense, sparse) = match self {
            Self::Dense(offsets) => (Some(offsets), None),
            Self::Sparse(offsets) => (None, Some(offsets)),
        };
        dense
            .into_iter()
            .flat_map(|offsets| {
                offsets
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(_, offset)| *offset != 0)
                    .map(|(slot, offset)| (slot as u32, offset))
            })
            .chain(
                sparse
                    .into_iter()
                    .flat_map(|offsets| offsets.iter().map(|(&s, &o)| (s, o))),
            )
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
        if self.headers_only {
            return 1;
        }
        match &self.slots {
            Slots::Dense(offsets) => offsets
                .iter()
                .rposition(|offset| *offset != 0)
                .map(|i| i as u32),
            Slots::Sparse(offsets) => offsets.last_key_value().map(|(&slot, _)| slot),
        }
        .map_or(1, |last| last.saturating_add(1).max(1))
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
    /// Finish every fallible check before the caller commits its tables.
    /// Those pin sites publish indexes then append unconditionally. Only a
    /// malformed body or a slot past `PHRASE_MASK` can refuse; the offset
    /// wraps. Applying the prepared bytes has no recoverable failure.
    pub(crate) fn prepare_add(
        &self,
        slot: u32,
        bytes: Vec<u8>,
    ) -> Result<PreparedReplacement, String> {
        let mut change = self.replacement(slot, &bytes)?;
        change.removes_old = false;
        Ok(PreparedReplacement { bytes, change })
    }

    fn replacement(&self, slot: u32, bytes: &[u8]) -> Result<Replacement, String> {
        if slot > oxpinyin_data::chunk_write::PHRASE_MASK {
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
            offset: if self.headers_only {
                8
            } else {
                append_offset(self.content.len())
            },
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
        if self.headers_only {
            *self = Self::default();
        }
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

    pub(crate) fn add_reading(
        &mut self,
        slot: u32,
        keys: &[u16],
        delta: u32,
    ) -> Result<(), String> {
        let item = self.item(slot).ok_or("missing user phrase")?;
        if item.phrase_length() != keys.len() {
            return Err("user phrase reading length differs".into());
        }
        let mut total = 0_u32;
        let mut matched = None;
        for (i, reading) in item.pronunciations().enumerate() {
            total = total.wrapping_add(reading.freq);
            if reading
                .keys
                .as_chunks::<2>()
                .0
                .iter()
                .zip(keys)
                .all(|(bytes, &key)| {
                    u16::from_le_bytes([bytes[0], bytes[1]]) & 0x7fff == key & 0x7fff
                })
            {
                matched = Some((i, reading.freq, total.checked_add(delta).is_some()));
                break;
            }
        }
        if let Some((i, frequency, accepted)) = matched {
            if accepted {
                let position = self.slots.offset(slot).ok_or("missing user phrase")? as usize
                    + 6
                    + 4 * keys.len()
                    + i * (2 * keys.len() + 4)
                    + 2 * keys.len();
                self.content[position..position + 4]
                    .copy_from_slice(&frequency.wrapping_add(delta).to_le_bytes());
            }
            return Ok(());
        }
        let mut bytes = item.as_bytes().to_vec();
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
                .all(|(bytes, &key)| {
                    u16::from_le_bytes([bytes[0], bytes[1]]) & 0x7fff == key & 0x7fff
                });
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
        readings: impl Iterator<Item = Vec<oxpinyin_core::ChewingKey>>,
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
                let matches = crate::store_libpinyin::pronunciation_matches(
                    &query,
                    self.content[position..frequency]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]])),
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
    /// sub-index takes every readable slot in slot order through the same
    /// add, so its content can wrap in turn and its total is the sum of the
    /// items copied. The pin does this only at end-import, save and mask-out.
    pub(crate) fn compact(&mut self) {
        *self = self.compacted();
    }

    fn compacted(&self) -> Self {
        let count = self.slots.entries().count();
        let mut compacted = Self::default();
        if self.token_end() as usize > count.saturating_mul(4).saturating_add(256) {
            compacted.slots = Slots::Sparse(BTreeMap::new());
        }
        for (slot, item) in self.items() {
            let bytes = item.as_bytes();
            let change = compacted.placement(slot, item);
            compacted.apply_replacement(change, bytes);
        }
        compacted
    }

    #[cfg(test)]
    pub(crate) fn load(file: &[u8]) -> Result<Self, String> {
        let payload =
            oxpinyin_data::user_files::read_chunk_payload(file).map_err(|e| e.to_string())?;
        Self::load_payload(payload)
    }

    pub(crate) fn load_payload(payload: &[u8]) -> Result<Self, String> {
        let (total, offsets, content) =
            sub_phrase_index_sections(payload).map_err(|e| e.to_string())?;
        let mut count = 0_usize;
        // Retain the old decoder's all-or-nothing validation, including slot
        // zero, partial trailing offset words, and arbitrary text/key values.
        for (slot, bytes) in offsets.as_chunks::<4>().0.iter().enumerate() {
            let offset = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            if offset == 0 {
                continue;
            }
            let bytes = content.get(offset as usize..).ok_or_else(|| {
                format!("chunk write error: sub phrase index: slot {slot} offset out of range")
            })?;
            validate_phrase_item(bytes).map_err(|error| {
                format!("chunk write error: sub phrase index: slot {slot}: {error}")
            })?;
            if let Some(view) = PhraseItemView::from_bytes(bytes) {
                let _ = view;
                count += 1;
            }
        }
        let entries = offsets
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        let slots = if offsets.len() / 4 <= count.saturating_mul(4).saturating_add(256) {
            Slots::Dense(entries.collect())
        } else {
            Slots::Sparse(
                entries
                    .enumerate()
                    .filter(|(_, offset)| *offset != 0)
                    .map(|(slot, offset)| (slot as u32, offset))
                    .collect(),
            )
        };
        Ok(Self {
            total,
            slots,
            content: content.to_vec(),
            headers_only: false,
        })
    }

    /// A rejected body can still have the two bytes the pin's count getter
    /// reads (phrase_index.cpp:195-204). Retain those bytes, not a count map.
    pub(crate) fn load_headers_payload(payload: &[u8]) -> Result<Self, String> {
        let (_, offsets, content) =
            sub_phrase_index_sections(payload).map_err(|e| e.to_string())?;
        Ok(Self {
            total: 0,
            slots: Slots::Dense(
                offsets
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect(),
            ),
            content: content.to_vec(),
            headers_only: true,
        })
    }

    pub(crate) fn pronunciation_count(&self, slot: u32) -> Option<u32> {
        let offset = self.slots.offset(slot)? as usize;
        self.content.get(offset + 1).map(|&n| u32::from(n))
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
        if self.headers_only {
            return None;
        }
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
            if slot > oxpinyin_data::chunk_write::PHRASE_MASK {
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
        oxpinyin_data::chunk_format::build_memory_chunk_owned(payload).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk, encode_phrase_item};
    use oxpinyin_data::user_files::read_chunk_payload;

    fn items() -> Vec<(u32, ChunkItem)> {
        vec![
            (
                1,
                ChunkItem {
                    phrase: vec![0x4f60],
                    unigram: 3,
                    prons: vec![(vec![0x1234], 7)],
                },
            ),
            (
                3,
                ChunkItem {
                    phrase: vec![0x597d, 0],
                    unigram: 5,
                    prons: vec![(vec![0xffff, 0], 11), (vec![0, 0xffff], 2)],
                },
            ),
        ]
    }

    /// Hand-built USER_FILE: slot 3 is present even with no phrase words;
    /// the two empty-key readings retain their order and frequencies.
    #[test]
    fn persisted_empty_user_item_survives_load_compact_and_store() {
        let body = [0, 2, 37, 0, 0, 0, 13, 0, 0, 0, 5, 0, 0, 0];
        let file = |body: &[u8]| {
            let mut payload = Vec::new();
            for word in [91_u32, 17, 34, 34 + 8 + body.len() as u32 + 1] {
                payload.extend_from_slice(&word.to_le_bytes());
            }
            payload.push(b'#');
            for offset in [0_u32, 0, 0, 8] {
                payload.extend_from_slice(&offset.to_le_bytes());
            }
            payload.push(b'#');
            payload.extend_from_slice(&[0; 8]);
            payload.extend_from_slice(body);
            payload.push(b'#');
            build_memory_chunk(&payload).unwrap()
        };
        let mut library = UserLibrary::load(&file(&body)).unwrap();
        assert_eq!(library.total(), 91, "load retains the header total");
        assert!(!library.is_empty());
        assert!(library.item(1).is_none());
        assert_eq!(library.item(3).unwrap().as_bytes(), body);
        library.compact();
        assert_eq!(library.total(), 37, "compact rebuilds the item sum");
        let saved = library.store().unwrap();
        let loaded = UserLibrary::load(&saved).unwrap();
        assert_eq!(loaded.total(), 37);
        assert_eq!(
            loaded.items().map(|(slot, _)| slot).collect::<Vec<_>>(),
            [3]
        );
        assert_eq!(loaded.item(3).unwrap().as_bytes(), body);
        assert_eq!(saved, loaded.store().unwrap());

        // A missing final pronunciation-frequency byte remains a structural
        // refusal; an empty phrase does not relax the declared body extent.
        assert!(UserLibrary::load(&file(&body[..body.len() - 1])).is_err());
        assert!(library.replace(4, &body[..body.len() - 1]).is_err());
        assert_eq!(library.store().unwrap(), saved);
    }

    /// 074a2219 phrase_index.cpp:56-93, :150-178, :219-235,
    /// :624-675: ordered readings, prefix overflow, sparse token reuse,
    /// and compaction produce the same item bytes and offset layout.
    #[test]
    fn mutations_compact_to_the_pins_item_and_store_layout() {
        let mut library = UserLibrary::load(&build_chunk(&items()).unwrap()).unwrap();
        let mut expected = items();
        library.add_reading(1, &[0x1234], 4).unwrap();
        expected[0].1.prons[0].1 += 4;
        library.add_reading(1, &[0x2234], 9).unwrap();
        expected[0].1.prons.push((vec![0x2234], 9));
        assert!(library.add_unigram(1, 11));
        expected[0].1.unigram += 11;
        assert!(library.remove(3));
        expected.pop();
        assert_eq!(library.token_end(), 2);
        library.compact();
        assert_eq!(library.store().unwrap(), build_chunk(&expected).unwrap());
        assert_eq!(library.content.len(), library.compacted_content_len());
    }

    #[test]
    fn exact_reading_overflow_and_count_wrap_follow_the_pin() {
        let mut item = items().remove(0).1;
        item.prons = vec![(vec![0x1234], u32::MAX), (vec![0x2234], 1)];
        let mut library = UserLibrary::load(&build_chunk(&[(1, item)]).unwrap()).unwrap();
        library.add_reading(1, &[0x1234], 1).unwrap();
        assert_eq!(
            library.item(1).unwrap().pronunciation(0).unwrap().freq,
            u32::MAX
        );
        library.add_reading(1, &[0x2234], 1).unwrap();
        assert_eq!(library.item(1).unwrap().pronunciation(1).unwrap().freq, 2);
        for key in 1..=253 {
            library.add_reading(1, &[key], 1).unwrap();
        }
        assert_eq!(library.item(1).unwrap().n_pronunciations(), 255);
        library.add_reading(1, &[254], 1).unwrap();
        assert_eq!(library.item(1).unwrap().n_pronunciations(), 0);
        library.add_reading(1, &[255], 2).unwrap();
        let view = library.item(1).unwrap();
        assert_eq!(view.n_pronunciations(), 1);
        assert_eq!(view.pronunciation(0).unwrap().freq, 2);
    }

    #[test]
    fn user_library_store_matches_build_chunk_in_both_slot_representations() {
        for slot in [3, 1024] {
            let mut items = items();
            items[1].0 = slot;
            let file = build_chunk(&items).unwrap();
            let library = UserLibrary::load(&file).unwrap();

            assert_eq!(library.total(), 8, "the two item unigrams sum to 8");
            assert_eq!(
                library.items().map(|(slot, _)| slot).collect::<Vec<_>>(),
                [1, slot],
                "the loaded slots"
            );
            assert_eq!(library.item(1).unwrap().n_pronunciations(), 1);
            assert_eq!(
                library.item(slot).unwrap().as_bytes(),
                encode_phrase_item(&items[1].1).unwrap(),
                "the loaded item bytes"
            );
            assert_eq!(library.item(slot).unwrap().n_pronunciations(), 2);
            assert_eq!(matches!(library.slots, Slots::Dense(_)), slot == 3);
            assert_eq!(
                library.store().unwrap(),
                file,
                "stored bytes equal the file"
            );
        }

        let file = build_chunk(&[]).unwrap();
        let library = UserLibrary::load(&file).unwrap();
        assert!(library.is_empty());
        assert_eq!(library.store().unwrap(), file);
        assert!(UserLibrary::default().content.is_empty());
    }

    fn assert_no_panic(file: &[u8]) {
        // The production loader must answer without panicking for every truncation.
        let _ = UserLibrary::load(file);
    }

    #[test]
    fn user_library_malformed_frames_match_main_without_panicking() {
        let file = build_chunk(&items()).unwrap();
        for cut in 0..=file.len() {
            assert_no_panic(&file[..cut]);
        }
        let payload = read_chunk_payload(&file).unwrap();
        for cut in 0..=payload.len() {
            assert_no_panic(&build_memory_chunk(&payload[..cut]).unwrap());
        }
        for offset in [1_u32, 7, 8, 1024, u32::MAX] {
            let mut bytes = payload.to_vec();
            bytes[21..25].copy_from_slice(&offset.to_le_bytes());
            assert_no_panic(&build_memory_chunk(&bytes).unwrap());
        }
    }

    /// A one-character, one-reading item: sixteen bytes.
    fn small(unigram: u32) -> Vec<u8> {
        oxpinyin_data::chunk_write::encode_phrase_item(&ChunkItem {
            phrase: vec![0x4e00 + unigram],
            unigram,
            prons: vec![(vec![0x1234], unigram + 100)],
        })
        .unwrap()
    }

    fn fill(library: &mut UserLibrary, unigrams: &[u32]) {
        for (slot, &unigram) in (1..).zip(unigrams) {
            library.replace(slot, &small(unigram)).unwrap();
        }
    }

    /// #1514 ruling 1 (`phrase_index.cpp:208-217`): once the size passes the
    /// modulus the next add lands at the size modulo it, over earlier bytes,
    /// and the size stays. The slot stores the wrapped offset and the total
    /// takes only the added unigram.
    #[test]
    fn an_add_past_the_boundary_overwrites_earlier_bytes_and_aliases_slots() {
        let _limits = TestLimits::set(64, 1 << 31);
        let mut library = UserLibrary::default();
        fill(&mut library, &[1, 2, 3, 4]);
        // 8 + 4 * 16 = 72: past 64, nothing wrapped yet.
        assert_eq!(library.content.len(), 72);
        assert_eq!(library.slots.offset(4), Some(56));
        assert_eq!(library.total(), 10);
        library.replace(5, &small(50)).unwrap();
        // 72 mod 64 = 8: over slot 1's item, size unchanged.
        assert_eq!(library.content.len(), 72);
        assert_eq!(library.slots.offset(5), Some(8));
        assert_eq!(library.slots.offset(1), Some(8), "slot 1 keeps its offset");
        assert_eq!(library.item(1).unwrap().unigram(), 50, "and reads slot 5's");
        assert_eq!(library.item(1).unwrap().as_bytes(), small(50));
        // The total takes +50 and loses nothing for slot 1's changed bytes.
        assert_eq!(library.total(), 60);
        assert_eq!(library.item(2).unwrap().unigram(), 2);
        // The same size gives the same offset again: no advancing ring.
        library.replace(6, &small(60)).unwrap();
        assert_eq!(library.slots.offset(6), Some(8));
        assert_eq!(library.item(5).unwrap().unigram(), 60);
        assert_eq!(library.content.len(), 72);
    }

    /// A wrapped allocation lands on a live slot (register row 119):
    /// `add_phrase_item` alone moves the offset word and adds the new
    /// unigram, and the overwritten item's unigram stays in `m_total_freq`
    /// (`phrase_index.cpp:208-217`), where `replace` takes it out first.
    #[test]
    fn an_add_over_a_live_slot_keeps_the_overwritten_unigram_in_the_total() {
        let mut library = UserLibrary::default();
        fill(&mut library, &[1, 2, 3]);
        assert_eq!(library.total(), 6);
        library
            .prepare_add(2, small(20))
            .unwrap()
            .apply(&mut library);
        assert_eq!(library.item(2).unwrap().unigram(), 20);
        assert_eq!(library.total(), 26, "slot 2's old unigram stays");
        assert_eq!(library.items().count(), 3);
        library.replace(2, &small(30)).unwrap();
        assert_eq!(library.total(), 36, "a replace removes first");
    }

    /// Zero narrows to 8, as an empty content does.
    #[test]
    fn a_size_that_is_a_multiple_of_the_modulus_adds_at_eight() {
        let _limits = TestLimits::set(56, 1 << 31);
        let mut library = UserLibrary::default();
        fill(&mut library, &[1, 2, 3]);
        assert_eq!(library.content.len(), 56);
        library.replace(4, &small(40)).unwrap();
        assert_eq!(library.slots.offset(4), Some(8));
        assert_eq!(library.content.len(), 56);
        assert_eq!(append_offset(0), 8);
    }

    /// Remove and modify act on the bytes now stored (`phrase_index.cpp:
    /// 219-234`): the unigram taken out is the overwritten item's.
    #[test]
    fn remove_and_modify_of_an_overwritten_item_use_its_current_bytes() {
        let _limits = TestLimits::set(64, 1 << 31);
        let mut library = UserLibrary::default();
        fill(&mut library, &[1, 2, 3, 4]);
        library.replace(5, &small(50)).unwrap();
        assert_eq!(library.total(), 60);
        // Slot 1 reads slot 5's item: removing it subtracts 50, not 1.
        assert!(library.remove(1));
        assert_eq!(library.total(), 10);
        assert!(library.item(1).is_none());
        assert_eq!(library.item(5).unwrap().unigram(), 50, "slot 5 stays live");
        // Modify slot 5: remove its 50, add 70 at the same wrapped offset.
        library.replace(5, &small(70)).unwrap();
        assert_eq!(library.total(), 30);
        assert_eq!(library.slots.offset(5), Some(8));
        assert_eq!(library.item(5).unwrap().unigram(), 70);
    }

    /// Ruling 2 (row 111): a header-derived length that crosses the item's
    /// original end but stays inside the content returns the following
    /// bytes; one that passes the content end answers no item.
    #[test]
    fn a_read_crossing_the_item_end_returns_the_next_bytes_and_one_past_the_content_is_none() {
        let mut library = UserLibrary::default();
        fill(&mut library, &[1, 2]);
        let offset = library.slots.offset(1).unwrap() as usize;
        // Two readings: 6 + 4 + 2 * 6 = 22 bytes, six of them slot 2's.
        library.content[offset + 1] = 2;
        let view = library.item(1).unwrap();
        assert_eq!(view.as_bytes().len(), 22);
        assert_eq!(&view.as_bytes()[16..], &small(2)[..6]);
        // 255 readings reach past the content end.
        library.content[offset + 1] = 255;
        assert!(library.item(1).is_none());
        assert_eq!(library.item(2).unwrap().unigram(), 2, "others are kept");
    }

    /// Ruling 3: the arena grows with every replacement and compacts only
    /// when asked, in slot order, with a total rebuilt from the items copied.
    #[test]
    fn compaction_runs_only_when_asked_and_wraps_like_any_add() {
        let _limits = TestLimits::set(64, 1 << 31);
        let mut library = UserLibrary::default();
        for _ in 0..20 {
            library.replace(1, &small(1)).unwrap();
        }
        assert!(library.content.len() >= 64, "no dead-over-live trigger");
        fill(&mut library, &[1, 2, 3, 4, 5]);
        let read: Vec<_> = library
            .items()
            .map(|(slot, item)| (slot, item.unigram()))
            .collect();
        library.compact();
        // Four items reach 72 bytes; the fifth wraps to 8 and the size stays.
        assert_eq!(library.content.len(), 72);
        assert_eq!(library.slots.offset(5), Some(8));
        assert_eq!(library.item(1).unwrap().unigram(), read[4].1);
        // The total is the sum of the items read from the source, wrapped
        // or not, and not the sum of what the compacted copy now reads.
        let sum = read.iter().fold(0_u32, |sum, &(_, unigram)| sum + unigram);
        assert_eq!(library.total(), sum);
    }

    /// Row 112: the save copies the compacted content into a fresh chunk;
    /// from the limit on, the copy is unsafe.
    #[test]
    fn the_store_copy_threshold_follows_the_compacted_content() {
        let _limits = TestLimits::set(1 << 32, 8 + 16 * 3);
        let mut library = UserLibrary::default();
        fill(&mut library, &[1, 2]);
        assert!(!library.store_copy_is_unsafe());
        library.replace(3, &small(3)).unwrap();
        assert!(
            library.store_copy_is_unsafe(),
            "8 + 3 * 16 reaches the limit"
        );
        assert!(library.remove(3));
        assert!(!library.store_copy_is_unsafe());
        assert!(!UserLibrary::default().store_copy_is_unsafe());
    }

    /// Corruption by an overwrite is defined and survives a compaction, a
    /// save and a reload as independent copies of the bytes now read.
    #[test]
    fn a_wrapped_state_survives_compaction_store_and_reload() {
        let mut library = UserLibrary::default();
        {
            let _limits = TestLimits::set(64, 1 << 31);
            fill(&mut library, &[1, 2, 3, 4]);
            library.replace(5, &small(50)).unwrap();
        }
        // The modulus is back to 2^32: the compaction does not wrap again.
        library.compact();
        assert_eq!(library.content.len(), 8 + 5 * 16);
        assert_eq!(library.total(), 50 + 2 + 3 + 4 + 50);
        assert_ne!(library.slots.offset(1), library.slots.offset(5));
        let saved = library.store().unwrap();
        let loaded = UserLibrary::load(&saved).unwrap();
        let read: Vec<_> = loaded
            .items()
            .map(|(slot, item)| (slot, item.unigram()))
            .collect();
        assert_eq!(read, [(1, 50), (2, 2), (3, 3), (4, 4), (5, 50)]);
        assert_eq!(loaded.store().unwrap(), saved);
    }

    /// Mask-out removes slots and leaves the compaction to the pin's point.
    #[test]
    fn removing_a_batch_collects_nothing_until_the_compaction() {
        let mut library = UserLibrary::default();
        fill(&mut library, &[1, 2, 3, 4]);
        let size = library.content.len();
        library.remove_matching(|slot| slot != 4);
        assert_eq!(library.content.len(), size);
        assert_eq!(library.total(), 4);
        library.compact();
        assert_eq!(library.content.len(), 8 + 16);
        assert_eq!(library.total(), 4);
    }

    /// `store` writes the pin's `guint32` `m_total_freq`, which a journal ADD
    /// wraps (`phrase_index.cpp:208-217`, `:365-367`): the file carries the
    /// wrapped sum and loads back.
    #[test]
    fn a_total_past_u32_is_stored_wrapped() {
        let mut library = UserLibrary::default();
        library.replace(1, &small(0x8000_0000)).unwrap();
        library.replace(2, &small(0x8000_0005)).unwrap();
        assert_eq!(library.total(), 5);
        let saved = library.store().unwrap();
        assert_eq!(UserLibrary::load(&saved).unwrap().total(), 5);
    }
}
