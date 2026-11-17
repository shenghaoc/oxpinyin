//! The per-library phrase-index chunk **writer** — the write half of the
//! `MemoryChunk` + `SubPhraseIndex` format `chunk_format` and
//! `phrase_library` read.
//!
//! This is the byte-level output of upstream's `SubPhraseIndex::store`
//! (`src/storage/phrase_index.cpp`), reached after `compact()` rebuilds a
//! sub-index in ascending token order. `oxpinyin-utils`' storage and
//! training utilities write the system libraries' files with it; the user store writes `user.bin`,
//! `addon.bin` and `network.bin` with it — the `USER_FILE` sub-indexes,
//! whose whole-store files the pin's `_write_files` produces through the
//! same `store` path.
//!
//! ```text
//! file    = { length: u32, checksum: u32 } payload          (MemoryChunk)
//! payload = [ total_freq: u32 ]
//!           [ index_one, index_two, index_three: u32×3 ]
//!           '#' offset-array '#' entry-area '#'
//! ```
//!
//! * `index_one == 17` (header 16 + separator) — the offset array holds
//!   one `u32` per `token & PHRASE_MASK` slot up to the highest occupied
//!   slot; `0` is the no-item sentinel (`add_phrase_item` never stores an
//!   offset below 8).
//! * The entry area's first 8 bytes stay zero (`add_phrase_item` bumps a
//!   zero content size to 8 before the first write) — **once there is a
//!   first write**: a library with no items has an empty entry area, so
//!   its whole payload is the 16-byte header and three separators
//!   (19 bytes; measured on the pin's own `gen_binary_files` output for
//!   an empty `.table`). Item offsets in the offset array are relative to
//!   the entry-area start.
//! * Each item is `{ u8 phrase_length, u8 n_pronunciations, u32 unigram,
//!   ucs4_t phrase[L], { ChewingKey u16[L], u32 freq } × n_pronunciations }`
//!   (`phrase_item_header`, `phrase_index.h:56`; `sizeof(ChewingKey) == 2`).
//! * Items appear in ascending slot order — the order `compact()`'s token
//!   walk feeds `add_phrase_item`, which appends to the entry area.
//!
//! The checksum is `MemoryChunk::get_check_sum`: the XOR of the payload's
//! little-endian `u32` words with tail bytes folded in shifted by
//! position (`chunk_format` — the one written copy both halves take).

use crate::data::chunk_format::{
    CHEWING_KEY_SIZE, FIRST_ITEM_OFFSET, INDEX_ONE, ITEM_HEADER_SIZE, SEPARATOR,
};

/// `PHRASE_MASK` (`novel_types.h:41`): the library-local token bits a
/// phrase-index slot is addressed by. Re-exported from the shared format
/// module for the writers above this crate.
pub use crate::data::chunk_format::PHRASE_MASK;

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
/// # Errors
///
/// Fails on a repeated or out-of-order slot, an empty phrase, a
/// pronunciation whose key run length differs from the phrase length, an
/// item longer than the format allows (phrase length or pronunciation
/// count above `u8::MAX`), or a total frequency above `u32::MAX`.
pub fn build_chunk(items: &[(u32, ChunkItem)]) -> Result<Vec<u8>, ChunkWriteError> {
    // ---- validate and serialise the entry area -------------------------
    // Empty until the first item: `add_phrase_item`'s `if (0 == offset)
    // offset = 8` only fires when an item is written.
    let mut content: Vec<u8> = Vec::new();
    let mut offsets: Vec<u32> = Vec::new();
    let mut total_freq: u64 = 0;
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
        total_freq += u64::from(item.unigram);
    }

    let total_freq = u32::try_from(total_freq)
        .map_err(|_| ChunkWriteError("chunk total_freq overflows u32".to_owned()))?;

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

/// A structural rejection from `validate_phrase_item`, without allocation.
/// Its diagnostic matches the corresponding [`ChunkWriteError`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PhraseItemValidationError(&'static str);

impl std::fmt::Display for PhraseItemValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "chunk write error: {}", self.0)
    }
}

impl std::error::Error for PhraseItemValidationError {}

/// Check exactly the byte extents [`decode_phrase_item`] reads, without
/// allocating or constructing a typed item. Trailing bytes and arbitrary
/// text/key/frequency values are accepted, just as by the decoder.
///
/// # Errors
/// Reports the decoder's first missing header field, truncated text,
/// truncated pronunciation keys, or truncated pronunciation frequency.
pub(crate) fn validate_phrase_item(bytes: &[u8]) -> Result<(), PhraseItemValidationError> {
    let length = usize::from(*bytes.first().ok_or(PhraseItemValidationError(
        "phrase item: no phrase length byte",
    ))?);
    let prons = usize::from(*bytes.get(1).ok_or(PhraseItemValidationError(
        "phrase item: no pronunciation count byte",
    ))?);
    if bytes.len() < ITEM_HEADER_SIZE {
        return Err(PhraseItemValidationError("phrase item: no unigram"));
    }
    let text_end = ITEM_HEADER_SIZE + 4 * length;
    if bytes.len() < text_end {
        return Err(PhraseItemValidationError(
            "phrase item: phrase text truncated",
        ));
    }
    let keys = CHEWING_KEY_SIZE * length;
    let row = keys + 4;
    let remaining = bytes.len() - text_end;
    // Both counts are u8, so these products fit even on 32-bit targets.
    if remaining < prons * row {
        // Earlier complete rows cannot fail. Locate the first incomplete
        // row without decoding every pronunciation before it.
        return Err(PhraseItemValidationError(if remaining % row < keys {
            "phrase item: pronunciation keys truncated"
        } else {
            "phrase item: pronunciation freq truncated"
        }));
    }
    Ok(())
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
    let unigram = bytes
        .get(offset..offset + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
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
        let freq = bytes
            .get(offset..offset + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
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
    let header = payload
        .get(..16)
        .ok_or_else(|| ChunkWriteError("sub phrase index: short header".to_owned()))?;
    let u32_at = |offset: usize| -> u32 {
        u32::from_le_bytes([
            header[offset],
            header[offset + 1],
            header[offset + 2],
            header[offset + 3],
        ])
    };
    let total = u32_at(0);
    let index_one = u32_at(4) as usize;
    let index_two = u32_at(8) as usize;
    let index_three = u32_at(12) as usize;

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
