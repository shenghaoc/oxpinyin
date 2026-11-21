//! Builds phrase-library chunk files from stored items. Each input pair to
//! [`build_chunk`] uses a 24-bit slot within one library. Supply distinct slots in
//! ascending order. [`crate::data::chunk_format::build_memory_chunk`] frames a prepared
//! payload.
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
pub fn build_chunk(items: &[(u32, ChunkItem)]) -> Option<Vec<u8>> {
    // The first item starts at byte 8. An empty library reserves no bytes.
    let mut content = Vec::new();
    let mut offsets = Vec::new();
    let mut total_freq = 0_u32;
    for (slot, item) in items {
        let index = *slot as usize;
        if *slot > PHRASE_MASK || index < offsets.len() {
            return None;
        }
        if content.is_empty() {
            content.resize(FIRST_ITEM_OFFSET as usize, 0);
        }
        let offset = u32::try_from(content.len()).ok()?;
        write_phrase_item(&mut content, item)?;
        offsets.resize(index + 1, 0);
        offsets[index] = offset;
        total_freq = total_freq.wrapping_add(item.unigram);
    }

    // ---- assemble payload: header, offset array, entry area ------------
    let index_one = INDEX_ONE;
    let index_two = index_one + u32::try_from(offsets.len() * 4).unwrap_or(u32::MAX) + 1;
    let index_three = index_two + u32::try_from(content.len()).unwrap_or(u32::MAX) + 1;

    let mut payload = Vec::with_capacity(index_three as usize);
    payload.extend_from_slice(&total_freq.to_le_bytes());
    payload.extend_from_slice(&index_one.to_le_bytes());
    payload.extend_from_slice(&index_two.to_le_bytes());
    payload.extend_from_slice(&index_three.to_le_bytes());
    payload.push(SEPARATOR);
    for offset in &offsets {
        payload.extend_from_slice(&offset.to_le_bytes());
    }
    payload.push(SEPARATOR);
    payload.extend_from_slice(&content);
    payload.push(SEPARATOR);

    crate::data::chunk_format::build_memory_chunk(&payload)
}

/// Serialises one item into the `PhraseItem` wire form.
///
/// Library chunks and `PhraseIndexLogger` records store the same item bytes.
///
/// Returns `None` if the phrase is empty, either count exceeds `u8::MAX`,
/// or a pronunciation has a different length from the phrase.
#[must_use]
pub fn encode_phrase_item(item: &ChunkItem) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    write_phrase_item(&mut bytes, item)?;
    Some(bytes)
}

fn write_phrase_item(bytes: &mut Vec<u8>, item: &ChunkItem) -> Option<()> {
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
    bytes.extend_from_slice(&[length, count]);
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
