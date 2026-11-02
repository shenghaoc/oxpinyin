//! Live system items: private replay views plus payloads decoded on first use.

use std::collections::BTreeMap;

use oxpinyin_data::chunk_write::ChunkItem;

use crate::replay::ReplayedLibrary;
use crate::store::Token;

#[derive(Default)]
pub(crate) struct SystemItems {
    decoded: BTreeMap<Token, ChunkItem>,
    replayed: BTreeMap<u8, ReplayedLibrary>,
}

impl SystemItems {
    pub(crate) fn new(
        decoded: BTreeMap<Token, ChunkItem>,
        replayed: BTreeMap<u8, ReplayedLibrary>,
    ) -> Self {
        Self { decoded, replayed }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.decoded.is_empty() && self.replayed.is_empty()
    }

    /// The item held under `token`: a decoded one, or a replayed library's
    /// buffer item decoded for this call only. Nothing is kept.
    pub(crate) fn get(&self, token: &Token) -> Option<std::borrow::Cow<'_, ChunkItem>> {
        if let Some(item) = self.decoded.get(token) {
            return Some(std::borrow::Cow::Borrowed(item));
        }
        let library = self.replayed.get(&((*token >> 24) as u8))?;
        library
            .item(*token & 0x00ff_ffff)
            .map(std::borrow::Cow::Owned)
    }

    /// Whether `token` holds a decoded item: an import, or one a mask or a
    /// reload materialized. Such an item is the authoritative value.
    pub(crate) fn has_decoded(&self, token: &Token) -> bool {
        self.decoded.contains_key(token)
    }

    /// The decoded item held under `token`, without decoding a replayed one.
    pub(crate) fn decoded(&self, token: &Token) -> Option<&ChunkItem> {
        self.decoded.get(token)
    }

    /// Reads the item the live replay buffer of `token`'s library holds under
    /// its slot, and whether the replay touched the slot.
    pub(crate) fn with_live<R>(
        &self,
        token: Token,
        read: impl FnOnce(&[u8], bool) -> R,
    ) -> Option<R> {
        let library = self.replayed.get(&((token >> 24) as u8))?;
        let (bytes, touched) = library.live_item(token & 0x00ff_ffff)?;
        Some(read(&bytes, touched))
    }

    pub(crate) fn live_item_past_buffer(&self, token: Token) -> bool {
        self.replayed
            .get(&((token >> 24) as u8))
            .is_some_and(|library| library.item_past_buffer(token & 0x00ff_ffff))
    }

    pub(crate) fn get_mut(&mut self, token: &Token) -> Option<&mut ChunkItem> {
        self.decoded.get_mut(token)
    }

    /// Trains `token`'s readings in place in its library's live buffer, which
    /// is made from `original` the first time. Returns whether the buffer
    /// held the item; `readings` is not consumed otherwise.
    pub(crate) fn train_live(
        &mut self,
        original: Option<crate::persistence::SystemLibraryRef<'_>>,
        token: Token,
        readings: impl Iterator<Item = Vec<oxpinyin_core::ChewingKey>>,
        delta: u32,
    ) -> bool {
        let nibble = (token >> 24) as u8;
        let slot = token & 0x00ff_ffff;
        let library = match self.replayed.entry(nibble) {
            std::collections::btree_map::Entry::Occupied(library) => library.into_mut(),
            std::collections::btree_map::Entry::Vacant(vacant) => {
                let Some(original) = original.filter(|library| library.contains_item(slot)) else {
                    return false;
                };
                vacant.insert(ReplayedLibrary::pristine(original))
            }
        };
        library.train(slot, readings, delta)
    }

    pub(crate) fn insert(&mut self, token: Token, item: ChunkItem) -> Option<ChunkItem> {
        self.decoded.insert(token, item)
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<Token, ChunkItem> {
        let mut items = BTreeMap::new();
        for (&library, replayed) in &self.replayed {
            for slot in replayed.slots() {
                let token = (u32::from(library) << 24) | slot;
                if !self.decoded.contains_key(&token)
                    && let Some(item) = replayed.item(slot)
                {
                    items.insert(token, item);
                }
            }
        }
        items.extend(self.decoded.clone());
        items
    }

    fn materialize(&mut self) {
        if self.replayed.is_empty() {
            return;
        }
        self.decoded = self.snapshot();
        self.replayed.clear();
    }

    pub(crate) fn retain(&mut self, keep: impl FnMut(&Token, &mut ChunkItem) -> bool) {
        self.materialize();
        self.decoded.retain(keep);
    }

    pub(crate) fn end(&self, library: u8) -> Option<Token> {
        let decoded = self
            .decoded
            .range((u32::from(library) << 24)..(u32::from(library + 1) << 24))
            .next_back()
            .map(|(&token, _)| token);
        let replayed = self
            .replayed
            .get(&library)
            .and_then(|view| view.slots().next_back())
            .map(|slot| (u32::from(library) << 24) | slot);
        decoded
            .into_iter()
            .chain(replayed)
            .max()
            .and_then(|token| token.checked_add(1))
    }
}
