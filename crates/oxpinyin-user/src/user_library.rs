//! Owned session sub-index: packed item bodies and independent index membership.

use std::collections::{BTreeMap, BTreeSet};

use oxpinyin_data::chunk_write::{sub_phrase_index_sections, validate_phrase_item};
use oxpinyin_data::phrase_library::PhraseItemView;

/// The pin's SubPhraseIndex (phrase_index.h:257-430 at 074a2219), owned.
#[derive(Clone, Debug, Default)]
pub(crate) struct UserLibrary {
    total: u32,
    sum: u32,
    slots: Slots,
    content: Vec<u8>,
    live: usize,
    headers_only: bool,
}

/// The session owns item bodies once and keeps pinyin-index membership
/// independently, as the pin's two structures do.
#[derive(Debug, Default)]
pub(crate) struct SessionPhrases {
    pub(crate) libraries: BTreeMap<u8, UserLibrary>,
    pub(crate) indexed: BTreeSet<(u32, Vec<u16>)>,
}

impl SessionPhrases {
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
            .chunks_exact(4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        let pronunciations = item
            .pronunciations()
            .filter_map(|reading| {
                let packed: Vec<_> = reading
                    .keys
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect();
                let keys = crate::store_libpinyin::packed_to_pinyin_keys(&packed)?;
                Some(crate::UserPronunciation::new(
                    keys,
                    u64::from(reading.freq),
                    self.indexed.contains(&(token, packed)),
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

    /// Replace one body; growth leaves the old body as bounded garbage.
    pub(crate) fn replace(&mut self, slot: u32, bytes: &[u8]) -> Result<(), String> {
        if slot > oxpinyin_data::chunk_write::PHRASE_MASK {
            return Err("user phrase slot exceeds PHRASE_MASK".into());
        }
        let view = PhraseItemView::from_bytes(bytes).ok_or("invalid user phrase body")?;
        if self.headers_only {
            *self = Self::default();
        }
        let bytes = view.as_bytes();
        let offset = u32::try_from(self.content.len().max(8)).map_err(|e| e.to_string())?;
        offset
            .checked_add(u32::try_from(bytes.len()).map_err(|e| e.to_string())?)
            .ok_or("user phrase arena too large")?;
        let old = self
            .item(slot)
            .map(|item| (item.as_bytes().len(), item.unigram()));
        if self.content.len() < 8 {
            self.content.resize(8, 0);
        }
        self.content.extend_from_slice(bytes);
        self.slots.set(slot, offset);
        if let Some((len, unigram)) = old {
            self.live -= len;
            self.total = self.total.wrapping_sub(unigram);
            self.sum = self.sum.wrapping_sub(unigram);
        }
        self.live += bytes.len();
        self.total = self.total.wrapping_add(view.unigram());
        self.sum = self.sum.wrapping_add(view.unigram());
        self.compact_garbage()
    }

    fn remove_body(&mut self, slot: u32) -> bool {
        let Some(item) = self.item(slot) else {
            return false;
        };
        let (len, unigram) = (item.as_bytes().len(), item.unigram());
        self.slots.set(slot, 0);
        self.live -= len;
        self.total = self.total.wrapping_sub(unigram);
        self.sum = self.sum.wrapping_sub(unigram);
        true
    }

    pub(crate) fn remove(&mut self, slot: u32) -> bool {
        let removed = self.remove_body(slot);
        // Existing offsets and bodies fit the same arena; compaction cannot
        // fail its size check after a removal from a valid library.
        if removed {
            let _ = self.compact_garbage();
        }
        removed
    }

    pub(crate) fn remove_matching(&mut self, selected: impl Fn(u32) -> bool) -> Result<(), String> {
        let slots: Vec<_> = self
            .items()
            .filter_map(|(slot, _)| selected(slot).then_some(slot))
            .collect();
        for slot in slots {
            self.remove_body(slot);
        }
        // A mask checks the garbage bound once after removing the batch.
        self.compact_garbage()
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
        self.sum = self.sum.wrapping_add(delta);
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
            if reading.keys.chunks_exact(2).zip(keys).all(|(bytes, &key)| {
                u16::from_le_bytes([bytes[0], bytes[1]]) & 0x7fff == key & 0x7fff
            }) {
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
                let matches = self.content[position..frequency]
                    .chunks_exact(2)
                    .zip(&query)
                    .all(|(bytes, query)| {
                        let stored = oxpinyin_core::ChewingKey::from_packed(u16::from_le_bytes([
                            bytes[0], bytes[1],
                        ]));
                        query.initial == stored.initial
                            && ((query.middle == stored.middle && query.final_ == stored.final_)
                                || (query.middle == 0 && query.final_ == 0)
                                || (stored.middle == 0 && stored.final_ == 0))
                            && (query.tone == stored.tone || query.tone == 0 || stored.tone == 0)
                    });
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

    fn compact_garbage(&mut self) -> Result<(), String> {
        if self
            .content
            .len()
            .saturating_sub(8)
            .saturating_sub(self.live)
            > self.live
        {
            // An extra garbage-bound compaction must retain the loaded total;
            // only the pin's explicit compact resets its bookkeeping.
            let total = self.total;
            self.compact()?;
            self.total = total;
        }
        Ok(())
    }

    pub(crate) fn compact(&mut self) -> Result<(), String> {
        let count = self.slots.entries().count();
        let mut compacted = Self::default();
        if self.token_end() as usize > count.saturating_mul(4).saturating_add(256) {
            compacted.slots = Slots::Sparse(BTreeMap::new());
        }
        compacted.content.reserve(self.live.saturating_add(8));
        for (slot, item) in self.items() {
            compacted.replace(slot, item.as_bytes())?;
        }
        *self = compacted;
        Ok(())
    }

    pub(crate) fn load(file: &[u8]) -> Result<Self, String> {
        let payload =
            oxpinyin_data::user_files::read_chunk_payload(file).map_err(|e| e.to_string())?;
        let (total, offsets, content) =
            sub_phrase_index_sections(payload).map_err(|e| e.to_string())?;
        let mut live = 0_usize;
        let mut count = 0_usize;
        let mut sum = 0_u32;
        // Retain the old decoder's all-or-nothing validation, including slot
        // zero, partial trailing offset words, and arbitrary text/key values.
        for (slot, bytes) in offsets.chunks_exact(4).enumerate() {
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
                live = live.saturating_add(view.as_bytes().len());
                count += 1;
                sum = sum.wrapping_add(view.unigram());
            }
        }
        let entries = offsets
            .chunks_exact(4)
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
            sum,
            slots,
            content: content.to_vec(),
            live,
            headers_only: false,
        })
    }

    /// A rejected body can still have the two bytes the pin's count getter
    /// reads (phrase_index.cpp:195-204). Retain those bytes, not a count map.
    pub(crate) fn load_headers(file: &[u8]) -> Result<Self, String> {
        let payload =
            oxpinyin_data::user_files::read_chunk_payload(file).map_err(|e| e.to_string())?;
        let (_, offsets, content) =
            sub_phrase_index_sections(payload).map_err(|e| e.to_string())?;
        Ok(Self {
            total: 0,
            sum: 0,
            slots: Slots::Dense(
                offsets
                    .chunks_exact(4)
                    .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect(),
            ),
            content: content.to_vec(),
            live: 0,
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
    pub(crate) const fn item_sum(&self) -> u32 {
        self.sum
    }
    pub(crate) const fn is_empty(&self) -> bool {
        self.live == 0
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
            self.live.saturating_add(8)
        };
        let index_three = index_two
            .checked_add(u32::try_from(content_len).map_err(|e| e.to_string())?)
            .and_then(|end| end.checked_add(1))
            .ok_or("arena too large")?;
        let mut payload = Vec::with_capacity(index_three as usize);
        payload.resize(index_two as usize, 0);
        payload[4..8].copy_from_slice(&17_u32.to_le_bytes());
        payload[8..12].copy_from_slice(&index_two.to_le_bytes());
        payload[12..16].copy_from_slice(&index_three.to_le_bytes());
        payload[16] = b'#';
        payload[index_two as usize - 1] = b'#';
        if !self.is_empty() {
            payload.resize(payload.len() + 8, 0);
        }
        let mut total = 0_u64;
        for (slot, view) in self.items() {
            if slot > oxpinyin_data::chunk_write::PHRASE_MASK {
                return Err(format!(
                    "chunk write error: chunk slot {slot:#010x} exceeds PHRASE_MASK"
                ));
            }
            if view.phrase_length() == 0 {
                return Err(format!(
                    "chunk write error: chunk slot {slot:#010x} phrase length 0 out of range"
                ));
            }
            let offset =
                u32::try_from(payload.len() - index_two as usize).map_err(|e| e.to_string())?;
            let at = 17 + slot as usize * 4;
            payload[at..at + 4].copy_from_slice(&offset.to_le_bytes());
            payload.extend_from_slice(view.as_bytes());
            total += u64::from(view.unigram());
        }
        let total = u32::try_from(total)
            .map_err(|_| "chunk write error: chunk total_freq overflows u32".to_owned())?;
        payload[..4].copy_from_slice(&total.to_le_bytes());
        payload.push(b'#');
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_data::chunk_format::build_memory_chunk;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk, decode_sub_phrase_index};
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
        library.compact().unwrap();
        assert_eq!(library.store().unwrap(), build_chunk(&expected).unwrap());
        assert_eq!(library.content.len(), library.live + 8);
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
    fn repeated_replacement_and_removal_bound_arena_garbage() {
        let bytes = oxpinyin_data::chunk_write::encode_phrase_item(&items().remove(0).1).unwrap();
        let mut library = UserLibrary::default();
        for _ in 0..100 {
            library.replace(1024, &bytes).unwrap();
            assert!(library.content.len() <= 2 * library.live + 8);
        }
        library.compact().unwrap();
        assert!(matches!(library.slots, Slots::Sparse(_)));
        assert_eq!(library.token_end(), 1025);
        assert!(library.remove(1024));
        assert!(library.is_empty());
        assert_eq!(library.token_end(), 1);
        assert!(library.content.is_empty());
    }

    fn compare(file: &[u8]) {
        let expected = read_chunk_payload(file)
            .map_err(|e| e.to_string())
            .and_then(|payload| decode_sub_phrase_index(payload).map_err(|e| e.to_string()));
        let actual = UserLibrary::load(file);
        match (expected, actual) {
            (Ok((total, items)), Ok(library)) => {
                assert_eq!(library.total(), total);
                let decoded: Vec<_> = library
                    .items()
                    .map(|(slot, item)| {
                        (
                            slot,
                            oxpinyin_data::chunk_write::decode_phrase_item(item.as_bytes())
                                .unwrap(),
                        )
                    })
                    .collect();
                assert_eq!(decoded, items);
                assert_eq!(
                    library.store(),
                    build_chunk(&items).map_err(|e| e.to_string())
                );
                assert_eq!(
                    library.live,
                    library.items().map(|(_, item)| item.as_bytes().len()).sum()
                );
            }
            (Err(expected), Err(actual)) => assert_eq!(actual, expected),
            (expected, _) => panic!("loader disagrees with existing decoder: {expected:?}"),
        }
    }

    #[test]
    fn user_library_store_matches_build_chunk_in_both_slot_representations() {
        for slot in [3, 1024] {
            let mut items = items();
            items[1].0 = slot;
            let file = build_chunk(&items).unwrap();
            let library = UserLibrary::load(&file).unwrap();
            assert_eq!(library.store().unwrap(), file);
            assert_eq!(matches!(library.slots, Slots::Dense(_)), slot == 3);
            assert!(library.item(2).is_none());
            assert_eq!(library.item(slot).unwrap().n_pronunciations(), 2);
            compare(&file);
        }
        let file = build_chunk(&[]).unwrap();
        let library = UserLibrary::load(&file).unwrap();
        assert!(library.is_empty());
        assert_eq!(library.store().unwrap(), file);
        assert!(UserLibrary::default().content.is_empty());
    }

    #[test]
    fn user_library_malformed_frames_match_main_without_panicking() {
        let file = build_chunk(&items()).unwrap();
        for cut in 0..=file.len() {
            compare(&file[..cut]);
        }
        let payload = read_chunk_payload(&file).unwrap();
        for cut in 0..=payload.len() {
            compare(&build_memory_chunk(&payload[..cut]).unwrap());
        }
        // Mutate every header, separator, offset, and item byte with a valid
        // frame checksum, so structural checks cannot hide behind framing.
        for pos in 0..payload.len() {
            for value in [0, 1, 17, b'#', 255] {
                let mut bytes = payload.to_vec();
                bytes[pos] = value;
                compare(&build_memory_chunk(&bytes).unwrap());
            }
        }
        for offset in [1_u32, 7, 8, 1024, u32::MAX] {
            let mut bytes = payload.to_vec();
            bytes[21..25].copy_from_slice(&offset.to_le_bytes());
            compare(&build_memory_chunk(&bytes).unwrap());
        }
    }
}
