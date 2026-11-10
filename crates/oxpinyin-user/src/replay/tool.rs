//! Nonshipping access to the existing live phrase buffer for the pin's tests.
use super::*;
use crate::persistence::{LiveSubIndex, SystemLibrary, diff_live_records};
use oxpinyin_data::user_files::{LogFault, encode_log_records, log_records};

/// One heap-loaded SubPhraseIndex, using the same byte arena as journal replay.
pub struct Index {
    live: ReplayedLibrary,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            live: ReplayedLibrary::pristine(SystemLibrary::failed(0).view()),
        }
    }
}

impl Index {
    /// Load the library's total and both byte areas, retaining their layout.
    #[must_use]
    pub fn from_library(library: PhraseLibrary) -> Self {
        Self {
            live: ReplayedLibrary::pristine(SystemLibrary::from_library(library).view()),
        }
    }

    /// The stored total, including an inconsistent foreign header.
    #[must_use]
    pub fn total(&self) -> u32 {
        self.live.total()
    }

    /// The local token range, including holes and trimming trailing zero slots.
    #[must_use]
    pub fn range(&self) -> core::ops::Range<u32> {
        1..self.live.range_end()
    }

    /// Read an item through the existing whole-buffer bounds checks.
    #[must_use]
    pub fn item(&self, slot: u32) -> Option<ChunkItem> {
        self.live.item(slot)
    }

    /// The pin's wrapping matched/total pronunciation ratio over stored rows.
    #[must_use]
    pub fn possibility(&self, slot: u32, keys: &[oxpinyin_core::ChewingKey]) -> Option<f32> {
        let item = self.item(slot)?;
        let mut total = 0_u32;
        let mut matched = 0_u32;
        for (stored, frequency) in item.prons {
            total = total.wrapping_add(frequency);
            if crate::store_libpinyin::pronunciation_matches(keys, stored.into_iter()) {
                matched = matched.wrapping_add(frequency);
            }
        }
        Some(if total == 0 {
            0.0
        } else {
            matched as f32 / total as f32
        })
    }

    /// Append the item as SubPhraseIndex::add_phrase_item does, without compacting.
    pub fn add(&mut self, slot: u32, item: &ChunkItem) {
        // This Index keeps no separate visibility cache to invalidate.
        let _ = self.live.add_item(slot, &journal_item_bytes(item));
    }

    /// Apply the sub-index's overflow-checked unigram change.
    pub fn add_unigram(&mut self, slot: u32, delta: u32) -> bool {
        self.live.add_unigram(slot, delta).is_accepted()
    }

    /// Mutate matching pronunciation counts in the resident buffer.
    pub fn increase(&mut self, slot: u32, keys: &[oxpinyin_core::ChewingKey], delta: u32) {
        self.live
            .train(slot, core::iter::once(keys.to_vec()), delta);
    }

    /// Copy readable items in ascending slot order, preserving the pin's compact loop.
    pub fn compact(&mut self) {
        let mut next = Self::default();
        for slot in self.range() {
            if let Some(bytes) = self.live.item_bytes(slot) {
                // This Index keeps no separate visibility cache to invalidate.
                let _ = next.live.add_item(slot, &bytes);
            }
        }
        *self = next;
    }

    /// SubPhraseIndex::store: total, raw offset array and raw content, framed.
    /// Returns None if the resulting frame cannot fit its length field.
    #[must_use]
    pub fn store(&self) -> Option<Vec<u8>> {
        let area = |chunk: &Chunk, which| {
            chunk
                .owned
                .as_deref()
                .unwrap_or_else(|| self.live.content.backing.bytes(which))[..chunk.data_end]
                .to_vec()
        };
        let offsets = area(&self.live.content.slots, Area::Offsets);
        let content = area(&self.live.content.items, Area::Content);
        let second = 18_usize.checked_add(offsets.len())?;
        let third = second.checked_add(content.len())?.checked_add(1)?;
        let mut payload = Vec::with_capacity(third);
        payload.extend_from_slice(&self.total().to_le_bytes());
        payload.extend_from_slice(&17_u32.to_le_bytes());
        payload.extend_from_slice(&u32::try_from(second).ok()?.to_le_bytes());
        payload.extend_from_slice(&u32::try_from(third).ok()?.to_le_bytes());
        payload.push(oxpinyin_data::chunk_format::SEPARATOR);
        payload.extend_from_slice(&offsets);
        payload.push(oxpinyin_data::chunk_format::SEPARATOR);
        payload.extend_from_slice(&content);
        payload.push(oxpinyin_data::chunk_format::SEPARATOR);
        oxpinyin_data::chunk_format::build_memory_chunk(&payload).ok()
    }

    /// Diff the live bytes against a separately heap-loaded old chunk.
    #[must_use]
    pub fn diff(&self, old: PhraseLibrary) -> Option<Vec<u8>> {
        let old = SystemLibrary::from_library(old);
        let records = diff_live_records(LiveSubIndex::Buffer(&self.live), old.view(), self.total());
        encode_log_records(&records).ok()
    }

    /// Replay with the existing journal walk and merge, preserving applied prefixes.
    ///
    /// # Errors
    /// Returns the pin assertion reached by the record walk or short ADD.
    pub fn merge(&mut self, log: &[u8]) -> Result<bool, JournalFault> {
        let live = core::mem::take(self).live;
        let mut merge = Merge::from_live(None, live);
        let mut matched = true;
        for record in log_records(log) {
            let record = match record {
                Ok(record) => record,
                Err(LogFault::HeaderToken(token)) => {
                    return Err(JournalFault::HeaderToken { library: 1, token });
                }
                Err(LogFault::OverRead) => {
                    self.live = merge.rollback();
                    return Ok(true);
                }
            };
            match merge.apply(record) {
                Ok(()) => {}
                Err(Stop::ShortAdd(token)) => {
                    return Err(JournalFault::ShortAdd { library: 1, token });
                }
                Err(Stop::Mismatch(_, _)) => {
                    matched = false;
                    break;
                }
            }
        }
        let (_, _, live) = merge.finish_views(&mut None, &"test_phrase_index_logger");
        if let Some(live) = live {
            self.live = live;
        }
        Ok(matched)
    }
}
