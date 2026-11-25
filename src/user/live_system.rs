//! Borrowed access to live items of configured system libraries.
//!
//! A session's effective system item is the shipped chunk's item, or the
//! item a journal replay, an import or training left in the session. The
//! readers of lookup, pricing, introspection and export ask this one type
//! instead of combining the shipped libraries, the unloaded-library mask and
//! the store's overlay themselves.
//!
//! Every read borrows the session for one call. An item is handed to a
//! callback as a [`SystemItem`], a view that borrows its owner, so it cannot
//! outlive the call, and reading it allocates nothing. The type is concrete:
//! one implementation exists and the consumers differ in what they read, not
//! in who answers.

use std::cell::Cell;

use crate::data::PhraseLibraries;
use crate::data::phrase_library::CHEWING_KEY_SIZE;
use crate::data::phrase_library::PhraseItemView;
use crate::vocab::PhraseText;

use crate::user::store::UserStore;
use crate::user::system_items::LiveItemFields;
use crate::vocab::PhraseToken;

/// Where a [`SystemItem`] lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemOrigin {
    /// The shipped chunk's item: the library has no live buffer.
    Shipped,
    /// An item read from the library's live buffer, which a replay, an import,
    /// a training or an increment made from the chunk.
    Overlay,
}

/// One effective item, borrowed from the library or the session.
#[derive(Clone, Copy)]
pub enum SystemItem<'a> {
    /// The item as the shipped chunk stores it.
    Shipped(PhraseItemView<'a>),
    /// An item read from the library's live buffer.
    Replayed(PhraseItemView<'a>),
}

/// The keys of one reading, in the chunk's packed `u16` form: little-endian
/// `u16` pairs, as the mapped chunk holds them.
#[derive(Clone, Copy)]
pub struct ReadingKeys<'a>(&'a [u8]);

impl<'a> ReadingKeys<'a> {
    /// Borrows a reading as a sequence of two-byte packed keys. Supply complete two-byte
    /// words.
    #[must_use]
    pub fn packed(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    /// The number of keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len() / CHEWING_KEY_SIZE
    }

    /// Whether the reading has no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The packed keys in stored order.
    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        self.0
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
    }
}

/// One stored reading of an item.
#[derive(Clone, Copy)]
pub struct Reading<'a> {
    /// The reading's keys.
    pub keys: ReadingKeys<'a>,
    /// The reading's stored frequency.
    pub frequency: u32,
}

impl<'a> SystemItem<'a> {
    fn view(&self) -> PhraseItemView<'a> {
        match self {
            Self::Shipped(item) | Self::Replayed(item) => *item,
        }
    }

    /// Whether the item is the shipped chunk's own or a live buffer's.
    #[must_use]
    pub fn origin(&self) -> ItemOrigin {
        match self {
            Self::Shipped(_) => ItemOrigin::Shipped,
            Self::Replayed(_) => ItemOrigin::Overlay,
        }
    }

    /// The item's bytes, borrowed only for this callback. Under
    /// `LiveSystemItems::read_fields` a shipped item's bytes can end at the
    /// content end, before the item's header-derived length.
    #[must_use]
    pub fn as_bytes(&self) -> &'a [u8] {
        self.view().as_bytes()
    }

    /// The live item's stored unigram field.
    #[must_use]
    pub fn unigram(&self) -> u32 {
        self.view().unigram()
    }

    /// The phrase length in characters.
    #[must_use]
    pub fn phrase_length(&self) -> usize {
        self.view().phrase_length()
    }

    /// The number of stored readings.
    #[must_use]
    pub fn n_pronunciations(&self) -> usize {
        self.view().n_pronunciations()
    }

    /// Reads the item's outward phrase text, retaining accepted non-scalar UCS-4 words. See
    /// [`crate::data::phrase_library::PhraseItemView::phrase_text`] for the word
    /// requirements.
    #[must_use]
    pub fn phrase_text(&self) -> Option<PhraseText> {
        self.view().phrase_text()
    }

    /// Whether every declared word of the text lies inside the item's bytes.
    #[must_use]
    pub(crate) fn phrase_words_present(&self) -> bool {
        self.view().phrase_words_present()
    }

    /// Reading `nth` in stored order.
    #[must_use]
    pub fn pronunciation(&self, nth: usize) -> Option<Reading<'_>> {
        let (Self::Shipped(item) | Self::Replayed(item)) = self;
        let reading = item.pronunciation(nth)?;
        Some(Reading {
            keys: ReadingKeys::packed(reading.keys),
            frequency: reading.freq,
        })
    }

    /// Iterates readings in stored order until the first unreadable reading.
    /// [`crate::data::phrase_library::PhraseItemView::pronunciations`] visits each declared
    /// position.
    pub fn pronunciations(&self) -> impl Iterator<Item = Reading<'_>> + '_ {
        (0..self.n_pronunciations()).map_while(|nth| self.pronunciation(nth))
    }
}

/// What a read of one token found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lookup<R> {
    /// The token's library is unloaded.
    Unloaded,
    /// The library is loaded and holds no item under the token.
    Absent,
    /// The effective item and what the reader made of it.
    Found(R),
}

impl<R> Lookup<R> {
    /// The reader's result.
    #[must_use]
    pub fn found(self) -> Option<R> {
        match self {
            Self::Found(value) => Some(value),
            Self::Unloaded | Self::Absent => None,
        }
    }
}

/// What a trellis step's in-place read knows of each library, read once
/// per key path ([`LiveSystemItems::step_libraries`]) and fixed while a
/// guess runs. Bit `n` is library `n`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct StepLibraries {
    /// The system libraries whose chunk is detached into its live buffer
    /// while the session holds system items ([`UserStore::step_libraries`]),
    /// less the ones the mask unloaded ([`LiveSystemItems::step_libraries`]).
    pub(crate) in_place: u16,
    /// The libraries the session arena holds: a read asks the arena for the
    /// token first.
    pub(crate) arena: u16,
}

impl StepLibraries {
    /// `token`'s library bit. A library index is below 16
    /// (`PHRASE_INDEX_LIBRARY_INDEX`, 074a2219 `novel_types.h:44`).
    fn bit(token: PhraseToken) -> u16 {
        1_u16
            .checked_shl(u32::from(crate::vocab::phrase_index_library_index(token)))
            .unwrap_or(0)
    }
}

/// Whether an in-place read asks the session arena for the token before
/// the live buffer ([`UserStore::read_live_item_in_place`]).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ArenaProbe {
    /// The arena holds a library at the token's index: an item there comes
    /// first.
    First,
    /// The arena holds no library at the token's index, so no item there
    /// can come first.
    Skip,
}

/// One operation's access to the effective system items.
///
/// Built from the shipped libraries, the session store and the unloaded-
/// library mask for the length of one operation. Nothing here owns a mapping
/// or can be cloned out of the borrow.
pub struct LiveSystemItems<'a> {
    libraries: &'a PhraseLibraries,
    user: &'a UserStore,
    /// `None` reads as no library unloaded.
    unloaded: Option<&'a Cell<u32>>,
}

impl<'a> LiveSystemItems<'a> {
    /// Borrows the shipped `libraries`, the session `user` store and the
    /// `unloaded` mask (bit `n` set: library `n` is unloaded).
    #[must_use]
    pub const fn new(
        libraries: &'a PhraseLibraries,
        user: &'a UserStore,
        unloaded: &'a Cell<u32>,
    ) -> Self {
        Self {
            libraries,
            user,
            unloaded: Some(unloaded),
        }
    }

    /// Borrows the shipped `libraries` and the session `user` store with no
    /// library unloaded. Consumer: the phrase export iterator, which checks a
    /// library's visibility once when it probes and then reads the items it
    /// probed whatever the mask says.
    #[must_use]
    pub(crate) const fn unmasked(libraries: &'a PhraseLibraries, user: &'a UserStore) -> Self {
        Self {
            libraries,
            user,
            unloaded: None,
        }
    }

    fn unloaded_mask(&self) -> u32 {
        self.unloaded.map_or(0, Cell::get)
    }

    fn library_loaded(&self, nibble: u32) -> bool {
        let mask = self.unloaded_mask();
        mask == 0 || nibble >= 32 || mask & (1 << nibble) == 0
    }

    /// Whether the facade's table layout names this library as a system chunk.
    #[must_use]
    pub(crate) fn is_system_library(&self, library: u8) -> bool {
        self.user.is_system_library(library)
    }

    /// Whether the session holds items beyond the shipped chunks.
    /// Consumer: the pricing overlay gate, `RuntimeDict::effective_query`.
    #[must_use]
    pub(crate) fn has_overlay(&self) -> bool {
        self.user.has_system_items()
    }

    /// Whether any entry-level filter can hide a token: a library is
    /// unloaded or the session holds a removed item. Consumers:
    /// `RuntimeDict::token_filter_active`.
    #[must_use]
    pub(crate) fn filter_active(&self) -> bool {
        self.unloaded_mask() != 0 || self.user.has_removed_system_items()
    }

    /// Whether the token's library is loaded and no journal REMOVE took its
    /// item out. Consumers: `RuntimeDict::library_visible_token`, the export
    /// probe's removed check.
    #[must_use]
    pub(crate) fn is_visible(&self, token: PhraseToken) -> bool {
        self.library_loaded(u32::from(crate::vocab::phrase_index_library_index(token)))
            && !self.user.system_item_removed(token)
    }

    /// Reads the effective item behind `token` in a configured system
    /// library; other libraries answer [`Lookup::Absent`].
    ///
    /// The session's item wins over the shipped one: the library's live
    /// buffer holds it. `read` runs at most once, inside the borrow. For a
    /// live-buffer item, the callback runs while a store cell is borrowed. Do
    /// not call store accessors from the callback; copy out what is needed
    /// and query the store after it returns. A callback borrowing the shipped
    /// chunk directly runs after those borrows have ended. Consumers: pricing
    /// (`effective_possibility`), `token_text`, the export probe and row
    /// reads.
    pub fn read<R>(&self, token: PhraseToken, read: impl FnOnce(SystemItem<'_>) -> R) -> Lookup<R> {
        if let Some(gate) = self.gate(token) {
            return gate;
        }
        let mut read = Some(read);
        let store = self.user;
        // The library's live buffer holds the session's item, or the
        // shipped one it was made from. One slot read answers both whether
        // a journal REMOVE took the item out and the item, as
        // `get_phrase_item` reads one offset (074a2219
        // `phrase_index.cpp:180-206`). The item is read only while the
        // session holds system items; otherwise the shipped chunk answers.
        let overlay = store.has_system_items();
        match store.read_live_item(token, |bytes| {
            if !overlay {
                return None;
            }
            let view = PhraseItemView::from_bytes(bytes)?;
            let f = read.take()?;
            Some(f(SystemItem::Replayed(view)))
        }) {
            LiveItemFields::Read(Some(found)) => return Lookup::Found(found),
            LiveItemFields::NoReadableItem => return Lookup::Absent,
            LiveItemFields::Read(None) | LiveItemFields::NoLiveLibrary => {}
        }
        match (
            self.libraries
                .library(token)
                .and_then(|library| library.item(token)),
            read,
        ) {
            (Some(item), Some(f)) => Lookup::Found(f(SystemItem::Shipped(item))),
            _ => Lookup::Absent,
        }
    }

    /// The libraries a trellis step reads in place, read once per key path
    /// ([`StepLibraries`]): those [`UserStore::step_libraries`] names
    /// detached, less the ones the mask unloaded. The mask does not change
    /// while a guess runs: its two writers, `RuntimeDict::unload_library`
    /// and `RuntimeDict::load_library`, are reached only from
    /// `Runtime::unload_library` and `Runtime::load_library`, which take
    /// `&mut self`, and a guess holds the runtime through `&self`.
    #[must_use]
    pub(crate) fn step_libraries(&self) -> StepLibraries {
        let libraries = self.user.step_libraries();
        // Masks of the sixteen library bits; a mask bit past them names no
        // library.
        let unloaded = (self.unloaded_mask() & u32::from(u16::MAX)) as u16;
        StepLibraries {
            in_place: libraries.in_place & !unloaded,
            arena: libraries.arena,
        }
    }

    /// [`Self::read`] for a trellis step: the live-buffer item behind
    /// `token`, read in place, where `libraries` names its library and no
    /// user item comes before it ([`UserStore::read_live_item_in_place`]).
    /// `None` sends the caller to its full resolution, which keeps the
    /// order a user item, the live item, the shipped item. Consumer:
    /// `RuntimeDict::read_in_place`.
    pub(crate) fn read_in_place<R>(
        &self,
        libraries: StepLibraries,
        token: PhraseToken,
        read: impl FnOnce(SystemItem<'_>) -> R,
    ) -> Option<R> {
        let bit = StepLibraries::bit(token);
        if libraries.in_place & bit == 0 {
            return None;
        }
        let arena = if libraries.arena & bit != 0 {
            ArenaProbe::First
        } else {
            ArenaProbe::Skip
        };
        self.user
            .read_live_item_in_place(token, arena, |view| read(SystemItem::Replayed(view)))
    }

    /// [`Self::read`] for the token getters. `get_phrase_item` checks only
    /// the offset and the two header bytes (074a2219 `phrase_index.cpp:180-205`)
    /// and each getter reads its own field, so the item is read field by
    /// field ([`PhraseItemView::fields`]): its unigram, text and readings
    /// inside the record stay readable when a later reading is incomplete. A rejected journal-written item
    /// still answers no item. Consumers: `effective_unigram`,
    /// `token_phrase`, `token_nth_pronunciation`.
    pub(crate) fn read_fields<R>(
        &self,
        token: PhraseToken,
        read: impl FnOnce(SystemItem<'_>) -> R,
    ) -> Lookup<R> {
        if let Some(gate) = self.gate(token) {
            return gate;
        }
        let mut read = Some(read);
        match self.user.with_live_item_fields(token, |bytes| {
            let view = PhraseItemView::fields(bytes)?;
            let f = read.take()?;
            Some(f(SystemItem::Replayed(view)))
        }) {
            LiveItemFields::Read(result) => return result.map_or(Lookup::Absent, Lookup::Found),
            LiveItemFields::NoReadableItem => return Lookup::Absent,
            LiveItemFields::NoLiveLibrary => {}
        }
        match (
            self.libraries
                .library(token)
                .and_then(|library| library.item_fields(token)),
            read,
        ) {
            (Some(item), Some(f)) => Lookup::Found(f(SystemItem::Shipped(item))),
            _ => Lookup::Absent,
        }
    }

    /// The answer for a token outside a loaded system library, or `None`.
    fn gate<R>(&self, token: PhraseToken) -> Option<Lookup<R>> {
        let library = crate::vocab::phrase_index_library_index(token);
        if !self.is_system_library(library) {
            return Some(Lookup::Absent);
        }
        if !self.library_loaded(u32::from(library)) {
            return Some(Lookup::Unloaded);
        }
        None
    }

    /// The stored reading count of `token`'s effective item. A shipped item
    /// is read the way the pin reads it (its offset and two header bytes), so
    /// this answers where [`Self::read`] cannot build a view over a short
    /// payload. Consumers: `token_pronunciation_count`, `token_exists`.
    #[must_use]
    pub(crate) fn pronunciation_count(&self, token: PhraseToken) -> Lookup<u32> {
        if !self.is_system_library(crate::vocab::phrase_index_library_index(token)) {
            return Lookup::Absent;
        }
        if !self.library_loaded(u32::from(crate::vocab::phrase_index_library_index(token))) {
            return Lookup::Unloaded;
        }
        // One read of the live buffer's item fields: a slot it holds no
        // readable count at is a removed, unreadable or absent item.
        match self.user.live_pronunciation_count_read(token) {
            LiveItemFields::Read(Some(found)) => return Lookup::Found(found),
            LiveItemFields::Read(None) | LiveItemFields::NoReadableItem => return Lookup::Absent,
            LiveItemFields::NoLiveLibrary => {}
        }
        self.libraries
            .library(token)
            .and_then(|library| library.pronunciation_count(token))
            .map_or(Lookup::Absent, Lookup::Found)
    }

    /// The live unigram of `token`'s effective system item. Returns `None`
    /// when the token is hidden or holds no item.
    #[must_use]
    pub fn effective_unigram(&self, token: PhraseToken) -> Option<u64> {
        self.read_fields(token, |item| u64::from(item.unigram()))
            .found()
    }

    /// The current sub-index's range end (phrase_index.cpp:610–646).
    #[must_use]
    pub(crate) fn system_range_end(&self, library: u8) -> Option<PhraseToken> {
        self.user.system_range_end(library)
    }
}

#[cfg(test)]
mod tests;
