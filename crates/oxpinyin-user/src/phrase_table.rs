//! Pin `PhraseLargeTable3`: UCS-4 text keys and unnormalized token-array values.
//!
//! Ported from `src/storage/phrase_large_table3*.cpp` at
//! `074a2219c90feaf962d0d24f034514033ece5f99`. Empty prefix records and
//! duplicate tokens are state, including the KC/tkrzw mask visitor's retained
//! suffix (`memory_chunk.h:290-296`); loading must not reconstruct these rows.

use oxpinyin_core::store::StoreError;

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

    #[cfg(feature = "bdb")]
    pub(crate) fn from_validated_bytes(bytes: Vec<u8>) -> Self {
        Self { bytes }
    }

    /// Stored bytes, including duplicate tokens.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Insert before the first greater token. An equal token found before that
    /// point ends the scan without insertion (`ERROR_INSERT_ITEM_EXISTS`).
    /// Insertion corresponds to `ERROR_OK` (`novel_types.h:78-79`,
    /// `phrase_large_table3.cpp:56-71`).
    pub fn add_index(&mut self, token: u32) {
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
                return;
            }
            if existing > token {
                offset = index * 4;
                break;
            }
        }
        self.bytes.splice(offset..offset, token.to_le_bytes());
    }

    /// Remove the first occurrence (`ERROR_OK`). Return early if the token is
    /// absent (`ERROR_REMOVE_ITEM_DONOT_EXISTS`, `novel_types.h:78-80`,
    /// `phrase_large_table3.cpp:73-89`).
    pub fn remove_index(&mut self, token: u32) {
        let Some(index) = self
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| decode(word))
            .position(|v| v == token)
        else {
            return;
        };
        self.bytes.drain(index * 4..index * 4 + 4);
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

    #[cfg(not(feature = "bdb"))]
    pub(crate) fn set_validated_content(&mut self, bytes: &[u8]) {
        if bytes.len() > self.bytes.len() {
            self.bytes.resize(bytes.len(), 0);
        }
        self.bytes[..bytes.len()].copy_from_slice(bytes);
    }
}

/// The pin's `PhraseTableEntry::search` over a stored record read in place, after
/// [`PhraseTableEntry::from_bytes`]'s check, with no entry built.
/// Append enabled tokens in stored order, including duplicates. The pin
/// returns `SEARCH_OK` if it appended a token and `SEARCH_NONE` otherwise
/// (`novel_types.h:71-72`, `phrase_large_table3.cpp:29-52`).
///
/// # Errors
/// Rejects incomplete tokens, as [`PhraseTableEntry::from_bytes`] does.
pub fn search_bytes(
    bytes: &[u8],
    tokens: &mut Vec<u32>,
    enabled: impl FnMut(u32) -> bool,
) -> Result<(), StoreError> {
    validate(bytes)?;
    search_tokens(bytes, tokens, enabled);
    Ok(())
}

fn search_tokens(bytes: &[u8], tokens: &mut Vec<u32>, mut enabled: impl FnMut(u32) -> bool) {
    for token in bytes.as_chunks::<4>().0.iter().map(|word| decode(word)) {
        if enabled(token) {
            tokens.push(token);
        }
    }
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
