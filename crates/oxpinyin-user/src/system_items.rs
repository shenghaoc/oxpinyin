//! Live system items: private replay views plus payloads decoded on first use.

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

#[derive(Debug, Default)]
pub(crate) struct SystemItems {
    decoded: BTreeMap<Token, ChunkItem>,
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
    pub(crate) fn new(
        decoded: BTreeMap<Token, ChunkItem>,
        replayed: BTreeMap<u8, ReplayedLibrary>,
    ) -> Self {
        Self {
            decoded,
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
        self.decoded.is_empty() && self.replayed.is_empty()
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

    pub(crate) fn facade_extra(&self) -> u64 {
        self.facade_extra
    }

    pub(crate) fn add_facade_extra(&mut self, delta: u64) {
        self.facade_extra = self.facade_extra.wrapping_add(delta);
    }

    pub(crate) fn sub_facade_extra(&mut self, delta: u64) {
        self.facade_extra = self.facade_extra.wrapping_sub(delta);
    }

    /// `FacadePhraseIndex::compact`'s sub-index total (074a2219
    /// `phrase_index.cpp:648-675`): the sum of the readable items the library
    /// holds, as each is now: an arena item's field, a decoded item's, the live
    /// buffer's, the original's, and nothing for a removed one. Only what the
    /// session changed is visited; the original's own sum is cached.
    pub(crate) fn compacted_total(
        &self,
        nibble: u8,
        original: SystemLibraryRef<'_>,
        arena: Option<&crate::user_library::UserLibrary>,
        removed: impl Iterator<Item = Token>,
    ) -> u32 {
        let base = |slot: u32| original.unigram(slot).unwrap_or(0);
        let mut total = original.item_total();
        let mut handled = std::collections::BTreeSet::new();
        let mut take = |slot: u32, now: u32, total: &mut u32| {
            if handled.insert(slot) {
                *total = total.wrapping_add(now).wrapping_sub(base(slot));
            }
        };
        if let Some(arena) = arena {
            for (slot, item) in arena.items() {
                take(slot, item.unigram(), &mut total);
            }
        }
        let lo = u32::from(nibble) << 24;
        let hi = lo | 0x00ff_ffff;
        for (&token, item) in self.decoded.range(lo..=hi) {
            take(token & 0x00ff_ffff, item.unigram, &mut total);
        }
        if let Some(buffer) = self.replayed.get(&nibble) {
            for slot in buffer.slots() {
                take(slot, buffer.unigram(slot).unwrap_or(0), &mut total);
            }
        }
        for token in removed {
            take(token & 0x00ff_ffff, 0, &mut total);
        }
        total
    }

    /// The live unigram of `token`'s system item: the decoded item's, else the
    /// library's live buffer's, else the original's (no write has made a
    /// buffer). `None` when no readable item is there.
    pub(crate) fn unigram(
        &self,
        token: Token,
        original: Option<SystemLibraryRef<'_>>,
    ) -> Option<u32> {
        if let Some(item) = self.decoded.get(&token) {
            return Some(item.unigram);
        }
        let slot = token & 0x00ff_ffff;
        match self.replayed.get(&((token >> 24) as u8)) {
            Some(library) => library.unigram(slot),
            None => original?.unigram(slot),
        }
    }

    /// `SubPhraseIndex::get_range`'s end of system library `nibble`: its live
    /// buffer's, else the original's.
    pub(crate) fn range_end(&self, nibble: u8, original: SystemLibraryRef<'_>) -> u32 {
        let live = self
            .replayed
            .get(&nibble)
            .map_or(original.range_end.max(1), ReplayedLibrary::range_end);
        let lo = u32::from(nibble) << 24;
        let decoded = self
            .decoded
            .range(lo..=(lo | 0x00ff_ffff))
            .next_back()
            .map_or(0, |(&token, _)| (token & 0x00ff_ffff) + 1);
        live.max(decoded)
    }

    /// `add_phrase_item` over `token`'s library buffer.
    pub(crate) fn add_item(
        &mut self,
        original: SystemLibraryRef<'_>,
        token: Token,
        payload: &[u8],
    ) {
        library_of(
            &mut self.replayed,
            &mut self.totals,
            (token >> 24) as u8,
            original,
        )
        .add_item(token & 0x00ff_ffff, payload);
    }

    /// Adds an item a load already counted in the library's total: the buffer
    /// takes the payload and the total stays as the load left it.
    pub(crate) fn load_item(
        &mut self,
        original: SystemLibraryRef<'_>,
        token: Token,
        payload: &[u8],
    ) {
        let library = library_of(
            &mut self.replayed,
            &mut self.totals,
            (token >> 24) as u8,
            original,
        );
        let total = library.total();
        library.add_item(token & 0x00ff_ffff, payload);
        library.set_total(total);
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

    /// `FacadePhraseIndex::add_unigram_frequency`'s sub-index half
    /// (`phrase_index.cpp:150-176`) for system library `token >> 24`, whose
    /// item is the arena's when `arena`, else the decoded item or the live
    /// buffer's. `None` for a library with no original. The sub-index total
    /// moves with an accepted add; the caller moves the facade's.
    pub(crate) fn add_unigram(
        &mut self,
        original: Option<SystemLibraryRef<'_>>,
        token: Token,
        delta: u32,
        arena: bool,
    ) -> Option<UnigramAdd> {
        let original = original?;
        let nibble = (token >> 24) as u8;
        let slot = token & 0x00ff_ffff;
        let library = library_of(&mut self.replayed, &mut self.totals, nibble, original);
        if arena {
            return Some(Self::take_total(library, delta));
        }
        if let Some(item) = self.decoded.get_mut(&token) {
            let outcome = Self::take_total(library, delta);
            if outcome == UnigramAdd::Accepted {
                item.unigram = item.unigram.wrapping_add(delta);
            }
            return Some(outcome);
        }
        Some(library.add_unigram(slot, delta))
    }

    fn take_total(library: &mut ReplayedLibrary, delta: u32) -> UnigramAdd {
        match library.total().checked_add(delta) {
            Some(total) => {
                library.set_total(total);
                UnigramAdd::Accepted
            }
            None => UnigramAdd::Refused,
        }
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
                let mut library = ReplayedLibrary::pristine(original);
                if let Some(total) = self.totals.remove(&nibble) {
                    library.set_total(total);
                }
                vacant.insert(library)
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

    /// Touched slots made unreadable by an in-place write must not fall back
    /// to the shipped item when the save builds its journal.
    pub(crate) fn unreadable_slots(&self) -> impl Iterator<Item = Token> + '_ {
        self.replayed.iter().flat_map(move |(&library, replayed)| {
            replayed.slots().filter_map(move |slot| {
                let token = (u32::from(library) << 24) | slot;
                (replayed.item_past_buffer(slot) && !self.decoded.contains_key(&token))
                    .then_some(token)
            })
        })
    }

    fn materialize(&mut self) {
        if self.replayed.is_empty() {
            return;
        }
        self.decoded = self.snapshot();
        for (&library, buffer) in &self.replayed {
            self.totals.insert(library, buffer.total());
        }
        self.replayed.clear();
    }

    pub(crate) fn retain(&mut self, keep: impl FnMut(&Token, &mut ChunkItem) -> bool) {
        self.materialize();
        self.decoded.retain(keep);
    }

    /// Drops decoded items, leaving every live buffer as it is.
    pub(crate) fn retain_decoded(&mut self, keep: impl FnMut(&Token, &mut ChunkItem) -> bool) {
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
