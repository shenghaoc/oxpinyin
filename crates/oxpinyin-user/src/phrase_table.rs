//! Pin `PhraseLargeTable3`: UCS-4 text keys and unnormalized token-array values.
//!
//! Ported from `src/storage/phrase_large_table3*.cpp` at
//! `074a2219c90feaf962d0d24f034514033ece5f99`. Empty prefix records and
//! duplicate tokens are state, including the KC/tkrzw mask visitor's retained
//! suffix (`memory_chunk.h:290-296`); loading must not reconstruct these rows.

use oxpinyin_store::StoreError;

/// No matching enabled token or existing key.
pub const SEARCH_NONE: i32 = 0;
/// At least one enabled token was appended.
pub const SEARCH_OK: i32 = 1;
/// Successful entry operation.
pub const ERROR_OK: i32 = 0;
/// Insertion encountered the token before any greater token.
pub const ERROR_INSERT_ITEM_EXISTS: i32 = 1;
/// Removal did not find the token.
pub const ERROR_REMOVE_ITEM_DONOT_EXISTS: i32 = 2;

/// Encode phrase text as the pin's little-endian UCS-4 key.
#[must_use]
pub fn key(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
}

/// Encode raw UCS-4 words as the pin's little-endian key. No `char` or
/// `str` is built, so a surrogate or above-Unicode word survives.
#[must_use]
pub fn key_ucs4(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

/// A raw token-array entry, without sorting or deduplication.
#[derive(Debug, Default)]
pub struct PhraseTableEntry {
    bytes: Vec<u8>,
}

impl PhraseTableEntry {
    /// Adopt an exact record, corresponding to `MemoryChunk::set_chunk`.
    ///
    /// # Errors
    /// Rejects incomplete tokens, whose pointer traversal is undefined in pin.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, StoreError> {
        validate(&bytes)?;
        Ok(Self { bytes })
    }

    /// Stored bytes, including duplicate tokens.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Insert before the first greater token; detect equality only before it.
    pub fn add_index(&mut self, token: u32) -> i32 {
        let mut offset = self.bytes.len();
        for (index, existing) in self
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| decode(word))
            .enumerate()
        {
            if existing == token {
                return ERROR_INSERT_ITEM_EXISTS;
            }
            if existing > token {
                offset = index * 4;
                break;
            }
        }
        self.bytes.splice(offset..offset, token.to_le_bytes());
        ERROR_OK
    }

    /// Remove the first occurrence only (`phrase_large_table3.cpp:73-88`).
    pub fn remove_index(&mut self, token: u32) -> i32 {
        let Some(index) = self
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| decode(word))
            .position(|v| v == token)
        else {
            return ERROR_REMOVE_ITEM_DONOT_EXISTS;
        };
        self.bytes.drain(index * 4..index * 4 + 4);
        ERROR_OK
    }

    /// Remove all matching tokens, preserving survivor order.
    pub fn mask_out(&mut self, mask: u32, value: u32) -> bool {
        let mut offset = 0;
        while offset < self.bytes.len() {
            if decode(&self.bytes[offset..offset + 4]) & mask == value {
                self.bytes.drain(offset..offset + 4);
            } else {
                offset += 4;
            }
        }
        true
    }

    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    pub(crate) fn set_content(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        validate(bytes)?;
        if bytes.len() > self.bytes.len() {
            self.bytes.resize(bytes.len(), 0);
        }
        self.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(())
    }
}

/// The pin's `PhraseTableEntry::search` over a stored record read in place, after
/// [`PhraseTableEntry::from_bytes`]'s check, with no entry built.
///
/// # Errors
/// Rejects incomplete tokens, as [`PhraseTableEntry::from_bytes`] does.
pub fn search_bytes(
    bytes: &[u8],
    tokens: &mut Vec<u32>,
    enabled: impl FnMut(u32) -> bool,
) -> Result<i32, StoreError> {
    validate(bytes)?;
    Ok(search_tokens(bytes, tokens, enabled))
}

fn search_tokens(bytes: &[u8], tokens: &mut Vec<u32>, mut enabled: impl FnMut(u32) -> bool) -> i32 {
    let mut result = SEARCH_NONE;
    for token in bytes.as_chunks::<4>().0.iter().map(|word| decode(word)) {
        if enabled(token) {
            tokens.push(token);
            result |= SEARCH_OK;
        }
    }
    result
}

pub(crate) fn validate(bytes: &[u8]) -> Result<(), StoreError> {
    if !bytes.len().is_multiple_of(4) {
        return Err(StoreError::InvalidInput(
            "phrase table value contains an incomplete token",
        ));
    }
    Ok(())
}

fn decode(bytes: &[u8]) -> u32 {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}
