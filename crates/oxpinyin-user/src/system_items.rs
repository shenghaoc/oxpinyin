//! Live system items: each loaded system library's private buffer.

use std::collections::BTreeMap;

use oxpinyin_data::chunk_write::ChunkItem;

use crate::persistence::SystemLibraryRef;
use crate::replay::{ReplayedLibrary, UnigramAdd};
use crate::store::Token;

/// The live buffer of system library `nibble`, made from `original` the first
/// time with the total a reload left.
fn library_of<'a>(
    replayed: &'a mut BTreeMap<u8, ReplayedLibrary>,
    totals: &mut BTreeMap<u8, u32>,
    nibble: u8,
    original: SystemLibraryRef<'_>,
) -> &'a mut ReplayedLibrary {
    match replayed.entry(nibble) {
        std::collections::btree_map::Entry::Occupied(library) => library.into_mut(),
        std::collections::btree_map::Entry::Vacant(vacant) => {
            let mut library = ReplayedLibrary::pristine(original);
            if let Some(total) = totals.remove(&nibble) {
                library.set_total(total);
            }
            vacant.insert(library)
        }
    }
}

/// What the session holds of the loaded system libraries beyond the shipped
/// chunks: one live buffer per library a replay, a training, an import or an
/// increment has written, whose items and `guint32` total are the
/// sub-index's. A library without one is its shipped chunk.
#[derive(Debug, Default)]
pub(crate) struct SystemItems {
    replayed: BTreeMap<u8, ReplayedLibrary>,
    /// The sub-index total of a library that holds no live buffer but whose
    /// total is not its original's: what a reload's replay left.
    totals: BTreeMap<u8, u32>,
    /// `FacadePhraseIndex::m_total_freq` beyond the sub-index totals of the
    /// system libraries: the user libraries' totals and every delta the facade
    /// took that a sub-index refused or could not take (`phrase_index.h:633`
    /// adds before the sub-index answers).
    facade_extra: u64,
}

impl SystemItems {
    pub(crate) fn new(replayed: BTreeMap<u8, ReplayedLibrary>) -> Self {
        Self {
            replayed,
            totals: BTreeMap::new(),
            facade_extra: 0,
        }
    }

    /// The sub-index totals of libraries without a live buffer and the facade
    /// total's extra, as an open or a reload leaves them.
    pub(crate) fn with_totals(mut self, totals: BTreeMap<u8, u32>, facade_extra: u64) -> Self {
        self.totals = totals;
        self.facade_extra = facade_extra;
        self
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.replayed.is_empty()
    }

    /// `SubPhraseIndex::m_total_freq` of system library `nibble`: its live
    /// buffer's, a reload's, or the original's.
    pub(crate) fn library_total(
        &self,
        nibble: u8,
        original: Option<SystemLibraryRef<'_>>,
    ) -> Option<u32> {
        if let Some(library) = self.replayed.get(&nibble) {
            return Some(library.total());
        }
        self.totals
            .get(&nibble)
            .copied()
            .or_else(|| original.map(|library| library.total))
    }

    /// Sets a system library's sub-index total outside a replay.
    pub(crate) fn set_library_total(&mut self, nibble: u8, total: u32) {
        match self.replayed.get_mut(&nibble) {
            Some(library) => library.set_total(total),
            None => {
                self.totals.insert(nibble, total);
            }
        }
    }

    /// Forgets a system library's live buffer and total.
    pub(crate) fn forget_library(&mut self, nibble: u8) {
        self.replayed.remove(&nibble);
        self.totals.remove(&nibble);
    }

    /// Replaces a system library's live buffer with the one a replay of its
    /// freshly mapped chunk left (`FacadePhraseIndex::load` then `merge`).
    pub(crate) fn insert_library(&mut self, nibble: u8, library: ReplayedLibrary) {
        self.totals.remove(&nibble);
        self.replayed.insert(nibble, library);
    }

    pub(crate) fn facade_extra(&self) -> u64 {
        self.facade_extra
    }

    pub(crate) fn add_facade_extra(&mut self, delta: u64) {
        self.facade_extra = self.facade_extra.wrapping_add(delta);
    }

    pub(crate) fn sub_facade_extra(&mut self, delta: u64) {
        self.facade_extra = self.facade_extra.wrapping_sub(delta);
    }

    /// Whether system library `nibble` has no sub-index: its baseline gave its
    /// mapping to a live buffer, and no buffer holds it now. An unload drops
    /// the buffer, and a reload that mapped nothing never makes another. The
    /// pin has no `SubPhraseIndex` there: `add_unigram_frequency` answers
    /// `ERROR_NO_SUB_PHRASE_INDEX` before it moves the facade total
    /// (`phrase_index.h:631-633`), `_add_phrase`'s `get_range` fails
    /// (`pinyin.cpp:589-591`) and `compact` skips the library
    /// (`phrase_index.cpp:651-652`). A buffer made from the emptied baseline
    /// would read arrays it no longer holds ([`library_of`]).
    pub(crate) fn lacks_sub_index(&self, nibble: u8, original: SystemLibraryRef<'_>) -> bool {
        !self.replayed.contains_key(&nibble)
            && original.mapped.is_some_and(|library| library.is_detached())
    }

    /// `FacadePhraseIndex::compact`'s sub-index total (074a2219
    /// `phrase_index.cpp:648-675`): the sum of the readable items the library
    /// holds. A library with a live buffer is walked, as the pin's compact
    /// walks its sub-index; one without is its original's own cached sum.
    pub(crate) fn compacted_total(&self, nibble: u8, original: SystemLibraryRef<'_>) -> u32 {
        match self.replayed.get(&nibble) {
            Some(buffer) => buffer.readable_unigram_sum(),
            None => original.item_total(),
        }
    }

    /// The live sub-index of system library `nibble` for a save's diff: its
    /// buffer, or the shipped chunk where no write has made one.
    pub(crate) fn live_library<'a>(
        &'a self,
        nibble: u8,
        original: SystemLibraryRef<'a>,
    ) -> crate::persistence::LiveSubIndex<'a> {
        match self.replayed.get(&nibble) {
            Some(buffer) => crate::persistence::LiveSubIndex::Buffer(buffer),
            None => crate::persistence::LiveSubIndex::Chunk(original),
        }
    }

    /// The live unigram of `token`'s system item: the library's live buffer's,
    /// else the original's (no write has made a buffer). `None` when no
    /// readable item is there.
    pub(crate) fn unigram(
        &self,
        token: Token,
        original: Option<SystemLibraryRef<'_>>,
    ) -> Option<u32> {
        let slot = token & 0x00ff_ffff;
        match self.replayed.get(&((token >> 24) as u8)) {
            Some(library) => library.unigram(slot),
            None => original?.unigram(slot),
        }
    }

    /// `SubPhraseIndex::get_range`'s end of system library `nibble`: its live
    /// buffer's, else the original's.
    pub(crate) fn range_end(&self, nibble: u8, original: SystemLibraryRef<'_>) -> u32 {
        self.replayed
            .get(&nibble)
            .map_or(original.range_end.max(1), ReplayedLibrary::range_end)
    }

    /// `add_phrase_item` over `token`'s library buffer.
    pub(crate) fn add_item(
        &mut self,
        original: SystemLibraryRef<'_>,
        token: Token,
        payload: &[u8],
    ) -> bool {
        library_of(
            &mut self.replayed,
            &mut self.totals,
            (token >> 24) as u8,
            original,
        )
        .add_item(token & 0x00ff_ffff, payload)
    }

    /// `remove_phrase_item` over `token`'s library buffer: the item's bytes.
    pub(crate) fn remove_item(
        &mut self,
        original: SystemLibraryRef<'_>,
        token: Token,
    ) -> Option<Vec<u8>> {
        library_of(
            &mut self.replayed,
            &mut self.totals,
            (token >> 24) as u8,
            original,
        )
        .remove_item(token & 0x00ff_ffff)
    }

    /// Whether any live buffer holds an item its original lacks.
    pub(crate) fn has_added(&self) -> bool {
        self.replayed.values().any(ReplayedLibrary::has_added)
    }

    /// Whether any live buffer had an item of its original taken out.
    pub(crate) fn has_removed(&self) -> bool {
        self.replayed.values().any(ReplayedLibrary::has_removed)
    }

    /// Whether a journal REMOVE took `token`'s item out of its sub-index: the
    /// library's buffer answers no item where the chunk holds one
    /// (`get_phrase_item` fails, 074a2219 `phrase_index.cpp:180-206`).
    pub(crate) fn is_removed(&self, token: Token, original: Option<SystemLibraryRef<'_>>) -> bool {
        let slot = token & 0x00ff_ffff;
        self.replayed
            .get(&((token >> 24) as u8))
            .is_some_and(|library| {
                // A mapped original's bytes are the buffer's own: what the
                // chunk held is the buffer's record of its slots' first
                // writes. A fixture's inline items answer for themselves.
                let original_held = original.map_or_else(
                    || library.slot_has_offset(slot),
                    |original| match original.mapped {
                        Some(_) => library.originally_held(slot),
                        None => original.contains_item(slot),
                    },
                );
                original_held && library.item_bytes(slot).is_none()
            })
    }

    /// `FacadePhraseIndex::add_unigram_frequency`'s sub-index half
    /// (`phrase_index.cpp:150-176`) for system library `token >> 24`. `None`
    /// for a library with no original. The sub-index total moves with an
    /// accepted add; the caller moves the facade's.
    pub(crate) fn add_unigram(
        &mut self,
        original: Option<SystemLibraryRef<'_>>,
        token: Token,
        delta: u32,
    ) -> Option<UnigramAdd> {
        let original = original?;
        let nibble = (token >> 24) as u8;
        let slot = token & 0x00ff_ffff;
        let library = library_of(&mut self.replayed, &mut self.totals, nibble, original);
        Some(library.add_unigram(slot, delta))
    }

    /// The item held under `token`, decoded for this call only. Nothing is
    /// kept.
    pub(crate) fn get(&self, token: &Token) -> Option<ChunkItem> {
        self.replayed
            .get(&((*token >> 24) as u8))?
            .item(*token & 0x00ff_ffff)
    }

    /// Reads the item the live buffer of `token`'s library holds under its
    /// slot.
    pub(crate) fn with_live<R>(&self, token: Token, read: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let library = self.replayed.get(&((token >> 24) as u8))?;
        let bytes = library.item_bytes(token & 0x00ff_ffff)?;
        Some(read(&bytes))
    }

    pub(crate) fn pristine_item_count(&self, nibble: u8) -> Option<u64> {
        self.replayed
            .get(&nibble)
            .map(ReplayedLibrary::readable_item_count)
    }

    pub(crate) fn live_pronunciation_count(&self, token: Token) -> Option<u32> {
        self.replayed
            .get(&((token >> 24) as u8))?
            .pronunciation_count(token & 0x00ff_ffff)
    }

    pub(crate) fn live_item_past_buffer(&self, token: Token) -> bool {
        self.replayed
            .get(&((token >> 24) as u8))
            .is_some_and(|library| library.item_past_buffer(token & 0x00ff_ffff))
    }

    /// Trains `token`'s readings in place in its library's live buffer, which
    /// is made from `original` the first time. Returns whether the buffer
    /// held the item; `readings` is not consumed otherwise.
    pub(crate) fn train_live(
        &mut self,
        original: Option<SystemLibraryRef<'_>>,
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
                let mut library = ReplayedLibrary::pristine(original);
                if let Some(total) = self.totals.remove(&nibble) {
                    library.set_total(total);
                }
                vacant.insert(library)
            }
        };
        library.train(slot, readings, delta)
    }

    /// Whether `token`'s library has a live buffer whose item under the
    /// token's slot, if any, is one its loaded chunk lacked: an import's or a
    /// replayed ADD's.
    pub(crate) fn is_added(&self, token: Token) -> bool {
        self.replayed
            .get(&((token >> 24) as u8))
            .is_some_and(|library| library.is_added_slot(token & 0x00ff_ffff))
    }

    /// Reads the item under `token` where it is one its library's loaded
    /// chunk lacked ([`Self::is_added`]).
    pub(crate) fn with_added<R>(&self, token: Token, read: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let library = self.replayed.get(&((token >> 24) as u8))?;
        let slot = token & 0x00ff_ffff;
        if !library.is_added_slot(slot) {
            return None;
        }
        let bytes = library.item_bytes(slot)?;
        Some(read(&bytes))
    }

    /// Exclusive token end of the items one library's buffer added, for the
    /// export's range probe: `None` where it added none.
    pub(crate) fn end(&self, library: u8) -> Option<Token> {
        let buffer = self.replayed.get(&library)?;
        buffer
            .has_added()
            .then(|| (u32::from(library) << 24) | buffer.range_end())
    }

    /// Whether library `nibble`'s live buffer records any slot's provenance.
    #[cfg(test)]
    pub(crate) fn has_provenance(&self, nibble: u8) -> bool {
        self.replayed
            .get(&nibble)
            .is_some_and(ReplayedLibrary::has_provenance)
    }

    /// The items each live buffer holds that differ from its original's, as
    /// values: a changed or added item, and `None` where the buffer holds no
    /// item at a slot the chunk holds. For the tests' value-model export.
    #[cfg(test)]
    pub(crate) fn changed_items(
        &self,
        originals: &impl crate::persistence::OriginalLibraries,
    ) -> BTreeMap<Token, Option<ChunkItem>> {
        let mut changes = BTreeMap::new();
        for (&nibble, buffer) in &self.replayed {
            let original = originals.get(&nibble);
            let end = buffer
                .range_end()
                .max(original.map_or(0, |library| library.range_end));
            for slot in 1..end {
                let token = (u32::from(nibble) << 24) | slot;
                let now = buffer.item(slot);
                let before = original.and_then(|library| library.item(slot));
                if now != before {
                    changes.insert(token, now);
                }
            }
        }
        changes
    }
}
