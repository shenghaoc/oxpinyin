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

use crate::PHRASE_MASK;
use crate::data::chunk_format::{CHEWING_KEY_SIZE, FIRST_ITEM_OFFSET, INDEX_ONE, SEPARATOR};

/// A malformed [`ChunkItem`] or slot sequence.
#[derive(Debug)]
pub struct ChunkWriteError(String);

impl std::fmt::Display for ChunkWriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "chunk write error: {}", self.0)
    }
}

impl std::error::Error for ChunkWriteError {}

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
/// `items` must be in ascending slot order (slot = `token & PHRASE_MASK`)
/// — the order `FacadePhraseIndex::compact()` produces — and slots must
/// not repeat.
///
/// The library's unigram total is the sum of item frequencies modulo 2^32.
///
/// # Errors
///
/// Fails on a repeated or out-of-order slot, an empty phrase, a
/// pronunciation whose key run length differs from the phrase length, an
/// item longer than the format allows (phrase length or pronunciation
/// count above `u8::MAX`).
pub fn build_chunk(items: &[(u32, ChunkItem)]) -> Result<Vec<u8>, ChunkWriteError> {
    // ---- validate and serialise the entry area -------------------------
    // Empty until the first item: `add_phrase_item`'s `if (0 == offset)
    // offset = 8` only fires when an item is written.
    let mut content: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new();
    let mut total_freq = 0_u32;
    let mut last_slot: Option<u32> = None;

    for &(slot, ref item) in items {
        if slot > PHRASE_MASK {
            return Err(ChunkWriteError(format!(
                "chunk slot {slot:#010x} exceeds PHRASE_MASK"
            )));
        }
        if let Some(prev) = last_slot.filter(|&p| slot <= p) {
            return Err(ChunkWriteError(format!(
                "chunk slots not ascending: {slot:#010x} after {prev:#010x}"
            )));
        }
        last_slot = Some(slot);

        let phrase_len = item.phrase.len();
        if phrase_len == 0 || phrase_len > usize::from(u8::MAX) {
            return Err(ChunkWriteError(format!(
                "chunk slot {slot:#010x} phrase length {} out of range",
                item.phrase.len()
            )));
        }
        if item.prons.len() > usize::from(u8::MAX) {
            return Err(ChunkWriteError(format!(
                "chunk slot {slot:#010x} has {} pronunciations",
                item.prons.len()
            )));
        }

        if content.is_empty() {
            content.resize(usize::try_from(FIRST_ITEM_OFFSET).unwrap_or(0), 0);
        }
        let offset = u32::try_from(content.len()).map_err(|_| {
            ChunkWriteError(format!("chunk slot {slot:#010x} offset overflows u32"))
        })?;
        offsets.resize(usize::try_from(slot).unwrap_or(0) + 1, 0);
        offsets[usize::try_from(slot).unwrap_or(0)] = offset;

        content.push(u8::try_from(phrase_len).unwrap_or(u8::MAX));
        content.push(u8::try_from(item.prons.len()).unwrap_or(u8::MAX));
        content.extend_from_slice(&item.unigram.to_le_bytes());
        for &code in &item.phrase {
            content.extend_from_slice(&code.to_le_bytes());
        }
        for (keys, freq) in &item.prons {
            if keys.len() != phrase_len {
                return Err(ChunkWriteError(format!(
                    "chunk slot {slot:#010x} pronunciation has {} keys for a {}-character phrase",
                    keys.len(),
                    phrase_len
                )));
            }
            for key in keys {
                content.extend_from_slice(&key.to_le_bytes());
            }
            content.extend_from_slice(&freq.to_le_bytes());
        }
        total_freq = total_freq.wrapping_add(item.unigram);
    }

    // ---- assemble payload: header, offset array, entry area ------------
    let slot_count = offsets.len();
    let index_one = INDEX_ONE;
    let index_two = index_one + u32::try_from(slot_count * 4).unwrap_or(u32::MAX) + 1;
    let index_three = index_two + u32::try_from(content.len()).unwrap_or(u32::MAX) + 1;

    let mut payload = Vec::with_capacity(usize::try_from(index_three).unwrap_or(0));
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

    // ---- MemoryChunk header --------------------------------------------
    // The single framing implementation; the error type narrows here.
    crate::data::chunk_format::build_memory_chunk(&payload)
        .map_err(|e| ChunkWriteError(e.to_string()))
}

/// Serialises one item into the `PhraseItem` wire form.
///
/// The entry-area encoding above and the `PhraseIndexLogger`'s record
/// payloads, which carry whole items (`append_record`'s
/// `oldone`/`newone` chunks).
///
/// The shape contract is [`build_chunk`]'s, checked here too because a
/// logger record with a short pronunciation run would decode wrong:
/// `decode_phrase_item` reads `2 * phrase_length` key bytes per
/// pronunciation, so a missing key byte shifts the frequency field into
/// key data — silently wrong on replay, not an error.
///
/// # Errors
///
/// Fails on an empty or over-`u8::MAX` phrase, more than `u8::MAX`
/// pronunciations, or any pronunciation whose key run length differs
/// from the phrase length.
pub fn encode_phrase_item(item: &ChunkItem) -> Result<Vec<u8>, ChunkWriteError> {
    let phrase_length = item.phrase.len();
    if phrase_length == 0 || phrase_length > usize::from(u8::MAX) {
        return Err(ChunkWriteError(format!(
            "phrase item: phrase length {phrase_length} out of range"
        )));
    }
    if item.prons.len() > usize::from(u8::MAX) {
        return Err(ChunkWriteError(format!(
            "phrase item: {} pronunciations",
            item.prons.len()
        )));
    }
    for (keys, _) in &item.prons {
        if keys.len() != phrase_length {
            return Err(ChunkWriteError(format!(
                "phrase item: pronunciation has {} keys for a {phrase_length}-character phrase",
                keys.len()
            )));
        }
    }
    let mut bytes = Vec::new();
    bytes.push(u8::try_from(phrase_length).unwrap_or(u8::MAX));
    bytes.push(u8::try_from(item.prons.len()).unwrap_or(u8::MAX));
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
    Ok(bytes)
}

/// Decodes one `PhraseItem` wire form — the inverse of
/// [`encode_phrase_item`], for the logger payloads' replay.
///
/// # Errors
///
/// Fails when the header claims a shape that does not fit the bytes
/// (length, pronunciation count, or run lengths).
pub(crate) fn decode_phrase_item(bytes: &[u8]) -> Result<ChunkItem, ChunkWriteError> {
    let phrase_length = *bytes
        .first()
        .ok_or_else(|| ChunkWriteError("phrase item: no phrase length byte".to_owned()))?
        as usize;
    let n_prons = *bytes
        .get(1)
        .ok_or_else(|| ChunkWriteError("phrase item: no pronunciation count byte".to_owned()))?
        as usize;
    let mut offset = 2;
    let unigram = bytes[offset..]
        .first_chunk()
        .map(|word| u32::from_le_bytes(*word))
        .ok_or_else(|| ChunkWriteError("phrase item: no unigram".to_owned()))?;
    offset += 4;

    let phrase_end = offset + 4 * phrase_length;
    let phrase_area = bytes
        .get(offset..phrase_end)
        .ok_or_else(|| ChunkWriteError("phrase item: phrase text truncated".to_owned()))?;
    let phrase: Vec<u32> = phrase_area
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    offset = phrase_end;

    let mut prons = Vec::with_capacity(n_prons);
    for _ in 0..n_prons {
        let keys_end = offset + CHEWING_KEY_SIZE * phrase_length;
        let keys_area = bytes.get(offset..keys_end).ok_or_else(|| {
            ChunkWriteError("phrase item: pronunciation keys truncated".to_owned())
        })?;
        let keys: Vec<u16> = keys_area
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_le_bytes(*c))
            .collect();
        offset = keys_end;
        let freq = bytes[offset..]
            .first_chunk()
            .map(|word| u32::from_le_bytes(*word))
            .ok_or_else(|| {
                ChunkWriteError("phrase item: pronunciation freq truncated".to_owned())
            })?;
        offset += 4;
        prons.push((keys, freq));
    }

    Ok(ChunkItem {
        phrase,
        unigram,
        prons,
    })
}

/// Borrow the stored total, offset-array bytes and item arena without decoding
/// any items. Uses exactly `decode_sub_phrase_index`'s layout checks.
///
/// # Errors
/// Refuses a short header, invalid separator or section bounds.
pub(crate) fn sub_phrase_index_sections(
    payload: &[u8],
) -> Result<(u32, &[u8], &[u8]), ChunkWriteError> {
    let [total, index_one, index_two, index_three] = payload
        .as_chunks::<4>()
        .0
        .first_chunk::<4>()
        .ok_or_else(|| ChunkWriteError("sub phrase index: short header".to_owned()))?;
    let total = u32::from_le_bytes(*total);
    let index_one = u32::from_le_bytes(*index_one) as usize;
    let index_two = u32::from_le_bytes(*index_two) as usize;
    let index_three = u32::from_le_bytes(*index_three) as usize;

    if payload.get(INDEX_ONE as usize - 1) != Some(&SEPARATOR) {
        return Err(ChunkWriteError(
            "sub phrase index: no lead separator".to_owned(),
        ));
    }
    if index_one != INDEX_ONE as usize {
        return Err(ChunkWriteError(format!(
            "sub phrase index: index_one {index_one} is not the header end"
        )));
    }
    let _ = index_two
        .checked_sub(1)
        .and_then(|at| payload.get(at))
        .filter(|&&byte| byte == SEPARATOR)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no offset separator".to_owned()))?;
    let _ = index_three
        .checked_sub(1)
        .and_then(|at| payload.get(at))
        .filter(|&&byte| byte == SEPARATOR)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no entry separator".to_owned()))?;
    if index_three > payload.len() {
        return Err(ChunkWriteError(
            "sub phrase index: past the payload end".to_owned(),
        ));
    }

    let offset_array = payload
        .get(index_one..index_two - 1)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no offset array".to_owned()))?;
    let entry_area = payload
        .get(index_two..index_three - 1)
        .ok_or_else(|| ChunkWriteError("sub phrase index: no entry area".to_owned()))?;

    Ok((total, offset_array, entry_area))
}

#[cfg(test)]
mod tests;
