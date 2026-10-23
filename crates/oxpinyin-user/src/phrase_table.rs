//! Pin `PhraseLargeTable3`: UCS-4 text keys and unnormalized token-array values.
//!
//! Ported from `src/storage/phrase_large_table3*.cpp` at
//! `074a2219c90feaf962d0d24f034514033ece5f99`. Empty prefix records and
//! duplicate tokens are state, including the KC/tkrzw mask visitor's retained
//! suffix (`memory_chunk.h:290-296`); loading must not reconstruct these rows.

use std::ops::Bound;

use oxpinyin_store::{ReadStore, StoreError, WriteTxn};

use crate::store::txn_get_bytes;

/// Internal table containing the pin's phrase records.
pub const TABLE: &str = "user_phrase_table3";
/// No matching enabled token or existing key.
pub const SEARCH_NONE: i32 = 0;
/// At least one enabled token was appended.
pub const SEARCH_OK: i32 = 1;
/// The exact key exists, even if its value is empty.
pub const SEARCH_CONTINUED: i32 = 2;
/// Successful entry operation.
pub const ERROR_OK: i32 = 0;
/// Insertion encountered the token before any greater token.
pub const ERROR_INSERT_ITEM_EXISTS: i32 = 1;
/// Removal did not find the token.
pub const ERROR_REMOVE_ITEM_DONOT_EXISTS: i32 = 2;
/// Pin's database-write failure code.
pub const ERROR_FILE_CORRUPTION: i32 = 7;

/// Search flags and tokens in stored traversal order.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SearchResult {
    /// Bitwise combination of the pin's `SEARCH_*` flags.
    pub flags: i32,
    /// Tokens, including duplicates, in record traversal order.
    pub tokens: Vec<u32>,
}

/// Encode phrase text as the pin's little-endian UCS-4 key.
#[must_use]
pub fn key(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| u32::from(ch).to_le_bytes())
        .collect()
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

    /// Token count (`phrase_large_table3.cpp:93-97`).
    #[must_use]
    pub fn get_length(&self) -> usize {
        self.bytes.len() / 4
    }

    /// Append enabled tokens without clearing the destination.
    ///
    /// `enabled` receives each token, allowing the caller to implement pin's
    /// disabled-library filtering (`phrase_large_table3.cpp:29-51`).
    pub fn search(&self, tokens: &mut Vec<u32>, enabled: impl FnMut(u32) -> bool) -> i32 {
        search_tokens(&self.bytes, tokens, enabled)
    }

    /// Insert before the first greater token; detect equality only before it.
    pub fn add_index(&mut self, token: u32) -> i32 {
        let mut offset = self.bytes.len();
        for (index, existing) in self.bytes.chunks_exact(4).map(decode).enumerate() {
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
            .chunks_exact(4)
            .map(decode)
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
    fn set_content(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        validate(bytes)?;
        if bytes.len() > self.bytes.len() {
            self.bytes.resize(bytes.len(), 0);
        }
        self.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(())
    }
}

/// [`PhraseTableEntry::search`] over a stored record read in place, after
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
    for token in bytes.chunks_exact(4).map(decode) {
        if enabled(token) {
            tokens.push(token);
            result |= SEARCH_OK;
        }
    }
    result
}

fn validate(bytes: &[u8]) -> Result<(), StoreError> {
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

/// Search an exact key with every library enabled.
///
/// # Errors
/// Returns an incomplete-token error. Backend read failure is `SEARCH_NONE`,
/// as at `phrase_large_table3_bdb.cpp:196-198` (pin 074a2219).
pub fn search(db: &impl ReadStore, text: &str) -> Result<SearchResult, StoreError> {
    let mut result = SearchResult::default();
    let Ok(Some(searched)) = db.read_with(TABLE, &key(text), |bytes| {
        search_bytes(bytes, &mut result.tokens, |_| true)
    }) else {
        return Ok(SearchResult::default());
    };
    result.flags = SEARCH_CONTINUED | searched?;
    Ok(result)
}

/// Search strictly longer phrases after an existing exact prefix record.
///
/// # Errors
/// Returns an incomplete-token error. Backend failures end the search with
/// the tokens accumulated so far (`phrase_large_table3_bdb.cpp:233-260`).
pub fn search_suggestion(db: &impl ReadStore, text: &str) -> Result<SearchResult, StoreError> {
    let prefix = key(text);
    let mut result = SearchResult::default();
    if !matches!(db.read_with(TABLE, &prefix, |_| ()), Ok(Some(()))) {
        return Ok(result);
    }
    let mut upper = prefix.clone();
    while upper.last() == Some(&u8::MAX) {
        upper.pop();
    }
    let hi = if let Some(last) = upper.last_mut() {
        *last += 1;
        Bound::Excluded(upper.as_slice())
    } else {
        Bound::Unbounded
    };
    let mut malformed = None;
    let _ = db.range(TABLE, Bound::Excluded(&prefix), hi, |_, bytes| {
        match search_bytes(bytes, &mut result.tokens, |_| true) {
            Ok(flags) => result.flags |= flags,
            Err(error) => {
                malformed = Some(error);
                return Err(StoreError::InvalidInput("incomplete phrase table token"));
            }
        }
        Ok(())
    });
    if let Some(error) = malformed {
        return Err(error);
    }
    Ok(result)
}

/// Add a token, creating missing proper prefixes only for a new text key.
///
/// # Errors
/// Returns incomplete-token errors or an empty-text error. Backend reads that
/// fail take the missing-entry path; failed writes return
/// `ERROR_FILE_CORRUPTION` (`phrase_large_table3_bdb.cpp:283-331`, KC
/// `phrase_large_table3_kyotodb.cpp:267-310`, tkrzw
/// `phrase_large_table3_tkrzwdb.cpp:202-239`, pin 074a2219).
/// Callers following pin's `_add_phrase` intentionally ignore this numeric
/// result and retain earlier changes (`pinyin.cpp:596-606`); it is not a
/// transaction rollback request.
pub fn add_index(txn: &mut impl WriteTxn, text: &str, token: u32) -> Result<i32, StoreError> {
    let key = key(text);
    if key.is_empty() {
        return Err(StoreError::InvalidInput("phrase table text is empty"));
    }
    if let Ok(Some(bytes)) = txn_get_bytes(txn, TABLE, &key) {
        let mut entry = PhraseTableEntry::from_bytes(bytes)?;
        let result = entry.add_index(token);
        if txn.put(TABLE, &key, entry.as_bytes()).is_err() {
            return Ok(ERROR_FILE_CORRUPTION);
        }
        return Ok(result);
    }
    let mut entry = PhraseTableEntry::default();
    entry.add_index(token);
    if txn.put(TABLE, &key, entry.as_bytes()).is_err() {
        return Ok(ERROR_FILE_CORRUPTION);
    }
    for length in (1..key.len() / 4).rev() {
        let prefix = &key[..length * 4];
        if matches!(txn.read_with(TABLE, prefix, |_| ()), Ok(Some(()))) {
            return Ok(ERROR_OK);
        }
        if txn.put(TABLE, prefix, &[]).is_err() {
            return Ok(ERROR_FILE_CORRUPTION);
        }
    }
    Ok(ERROR_OK)
}

/// Remove one token, retaining the key even when its value becomes empty.
///
/// # Errors
/// Returns an incomplete-token error. Initial read failure is missing-item;
/// write failure is `ERROR_FILE_CORRUPTION`
/// (`phrase_large_table3_bdb.cpp:349-366`, KC
/// `phrase_large_table3_kyotodb.cpp:324-343`, tkrzw
/// `phrase_large_table3_tkrzwdb.cpp:253-272`, pin 074a2219).
pub fn remove_index(txn: &mut impl WriteTxn, text: &str, token: u32) -> Result<i32, StoreError> {
    let key = key(text);
    let Ok(Some(bytes)) = txn_get_bytes(txn, TABLE, &key) else {
        return Ok(ERROR_REMOVE_ITEM_DONOT_EXISTS);
    };
    let mut entry = PhraseTableEntry::from_bytes(bytes)?;
    let result = entry.remove_index(token);
    if result == ERROR_OK && txn.put(TABLE, &key, entry.as_bytes()).is_err() {
        return Ok(ERROR_FILE_CORRUPTION);
    }
    Ok(result)
}

/// Apply pin's backend-specific mask walk in ascending raw key order.
///
/// BDB resets each record; KC/tkrzw retain the previous post-mask suffix
/// when the next record is shorter. Empty records participate in the walk.
///
/// # Errors
/// Returns store errors or an incomplete-token error.
pub fn mask_out(txn: &mut impl WriteTxn, mask: u32, value: u32) -> Result<bool, StoreError> {
    let mut rows = Vec::new();
    txn.for_each(TABLE, |key, bytes| {
        rows.push((key.to_vec(), bytes.to_vec()));
        Ok(())
    })?;
    #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
    let mut entry = PhraseTableEntry::default();
    for (key, bytes) in rows {
        #[cfg(feature = "bdb")]
        let mut entry = PhraseTableEntry::from_bytes(bytes)?;
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        entry.set_content(&bytes)?;
        entry.mask_out(mask, value);
        txn.put(TABLE, &key, entry.as_bytes())?;
    }
    Ok(true)
}
