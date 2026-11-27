//! Builds phrase-library chunk files from stored items. Each input pair to
//! [`build_chunk`] uses a 24-bit slot within one library. Supply distinct slots in
//! ascending order.
//!
//! ```text
//! file    = { length: u32, checksum: u32 } payload          (MemoryChunk)
//! payload = [ total_freq: u32 ]
//!           [ index_one, index_two, index_three: u32×3 ]
//!           '#' offset-array '#' entry-area '#'
//! ```
//!
//! The offset array contains a little-endian `u32` for each slot up to the highest used.
//! Zero marks an empty slot. Item offsets are relative to the entry area. The first
//! item starts eight bytes into that area. Each item stores byte-sized phrase length
//! and reading count, a unigram frequency, the UCS-4 words, and a packed key sequence
//! and frequency for each reading. [`crate::data::chunk_format::chunk_checksum`] covers
//! the payload.
//!
//! The pinned `SubPhraseIndex::add_phrase_item` accumulates the library's
//! unigram total modulo 2^32 (`phrase_index.cpp:215`, `phrase_index.h:259`).

// SubPhraseIndex::add_phrase_item masks tokens with PHRASE_MASK,
// phrase_index.cpp:208–216; memory_chunk.h:536–557 specifies the frame. The builder
// requires the item offsets to fit their output fields.

use crate::data::chunk_format::{CHUNK_HEADER_SIZE, finish_memory_chunk};
use crate::data::phrase_library::{
    CHEWING_KEY_SIZE, FIRST_ITEM_OFFSET, INDEX_ONE, ITEM_HEADER_SIZE, SEPARATOR,
};
use crate::vocab::PHRASE_MASK;

/// One phrase entry of a library chunk.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChunkItem {
    /// The phrase text as UCS-4 code points (`ucs4_t` array).
    pub phrase: Vec<u32>,
    /// The item's unigram frequency — the `\1-gram` count plus the one
    /// `gen_unigram` adds for every library token (the system libraries;
    /// a user item carries whatever training accumulated).
    pub unigram: u32,
    /// Pronunciations in insertion order: packed `ChewingKey` sequence
    /// (`to_packed`, little-endian on disk) and the accumulated frequency
    /// (`PhraseItem::add_pronunciation` sums duplicate exact key
    /// sequences).
    pub prons: Vec<(Vec<u16>, u32)>,
}

/// Serialises one library's phrase items into the complete chunk file.
///
/// Supply strictly increasing library-local slots in `0..=PHRASE_MASK`.
/// Item offsets, section offsets, and the payload length must fit in `u32`.
///
/// The library's unigram total is the sum of item frequencies modulo 2^32.
///
/// Returns `None` for a repeated, decreasing, or out-of-range slot, or an
/// invalid item shape.
#[must_use]
pub fn build_chunk(items: &[(u32, &ChunkItem)]) -> Option<Vec<u8>> {
    let mut slots = 0;
    let mut content_len = if items.is_empty() {
        0
    } else {
        FIRST_ITEM_OFFSET
    };
    for (slot, item) in items {
        if *slot > PHRASE_MASK || *slot < slots {
            return None;
        }
        slots = slot + 1;
        let [length, count] = phrase_item_counts(item)?.map(u32::from);
        let item_len =
            ITEM_HEADER_SIZE as u32 + 4 * length + count * (CHEWING_KEY_SIZE as u32 * length + 4);
        content_len = content_len.checked_add(item_len)?;
    }

    let index_two = INDEX_ONE + 4 * slots + 1;
    let index_three = index_two.checked_add(content_len)?.checked_add(1)?;
    // Buffer positions include the file header; stored section offsets do not.
    let entry_start = CHUNK_HEADER_SIZE + index_two as usize;
    let mut file = Vec::with_capacity((index_three as usize).checked_add(CHUNK_HEADER_SIZE)?);
    file.resize(entry_start, 0);
    let payload = &mut file[CHUNK_HEADER_SIZE..];
    payload[4..8].copy_from_slice(&INDEX_ONE.to_le_bytes());
    payload[8..12].copy_from_slice(&index_two.to_le_bytes());
    payload[12..16].copy_from_slice(&index_three.to_le_bytes());
    payload[INDEX_ONE as usize - 1] = SEPARATOR;
    payload[index_two as usize - 1] = SEPARATOR;
    if !items.is_empty() {
        file.resize(file.len() + FIRST_ITEM_OFFSET as usize, 0);
    }

    let mut total_freq = 0_u32;
    for (slot, item) in items {
        // Item offsets count from the entry area, including its initial hole.
        let offset = u32::try_from(file.len() - entry_start).ok()?;
        let pos = CHUNK_HEADER_SIZE + (INDEX_ONE + 4 * slot) as usize;
        file[pos..pos + 4].copy_from_slice(&offset.to_le_bytes());
        write_phrase_item(&mut file, item)?;
        total_freq = total_freq.wrapping_add(item.unigram);
    }
    file[CHUNK_HEADER_SIZE..CHUNK_HEADER_SIZE + 4].copy_from_slice(&total_freq.to_le_bytes());
    file.push(SEPARATOR);
    finish_memory_chunk(file)
}

/// Serialises one item into the `PhraseItem` wire form.
///
/// Library chunks and `PhraseIndexLogger` records store the same item bytes.
///
/// Returns `None` if the phrase is empty, either count exceeds `u8::MAX`,
/// or a pronunciation has a different length from the phrase.
#[must_use]
pub(crate) fn encode_phrase_item(item: &ChunkItem) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    write_phrase_item(&mut bytes, item)?;
    Some(bytes)
}

fn phrase_item_counts(item: &ChunkItem) -> Option<[u8; 2]> {
    let length = u8::try_from(item.phrase.len()).ok()?;
    let count = u8::try_from(item.prons.len()).ok()?;
    if length == 0
        || item
            .prons
            .iter()
            .any(|(keys, _)| keys.len() != item.phrase.len())
    {
        return None;
    }
    Some([length, count])
}

fn write_phrase_item(bytes: &mut Vec<u8>, item: &ChunkItem) -> Option<()> {
    bytes.extend_from_slice(&phrase_item_counts(item)?);
    bytes.extend_from_slice(&item.unigram.to_le_bytes());
    for &code in &item.phrase {
        bytes.extend_from_slice(&code.to_le_bytes());
    }
    for (keys, freq) in &item.prons {
        for key in keys {
            bytes.extend_from_slice(&key.to_le_bytes());
        }
        bytes.extend_from_slice(&freq.to_le_bytes());
    }
    Some(())
}

/// Decodes one `PhraseItem` wire form — the inverse of
/// [`encode_phrase_item`], for the logger payloads' replay.
///
/// Returns `None` when the declared text or readings extend past the bytes.
pub(crate) fn decode_phrase_item(bytes: &[u8]) -> Option<ChunkItem> {
    let phrase_length = usize::from(*bytes.first()?);
    let n_prons = usize::from(*bytes.get(1)?);
    let unigram = u32::from_le_bytes(*bytes.get(2..)?.first_chunk()?);
    let phrase_end = ITEM_HEADER_SIZE + 4 * phrase_length;
    let phrase_area = bytes.get(ITEM_HEADER_SIZE..phrase_end)?;
    let phrase: Vec<u32> = phrase_area
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    let mut offset = phrase_end;

    let mut prons = Vec::with_capacity(n_prons);
    for _ in 0..n_prons {
        let keys_end = offset + CHEWING_KEY_SIZE * phrase_length;
        let keys_area = bytes.get(offset..keys_end)?;
        let keys: Vec<u16> = keys_area
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        offset = keys_end;
        let freq = u32::from_le_bytes(*bytes[offset..].first_chunk()?);
        offset += 4;
        prons.push((keys, freq));
    }

    Some(ChunkItem {
        phrase,
        unigram,
        prons,
    })
}

/// Borrows the total, offset array and item arena after checking their
/// bounds and the three separators.
pub(crate) fn sub_phrase_index_sections(payload: &[u8]) -> Option<(u32, &[u8], &[u8])> {
    let [total, index_one, index_two, index_three] =
        payload.as_chunks::<4>().0.first_chunk::<4>()?;
    let index_one = u32::from_le_bytes(*index_one) as usize;
    let index_two = u32::from_le_bytes(*index_two) as usize;
    let index_three = u32::from_le_bytes(*index_three) as usize;
    if index_one != INDEX_ONE as usize || payload.get(INDEX_ONE as usize - 1) != Some(&SEPARATOR) {
        return None;
    }
    let (offset_separator, offsets) = payload.get(index_one..index_two)?.split_last()?;
    let (entry_separator, entries) = payload.get(index_two..index_three)?.split_last()?;
    if *offset_separator != SEPARATOR || *entry_separator != SEPARATOR {
        return None;
    }
    Some((u32::from_le_bytes(*total), offsets, entries))
}

#[cfg(test)]
#[path = "write_tests.rs"]
mod tests;
