//! The user phrase table as the pin's own records.
//!
//! `PhraseLargeTable3` holds `user_phrase_index.bin` as a DBM whose keys are
//! raw UCS-4 text and whose values are ascending token arrays. The
//! session uses the backend's in-memory tree and keeps empty prefix markers,
//! duplicate tokens and malformed rows verbatim.

use oxpinyin_store::StoreError;

#[cfg(test)]
use crate::phrase_table::ERROR_OK;
use crate::phrase_table::{PhraseTableEntry, key_ucs4, search_bytes};
use crate::records::NativeTable;

/// One suggestion row: the raw key and its stored tokens.
type SuggestionRows = Vec<(Vec<u8>, Vec<u32>)>;

#[derive(Debug, Default)]
pub(crate) struct UserPhraseTable {
    records: NativeTable,
}

impl UserPhraseTable {
    #[cfg(test)]
    pub(crate) fn new() -> Self {
        Self {
            records: NativeTable::default(),
        }
    }

    pub(crate) fn push_record(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.records.insert(key, value)
    }

    pub(crate) fn record(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.records.record(key)
    }

    pub(crate) fn save(&self, path: &std::path::Path) -> Result<(), StoreError> {
        self.records.save(path)
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.records.contains_key(key)
    }

    /// Visits records in native key order.
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
                let mut entry = PhraseTableEntry::from_bytes(bytes)?;
                let result = entry.add_index(token);
                self.records.insert(&key, entry.as_bytes())?;
                (result, false)
            }
            None => {
                let mut entry = PhraseTableEntry::default();
                let result = entry.add_index(token);
                self.records.insert(&key, entry.as_bytes())?;
                (result, true)
            }
        };
        if new {
            for length in (1..words.len()).rev() {
                let prefix = key[..length * 4].to_vec();
                if self.contains_key(&prefix) {
                    break;
                }
                self.records.insert(&prefix, &[])?;
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
            .as_deref()
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
        let Some(bytes) = self.record(&key) else {
            return Ok(crate::phrase_table::ERROR_REMOVE_ITEM_DONOT_EXISTS);
        };
        let mut entry = PhraseTableEntry::from_bytes(bytes)?;
        let result = entry.remove_index(token);
        self.records.insert(&key, entry.as_bytes())?;
        Ok(result)
    }

    /// The pin's mask walk: one pass over every record in key order, as
    /// `PhraseLargeTable3::mask_out`'s `ProcessEach` makes it
    /// (`phrase_large_table3_tkrzwdb.cpp:304-315` and its Kyoto Cabinet and
    /// Berkeley DB twins at 074a2219). Kyoto Cabinet and tkrzw reuse one
    /// entry, so a shorter masked record can keep the previous one's tail;
    /// Berkeley DB masks each record in a fresh entry. The caller has
    /// validated every row before invoking the writable walk.
    pub(crate) fn mask(&mut self, mask: u32, value: u32) -> Result<(), StoreError> {
        #[cfg(not(feature = "bdb"))]
        let mut reused = PhraseTableEntry::default();
        self.records.rewrite(&mut |_key, bytes, kept| {
            #[cfg(not(feature = "bdb"))]
            {
                reused.set_validated_content(bytes);
                reused.mask_out(mask, value);
                kept.extend_from_slice(reused.as_bytes());
            }
            #[cfg(feature = "bdb")]
            {
                let mut entry = PhraseTableEntry::from_validated_bytes(bytes.to_vec());
                entry.mask_out(mask, value);
                kept.extend_from_slice(entry.as_bytes());
            }
        })
    }

    /// Tokens stored under the exact key, in record order.
    pub(crate) fn tokens(&self, key: &[u8]) -> Result<Vec<u32>, StoreError> {
        let Some(bytes) = self.record(key) else {
            return Ok(Vec::new());
        };
        let mut tokens = Vec::new();
        search_bytes(&bytes, &mut tokens, |_| true)?;
        Ok(tokens)
    }

    /// Strictly longer keys after an existing prefix, with their tokens.
    pub(crate) fn suggest_after(&self, prefix: &[u8]) -> Result<SuggestionRows, StoreError> {
        if !self.contains_key(prefix) {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for key in self.records.keys_starting_with(prefix) {
            if key.len() <= prefix.len() {
                continue;
            }
            let Some(bytes) = self.record(&key) else {
                continue;
            };
            let mut tokens = Vec::new();
            search_bytes(&bytes, &mut tokens, |_| true)?;
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
        assert_eq!(
            table.record(&key_ucs4(&[0x4f60])).as_deref(),
            Some(&[7, 0, 0, 0][..])
        );
        assert_eq!(
            table.record(&key_ucs4(&[0x4f60, 0x597d])).as_deref(),
            Some(&[9, 0, 0, 0][..])
        );
        assert_eq!(table.remove(&[0x4f60, 0x597d], 9).unwrap(), ERROR_OK);
        assert!(
            table
                .record(&key_ucs4(&[0x4f60, 0x597d]))
                .is_some_and(|bytes| bytes.is_empty())
        );
    }
}
