//! The user phrase table as the pin's own records.
//!
//! `PhraseLargeTable3` holds `user_phrase_index.bin` as a DBM whose keys are
//! raw UCS-4 text and whose values are ascending token arrays. This module
//! keeps the table in one owned arena with a sorted offset vector, loaded in
//! one walk, and an overlay for session edits; the save merges the two in
//! key order. Empty prefix markers, duplicate tokens and malformed rows are
//! state and are kept verbatim.

use std::collections::BTreeMap;

use oxpinyin_store::StoreError;

use crate::phrase_table::{ERROR_OK, PhraseTableEntry, key_ucs4, search_bytes};

const HEADER: usize = 8;

#[derive(Clone, Debug, Default)]
pub(crate) struct UserPhraseTable {
    arena: Vec<u8>,
    offsets: Vec<u32>,
    overlay: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl UserPhraseTable {
    pub(crate) const fn new() -> Self {
        Self {
            arena: Vec::new(),
            offsets: Vec::new(),
            overlay: BTreeMap::new(),
        }
    }

    pub(crate) fn from_entries(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        let mut table = Self::new();
        for (key, value) in &entries {
            table.push_record(key, value);
        }
        table.finish_load();
        table
    }

    pub(crate) fn push_record(&mut self, key: &[u8], value: &[u8]) {
        let offset = u32::try_from(self.arena.len()).unwrap_or(u32::MAX);
        self.arena
            .extend_from_slice(&u32::try_from(key.len()).unwrap_or(u32::MAX).to_le_bytes());
        self.arena
            .extend_from_slice(&u32::try_from(value.len()).unwrap_or(u32::MAX).to_le_bytes());
        self.arena.extend_from_slice(key);
        self.arena.extend_from_slice(value);
        self.offsets.push(offset);
    }

    pub(crate) fn finish_load(&mut self) {
        let arena = &self.arena;
        self.offsets
            .sort_by(|left, right| key_at(arena, *left).cmp(key_at(arena, *right)));
    }

    pub(crate) fn record(&self, key: &[u8]) -> Option<&[u8]> {
        if let Some(value) = self.overlay.get(key) {
            return Some(value);
        }
        self.arena_record(key)
    }

    fn arena_record(&self, key: &[u8]) -> Option<&[u8]> {
        let at = self
            .offsets
            .binary_search_by(|offset| key_at(&self.arena, *offset).cmp(key))
            .ok()?;
        Some(value_at(&self.arena, self.offsets[at]))
    }

    pub(crate) fn contains_key(&self, key: &[u8]) -> bool {
        self.overlay.contains_key(key) || self.arena_record(key).is_some()
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
                self.overlay.insert(key.clone(), entry.as_bytes().to_vec());
                (result, false)
            }
            None => {
                let mut entry = PhraseTableEntry::default();
                let result = entry.add_index(token);
                self.overlay.insert(key.clone(), entry.as_bytes().to_vec());
                (result, true)
            }
        };
        if new {
            for length in (1..words.len()).rev() {
                let prefix = key[..length * 4].to_vec();
                if self.contains_key(&prefix) {
                    break;
                }
                self.overlay.insert(prefix, Vec::new());
            }
        }
        Ok(result)
    }

    /// `PhraseTableEntry::remove_index` for `words` (first occurrence).
    pub(crate) fn remove(&mut self, words: &[u32], token: u32) -> Result<i32, StoreError> {
        let key = key_ucs4(words);
        let Some(bytes) = self.record(&key).map(<[u8]>::to_vec) else {
            return Ok(crate::phrase_table::ERROR_REMOVE_ITEM_DONOT_EXISTS);
        };
        let mut entry = PhraseTableEntry::from_bytes(bytes)?;
        let result = entry.remove_index(token);
        self.overlay.insert(key, entry.as_bytes().to_vec());
        Ok(result)
    }

    /// The pin's mask walk over every record. Kyoto Cabinet and tkrzw reuse
    /// one entry, so a shorter masked record can keep the previous one's
    /// tail; Berkeley DB masks each record in a fresh entry.
    pub(crate) fn mask(&mut self, mask: u32, value: u32) -> Result<(), StoreError> {
        let rows = self.rows();
        #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
        let mut reused = PhraseTableEntry::default();
        for (key, bytes) in rows {
            #[cfg(feature = "bdb")]
            let mut entry = PhraseTableEntry::from_bytes(bytes)?;
            #[cfg(any(feature = "kyotocabinet", feature = "tkrzw"))]
            {
                reused.set_content(&bytes)?;
                reused.mask_out(mask, value);
                self.overlay.insert(key, reused.as_bytes().to_vec());
            }
            #[cfg(feature = "bdb")]
            {
                entry.mask_out(mask, value);
                self.overlay.insert(key, entry.as_bytes().to_vec());
            }
        }
        Ok(())
    }

    pub(crate) fn rows(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut rows: Vec<(Vec<u8>, Vec<u8>)> = self
            .offsets
            .iter()
            .map(|offset| {
                let key = key_at(&self.arena, *offset);
                let value = self
                    .overlay
                    .get(key)
                    .cloned()
                    .unwrap_or_else(|| value_at(&self.arena, *offset).to_vec());
                (key.to_vec(), value)
            })
            .collect();
        for (key, value) in &self.overlay {
            if !self.contains_arena_key(key) {
                rows.push((key.clone(), value.clone()));
            }
        }
        rows.sort_by(|left, right| left.0.cmp(&right.0));
        rows
    }

    fn contains_arena_key(&self, key: &[u8]) -> bool {
        self.offsets
            .binary_search_by(|offset| key_at(&self.arena, *offset).cmp(key))
            .is_ok()
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
    pub(crate) fn suggest_after(
        &self,
        prefix: &[u8],
    ) -> Result<Vec<(Vec<u8>, Vec<u32>)>, StoreError> {
        if !self.contains_key(prefix) {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for key in self.keys_starting_with(prefix) {
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

    fn keys_starting_with(&self, prefix: &[u8]) -> Vec<Vec<u8>> {
        let mut keys = Vec::new();
        let start = self
            .offsets
            .partition_point(|offset| key_at(&self.arena, *offset) < prefix);
        for offset in &self.offsets[start..] {
            let key = key_at(&self.arena, *offset);
            if !key.starts_with(prefix) {
                break;
            }
            keys.push(key.to_vec());
        }
        for (key, _) in self.overlay.range(prefix.to_vec()..) {
            if !key.starts_with(prefix) {
                break;
            }
            if !self.contains_arena_key(key) {
                keys.push(key.clone());
            }
        }
        keys.sort();
        keys.dedup();
        keys
    }
}

fn key_at(arena: &[u8], offset: u32) -> &[u8] {
    let at = offset as usize;
    let len = u32::from_le_bytes([arena[at], arena[at + 1], arena[at + 2], arena[at + 3]]) as usize;
    &arena[at + HEADER..at + HEADER + len]
}

fn value_at(arena: &[u8], offset: u32) -> &[u8] {
    let at = offset as usize;
    let key_len =
        u32::from_le_bytes([arena[at], arena[at + 1], arena[at + 2], arena[at + 3]]) as usize;
    let value_len =
        u32::from_le_bytes([arena[at + 4], arena[at + 5], arena[at + 6], arena[at + 7]]) as usize;
    &arena[at + HEADER + key_len..at + HEADER + key_len + value_len]
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
