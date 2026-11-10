//! The user phrase table as the pin's own records.
//!
//! `PhraseLargeTable3` holds `user_phrase_index.bin` as a DBM whose keys are
//! raw UCS-4 text and whose values are ascending token arrays. This module
//! keeps the table in one owned arena with a sorted offset vector, loaded in
//! one walk, and an overlay for session edits; the save merges the two in
//! key order. Empty prefix markers, duplicate tokens and malformed rows are
//! state and are kept verbatim.

use oxpinyin_store::StoreError;

#[cfg(test)]
use crate::phrase_table::ERROR_OK;
use crate::phrase_table::{PhraseTableEntry, key_ucs4, search_bytes};
use crate::records::RecordArena;

/// One suggestion row: the raw key and its stored tokens.
type SuggestionRows = Vec<(Vec<u8>, Vec<u32>)>;

#[derive(Clone, Debug, Default)]
pub(crate) struct UserPhraseTable {
    records: RecordArena,
}

impl UserPhraseTable {
    #[cfg(test)]
    pub(crate) const fn new() -> Self {
        Self {
            records: RecordArena::new(),
        }
    }

    pub(crate) fn push_record(&mut self, key: &[u8], value: &[u8]) {
        self.records.push_record(key, value);
    }

    pub(crate) fn finish_load(&mut self) {
        self.records.finish_load();
    }

    pub(crate) fn record(&self, key: &[u8]) -> Option<&[u8]> {
        self.records.record(key)
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.records.contains_key(key)
    }

    /// Visits every record in ascending key order, arena and overlay
    /// merged, without materialising a row vector.
    pub(crate) fn for_each_record<E>(
        &self,
        visit: impl FnMut(&[u8], &[u8]) -> Result<(), E>,
    ) -> Result<(), E> {
        self.records.for_each_record(visit)
    }

    /// `PhraseTableEntry::add_index` for `words`; creates the key and its
    /// prefix markers when absent, as `PhraseLargeTable3::add_index` does.
    pub(crate) fn add(&mut self, words: &[u32], token: u32) -> Result<i32, StoreError> {
        let key = key_ucs4(words);
        if key.is_empty() {
            return Err(StoreError::InvalidInput("phrase table text is empty"));
        }
        let (result, new) = match self.record(&key) {
            Some(bytes) => {
                let mut entry = PhraseTableEntry::from_bytes(bytes.to_vec())?;
                let result = entry.add_index(token);
                self.records.insert(key.clone(), entry.as_bytes().to_vec());
                (result, false)
            }
            None => {
                let mut entry = PhraseTableEntry::default();
                let result = entry.add_index(token);
                self.records.insert(key.clone(), entry.as_bytes().to_vec());
                (result, true)
            }
        };
        if new {
            for length in (1..words.len()).rev() {
                let prefix = key[..length * 4].to_vec();
                if self.contains_key(&prefix) {
                    break;
                }
                self.records.insert(prefix, Vec::new());
            }
        }
        Ok(result)
    }

    /// Fails exactly when [`Self::add`] would, without changing the table:
    /// an empty key, or a stored record with a partial token tail, whose
    /// entry walk the pin reads past (register row 101,
    /// `phrase_large_table3.cpp:57-65` at 074a2219).
    pub(crate) fn check_add(&self, words: &[u32]) -> Result<(), StoreError> {
        let key = key_ucs4(words);
        if key.is_empty() {
            return Err(StoreError::InvalidInput("phrase table text is empty"));
        }
        self.record(&key)
            .map_or(Ok(()), crate::phrase_table::validate)
    }

    /// Fails exactly when [`Self::mask`] would, without changing the table:
    /// any record with a partial token tail, whose entry walk the pin's mask
    /// reads past (register row 101, `phrase_large_table3.cpp:103-117`).
    pub(crate) fn check_mask(&self) -> Result<(), StoreError> {
        self.for_each_record(|_, value| crate::phrase_table::validate(value))
    }

    /// `PhraseTableEntry::remove_index` for `words` (first occurrence).
    pub(crate) fn remove(&mut self, words: &[u32], token: u32) -> Result<i32, StoreError> {
        let key = key_ucs4(words);
        let Some(bytes) = self.record(&key).map(<[u8]>::to_vec) else {
            return Ok(crate::phrase_table::ERROR_REMOVE_ITEM_DONOT_EXISTS);
        };
        let mut entry = PhraseTableEntry::from_bytes(bytes)?;
        let result = entry.remove_index(token);
        self.records.insert(key, entry.as_bytes().to_vec());
        Ok(result)
    }

    /// The pin's mask walk: one pass over every record in key order, as
    /// `PhraseLargeTable3::mask_out`'s `ProcessEach` makes it
    /// (`phrase_large_table3_tkrzwdb.cpp:304-315` and its Kyoto Cabinet and
    /// Berkeley DB twins at 074a2219). Kyoto Cabinet and tkrzw reuse one
    /// entry, so a shorter masked record can keep the previous one's tail;
    /// Berkeley DB masks each record in a fresh entry. The records are
    /// rewritten into a new arena in the same pass and the overlay is
    /// emptied, so no record is held twice. A record that fails the entry
    /// check stops the mask there: it and every later record keep their
    /// bytes, and the error is returned.
    pub(crate) fn mask(&mut self, mask: u32, value: u32) -> Result<(), StoreError> {
        #[cfg(not(feature = "bdb"))]
        let mut reused = PhraseTableEntry::default();
        let mut failed = None;
        self.records.rebuild(|_key, bytes, kept| {
            if failed.is_none() {
                #[cfg(not(feature = "bdb"))]
                let masked = reused.set_content(bytes).map(|()| {
                    reused.mask_out(mask, value);
                    reused.as_bytes()
                });
                #[cfg(feature = "bdb")]
                let mut entry = None;
                #[cfg(feature = "bdb")]
                let masked = PhraseTableEntry::from_bytes(bytes.to_vec()).map(|fresh| {
                    let fresh = entry.insert(fresh);
                    fresh.mask_out(mask, value);
                    fresh.as_bytes()
                });
                match masked {
                    Ok(masked) => {
                        kept.extend_from_slice(masked);
                        return;
                    }
                    Err(error) => failed = Some(error),
                }
            }
            kept.extend_from_slice(bytes);
        });
        failed.map_or(Ok(()), Err)
    }

    /// Tokens stored under the exact key, in record order.
    pub(crate) fn tokens(&self, key: &[u8]) -> Result<Vec<u32>, StoreError> {
        let Some(bytes) = self.record(key) else {
            return Ok(Vec::new());
        };
        let mut tokens = Vec::new();
        search_bytes(bytes, &mut tokens, |_| true)?;
        Ok(tokens)
    }

    /// Strictly longer keys after an existing prefix, with their tokens.
    pub(crate) fn suggest_after(&self, prefix: &[u8]) -> Result<SuggestionRows, StoreError> {
        if !self.contains_key(prefix) {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        let mut keys = self.records.keys_starting_with(prefix);
        keys.dedup();
        for key in keys {
            if key.len() <= prefix.len() {
                continue;
            }
            let Some(bytes) = self.record(&key) else {
                continue;
            };
            let mut tokens = Vec::new();
            search_bytes(bytes, &mut tokens, |_| true)?;
            out.push((key, tokens));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_remove_and_prefix_markers_match_the_entry_ops() {
        let mut table = UserPhraseTable::new();
        assert_eq!(table.add(&[0x4f60], 7).unwrap(), ERROR_OK);
        assert_eq!(table.add(&[0x4f60, 0x597d], 9).unwrap(), ERROR_OK);
        assert_eq!(table.record(&key_ucs4(&[0x4f60])), Some(&[7, 0, 0, 0][..]));
        assert_eq!(
            table.record(&key_ucs4(&[0x4f60, 0x597d])),
            Some(&[9, 0, 0, 0][..])
        );
        assert_eq!(table.remove(&[0x4f60, 0x597d], 9).unwrap(), ERROR_OK);
        assert!(
            table
                .record(&key_ucs4(&[0x4f60, 0x597d]))
                .is_some_and(<[u8]>::is_empty)
        );
    }
}
