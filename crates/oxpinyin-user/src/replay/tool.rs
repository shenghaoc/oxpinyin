//! Nonshipping access to the existing live phrase buffer for the pin's tests.
use super::*;
use crate::persistence::SystemLibrary;

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
    /// Read an item through the existing whole-buffer bounds checks.
    #[must_use]
    pub fn item(&self, slot: u32) -> Option<ChunkItem> {
        self.live.item(slot)
    }

    /// Append the item as SubPhraseIndex::add_phrase_item does, without compacting.
    pub fn add(&mut self, slot: u32, item: &ChunkItem) {
        // This Index keeps no separate visibility cache to invalidate.
        let _ = self.live.add_item(slot, &journal_item_bytes(item));
    }
}
