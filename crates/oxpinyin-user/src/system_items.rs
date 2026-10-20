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

    pub(crate) fn contains_key(&self, token: &Token) -> bool {
        self.decoded.contains_key(token)
            || self
                .replayed
                .get(&((*token >> 24) as u8))
                .is_some_and(|library| library.contains(*token & 0x00ff_ffff))
    }

    pub(crate) fn get(&mut self, token: &Token) -> Option<&ChunkItem> {
        if !self.decoded.contains_key(token) {
            let library = self.replayed.get(&((*token >> 24) as u8))?;
            self.decoded
                .insert(*token, library.item(*token & 0x00ff_ffff)?);
        }
        self.decoded.get(token)
    }

    pub(crate) fn get_mut(&mut self, token: &Token) -> Option<&mut ChunkItem> {
        self.get(token)?;
        self.decoded.get_mut(token)
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

    pub(crate) fn keys(&mut self) -> std::collections::btree_map::Keys<'_, Token, ChunkItem> {
        self.materialize();
        self.decoded.keys()
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
