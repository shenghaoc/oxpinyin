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

use oxpinyin_core::PhraseText;
use oxpinyin_data::phrase_libraries::PhraseLibraries;
use oxpinyin_data::phrase_library::PhraseItemView;

use crate::store::{Token, UserStore, UserStoreError};

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
    /// The keys of a reading as the chunk stores them.
    #[must_use]
    pub fn packed(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    /// The number of keys.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len() / 2
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

    /// Validated item bytes, borrowed only for this callback.
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

    /// The phrase text, `None` where a character is not a scalar value.
    #[must_use]
    pub fn phrase_text(&self) -> Option<PhraseText> {
        self.view().phrase_text()
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

    /// Every stored reading in stored order.
    pub fn pronunciations(&self) -> impl Iterator<Item = Reading<'_>> + '_ {
        (0..self.n_pronunciations()).map_while(|nth| self.pronunciation(nth))
    }
}

/// What a read of one token found.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Lookup<R> {
    /// The token's library is unloaded.
    Unloaded,
    /// A journal REMOVE took the item out of its sub-index; the shipped chunk
    /// and the phrase table still hold it.
    Removed,
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
            Self::Unloaded | Self::Removed | Self::Absent => None,
        }
    }
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
    pub const fn unmasked(libraries: &'a PhraseLibraries, user: &'a UserStore) -> Self {
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
    pub fn is_system_library(&self, library: u8) -> bool {
        self.user.is_system_library(library)
    }

    /// Whether the session holds items beyond the shipped chunks.
    /// Consumer: the pricing overlay gate, `RuntimeDict::effective_query`.
    #[must_use]
    pub fn has_overlay(&self) -> bool {
        self.user.has_system_items()
    }

    /// Whether any entry-level filter can hide a token: a library is
    /// unloaded or the session holds a removed item. Consumers:
    /// `RuntimeDict::token_filter_active`.
    #[must_use]
    pub fn filter_active(&self) -> bool {
        self.unloaded_mask() != 0 || self.user.has_removed_system_items()
    }

    /// Whether the token's library is loaded and no journal REMOVE took its
    /// item out. Consumers: `RuntimeDict::library_visible_token`, the export
    /// probe's removed check.
    #[must_use]
    pub fn is_visible(&self, token: Token) -> bool {
        self.library_loaded(token >> 24) && !self.user.system_item_removed(token)
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
    /// (`effective_possibility`), `token_text`, `token_nth_pronunciation`,
    /// the export probe and row reads.
    pub fn read<R>(&self, token: Token, read: impl FnOnce(SystemItem<'_>) -> R) -> Lookup<R> {
        if !u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index)) {
            return Lookup::Absent;
        }
        if !self.library_loaded(token >> 24) {
            return Lookup::Unloaded;
        }
        let mut read = Some(read);
        let store = self.user;
        if store.system_item_removed(token) {
            return Lookup::Removed;
        }
        // The library's live buffer holds the session's item, or the
        // shipped one it was made from.
        if let Some(found) = store
            .with_live_item(token, |bytes| {
                let view = PhraseItemView::from_bytes(bytes)?;
                let f = read.take()?;
                Some(f(SystemItem::Replayed(view)))
            })
            .flatten()
        {
            return Lookup::Found(found);
        }
        if store.live_item_past_buffer(token) {
            return Lookup::Absent;
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

    /// The stored reading count of `token`'s effective item. A shipped item
    /// is read the way the pin reads it (its offset and two header bytes), so
    /// this answers where [`Self::read`] cannot build a view over a short
    /// payload. Consumers: `token_pronunciation_count`, `token_exists`.
    #[must_use]
    pub fn pronunciation_count(&self, token: Token) -> Lookup<u32> {
        if !u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index)) {
            return Lookup::Absent;
        }
        if !self.library_loaded(token >> 24) {
            return Lookup::Unloaded;
        }
        let store = self.user;
        if store.system_item_removed(token) {
            return Lookup::Removed;
        }
        if let Some(found) = store.live_pronunciation_count(token) {
            return Lookup::Found(found);
        }
        if store.live_item_past_buffer(token) {
            return Lookup::Absent;
        }
        self.libraries
            .library(token)
            .and_then(|library| library.pronunciation_count(token))
            .map_or(Lookup::Absent, Lookup::Found)
    }

    /// The live unigram of `token`'s effective system item. Returns `None`
    /// when the token is hidden or holds no item.
    #[must_use]
    pub fn effective_unigram(&self, token: Token) -> Option<u64> {
        if !u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index))
            || !self.is_visible(token)
        {
            return None;
        }
        self.read(token, |item| u64::from(item.unigram())).found()
    }

    /// Whether the session holds an item for `token`, in its arena or its
    /// library's live buffer, whatever the mask or a removal says: an item the
    /// shipped chunk lacks. The store's unigram delta of a held item is
    /// read as well, only for its error, because the check this replaces built
    /// the item with its live count and failed when that read did.
    /// Consumer: the n-best step's imported-item flag, `RuntimeLm`.
    ///
    /// # Errors
    /// Returns the store's error when the unigram delta of a held item cannot
    /// be read.
    pub fn is_imported(&self, token: Token) -> Result<bool, UserStoreError> {
        let store = self.user;
        if !u8::try_from(token >> 24).is_ok_and(|index| self.is_system_library(index)) {
            return Ok(false);
        }
        let held = store.with_user_item(token, |_| ()).is_some();
        if held {
            store.unigram_delta(token)?;
        }
        Ok(held)
    }

    /// Exclusive token end of the session items one library holds. Consumer:
    /// the export range probe, `appended_end`.
    #[must_use]
    pub fn overlay_end(&self, library: u8) -> Option<Token> {
        self.user.system_item_end(library)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oxpinyin_data::chunk_write::{ChunkItem, build_chunk};

    /// Policy row 107: phrase_index.cpp:180-206 at 074a2219 can hand out
    /// an item extending past its buffer. The approved safe outcome is no
    /// item, never resurrection of the valid original in a separate mapping.
    #[test]
    fn an_overlong_live_item_never_falls_back_to_a_shipped_copy() {
        use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
        use oxpinyin_data::{
            phrase_library::PhraseLibrary,
            user_files::{LogRecord, SystemVersions, UserTableInfo, encode_log_records},
        };
        use std::collections::BTreeMap;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("system.bin");
        let item = ChunkItem {
            phrase: vec![0x4f60],
            unigram: 1,
            prons: Vec::new(),
        };
        std::fs::write(&path, build_chunk(&[(1, item.clone())]).unwrap()).unwrap();
        let mut shipped = PhraseLibraries::empty();
        shipped.load(1, &path).unwrap();
        let original = PhraseLibrary::open(&path).unwrap();
        let originals = SystemOriginals::from(crate::LibraryTable::from([(
            1,
            SystemLibrary {
                total: original.total_freq(),
                range_end: original.token_range().end,
                mapped: Some(original),
                items: BTreeMap::new(),
            },
        )]));
        let old = oxpinyin_data::chunk_write::encode_phrase_item(&item).unwrap();
        let mut over = old.clone();
        over[0] = 255;
        over[1] = 255;
        let payload = encode_log_records(&[LogRecord::Modify {
            token: 1,
            old_item: old,
            new_item: over,
        }])
        .unwrap();
        let versions = SystemVersions::for_this_build(7, 14);
        let user = dir.path().join("user");
        std::fs::create_dir(&user).unwrap();
        std::fs::write(
            user.join("user.conf"),
            UserTableInfo::conform_to(&versions).to_text(),
        )
        .unwrap();
        std::fs::write(
            user.join("gb_char.dbin"),
            oxpinyin_data::chunk_format::build_memory_chunk(&payload).unwrap(),
        )
        .unwrap();
        let store =
            UserStore::open_libpinyin(&user, originals, versions, UserConfLaw::Pinyin).unwrap();
        assert!(store.live_item_past_buffer(0x0100_0001));
        store.test_forget_removal(0x0100_0001);
        let unloaded = Cell::new(0);
        let live = LiveSystemItems::new(&shipped, &store, &unloaded);
        assert_eq!(
            live.read(0x0100_0001, |item| item.unigram()),
            Lookup::Absent
        );
        assert_eq!(shipped.unigram_count(0x0100_0001), Some(1));
    }

    /// 074a2219 pinyin.cpp:337-367 loads the libraries named by table.conf;
    /// the configured library index is not restricted to the stock 1..=4.
    #[test]
    fn a_nonstock_loaded_library_is_a_system_library() {
        use crate::persistence::{SystemLibrary, SystemOriginals, UserConfLaw};
        use oxpinyin_data::{
            phrase_library::PhraseLibrary,
            user_files::{SystemVersions, UserTableInfo},
        };
        use std::collections::BTreeMap;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("six.bin");
        let item = ChunkItem {
            phrase: vec![0x4f60],
            unigram: 1,
            prons: Vec::new(),
        };
        std::fs::write(&path, build_chunk(&[(1, item)]).unwrap()).unwrap();
        let mut libraries = PhraseLibraries::empty();
        assert!(libraries.load(6, &path).unwrap());
        let original = PhraseLibrary::open(&path).unwrap();
        let originals = SystemOriginals::from(crate::LibraryTable::from([(
            6,
            SystemLibrary {
                total: original.total_freq(),
                range_end: original.token_range().end,
                mapped: Some(original),
                items: BTreeMap::new(),
            },
        )]));
        let versions = SystemVersions::for_this_build(7, 14);
        let user = dir.path().join("user");
        std::fs::create_dir(&user).unwrap();
        std::fs::write(
            user.join("user.conf"),
            UserTableInfo::conform_to(&versions).to_text(),
        )
        .unwrap();
        let store =
            UserStore::open_libpinyin(&user, originals, versions, UserConfLaw::Pinyin).unwrap();
        let unloaded = Cell::new(0);
        let live = LiveSystemItems::new(&libraries, &store, &unloaded);
        assert!(live.is_system_library(6));
        assert!(!live.is_system_library(5));
        assert_eq!(
            live.read(0x0600_0001, |item| item.unigram()),
            Lookup::Found(1)
        );
    }
}
