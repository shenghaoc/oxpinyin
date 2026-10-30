//! Owned USER_FILE sub-index; phase 1 still seeds the session's existing rows.

use std::collections::BTreeMap;

use oxpinyin_data::chunk_write::{sub_phrase_index_sections, validate_phrase_item};
use oxpinyin_data::phrase_library::PhraseItemView;

/// The pin's SubPhraseIndex (phrase_index.h:257-430 at 074a2219), owned.
#[derive(Clone, Debug, Default)]
pub(crate) struct UserLibrary {
    total: u32,
    slots: Slots,
    content: Vec<u8>,
    live: usize,
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
    pub(crate) fn load(file: &[u8]) -> Result<Self, String> {
        let payload =
            oxpinyin_data::user_files::read_chunk_payload(file).map_err(|e| e.to_string())?;
        let (total, offsets, content) =
            sub_phrase_index_sections(payload).map_err(|e| e.to_string())?;
        let mut live = 0_usize;
        let mut count = 0_usize;
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
            slots,
            content: content.to_vec(),
            live,
        })
    }

    pub(crate) const fn total(&self) -> u32 {
        self.total
    }
    pub(crate) const fn is_empty(&self) -> bool {
        self.live == 0
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

    /// Phase 1's independent store witness. Shipping saves still use rows and
    /// build_chunk. As at compact(), copy live bodies in ascending slot order
    /// and rebuild the item sum; the loaded total remains separate.
    #[cfg(test)]
    pub(crate) fn store(&self) -> Result<Vec<u8>, String> {
        let mut content = Vec::with_capacity(self.live.saturating_add(8));
        let mut offsets = Vec::new();
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
            if content.is_empty() {
                content.resize(8, 0);
            }
            let offset = u32::try_from(content.len()).map_err(|e| e.to_string())?;
            offsets.resize(slot as usize + 1, 0_u32);
            offsets[slot as usize] = offset;
            content.extend_from_slice(view.as_bytes());
            total += u64::from(view.unigram());
        }
        let total = u32::try_from(total)
            .map_err(|_| "chunk write error: chunk total_freq overflows u32".to_owned())?;
        let index_two = 18_u32
            .checked_add(u32::try_from(offsets.len() * 4).map_err(|e| e.to_string())?)
            .ok_or("offset array too large")?;
        let index_three = index_two
            .checked_add(u32::try_from(content.len()).map_err(|e| e.to_string())?)
            .and_then(|end| end.checked_add(1))
            .ok_or("arena too large")?;
        let mut payload = Vec::new();
        for word in [total, 17, index_two, index_three] {
            payload.extend_from_slice(&word.to_le_bytes());
        }
        payload.push(b'#');
        for offset in offsets {
            payload.extend_from_slice(&offset.to_le_bytes());
        }
        payload.push(b'#');
        payload.extend_from_slice(&content);
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
