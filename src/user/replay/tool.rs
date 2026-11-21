//! Nonshipping access to live phrase items for tools.
use super::*;
use crate::user::SystemLibrary;

/// One heap-loaded SubPhraseIndex, using the same item storage as journal replay.
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
    /// Read a complete phrase item.
    #[must_use]
    pub fn item(&self, slot: u32) -> Option<ChunkItem> {
        self.live.item(slot)
    }

    /// Set the slot and add the item's unigram to the total.
    pub fn add(&mut self, slot: u32, item: &ChunkItem) {
        self.live.add_item(slot, &journal_item_bytes(item));
    }
}
