//! Borrowed access to the live system items of libraries `1..=4`.
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

use std::sync::atomic::{AtomicU32, Ordering};

use oxpinyin_core::PhraseText;
use oxpinyin_data::chunk_write::ChunkItem;
use oxpinyin_data::phrase_libraries::PhraseLibraries;
use oxpinyin_data::phrase_library::PhraseItemView;

use crate::store::{Token, UserStore, UserStoreError};

/// Where a [`SystemItem`] lives.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ItemOrigin {
    /// The shipped chunk's item, untouched by the session.
    Shipped,
    /// An item the session holds in place of, or beside, the chunk's: an
    /// import, a replayed journal record, or a trained item.
    Overlay,
}

/// One effective item, borrowed from the library or the session.
#[derive(Clone, Copy)]
pub enum SystemItem<'a> {
    /// The item as the shipped chunk stores it.
    Shipped(PhraseItemView<'a>),
    /// An imported item in the session's phrase arena, in the chunk's layout.
    Arena(PhraseItemView<'a>),
    /// A session item held decoded. Its `unigram` field is stale; read the
    /// live count through [`LiveSystemItems::effective_unigram`].
    Overlay(&'a ChunkItem),
}

/// The keys of one reading, in the chunk's packed `u16` form.
#[derive(Clone, Copy)]
pub enum ReadingKeys<'a> {
    /// Little-endian `u16` pairs, as the mapped chunk holds them.
    Packed(&'a [u8]),
    /// The words of a decoded item.
    Words(&'a [u16]),
}

impl ReadingKeys<'_> {
    /// The number of keys.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Packed(bytes) => bytes.len() / 2,
            Self::Words(words) => words.len(),
        }
    }

    /// Whether the reading has no keys.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The packed keys in stored order.
    pub fn iter(&self) -> impl Iterator<Item = u16> + '_ {
        let (packed, words) = match self {
            Self::Packed(bytes) => (*bytes, &[][..]),
            Self::Words(words) => (&[][..], *words),
        };
        packed
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .chain(words.iter().copied())
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

impl SystemItem<'_> {
    /// Whether the item is the shipped chunk's own or a session item.
    #[must_use]
    pub fn origin(&self) -> ItemOrigin {
        match self {
            Self::Shipped(_) => ItemOrigin::Shipped,
            Self::Arena(_) | Self::Overlay(_) => ItemOrigin::Overlay,
        }
    }

    /// The phrase length in characters.
    #[must_use]
    pub fn phrase_length(&self) -> usize {
        match self {
            Self::Shipped(item) | Self::Arena(item) => item.phrase_length(),
            Self::Overlay(item) => item.phrase.len(),
        }
    }

    /// The number of stored readings.
    #[must_use]
    pub fn n_pronunciations(&self) -> usize {
        match self {
            Self::Shipped(item) | Self::Arena(item) => item.n_pronunciations(),
            Self::Overlay(item) => item.prons.len(),
        }
    }

    /// The phrase text, `None` where a character is not a scalar value.
    #[must_use]
    pub fn phrase_text(&self) -> Option<PhraseText> {
        match self {
            Self::Shipped(item) | Self::Arena(item) => item.phrase_text(),
            Self::Overlay(item) => PhraseText::from_words(item.phrase.iter().copied()),
        }
    }

    /// Reading `nth` in stored order.
    #[must_use]
    pub fn pronunciation(&self, nth: usize) -> Option<Reading<'_>> {
        match self {
            Self::Shipped(item) | Self::Arena(item) => {
                let reading = item.pronunciation(nth)?;
                Some(Reading {
                    keys: ReadingKeys::Packed(reading.keys),
                    frequency: reading.freq,
                })
            }
            Self::Overlay(item) => {
                let (keys, frequency) = item.prons.get(nth)?;
                Some(Reading {
                    keys: ReadingKeys::Words(keys),
                    frequency: *frequency,
                })
            }
        }
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
    user: Option<&'a UserStore>,
    unloaded: &'a AtomicU32,
}

impl<'a> LiveSystemItems<'a> {
    /// Borrows the shipped `libraries`, the session `user` store and the
    /// `unloaded` mask (bit `n` set: library `n` is unloaded).
    #[must_use]
    pub const fn new(
        libraries: &'a PhraseLibraries,
        user: Option<&'a UserStore>,
        unloaded: &'a AtomicU32,
    ) -> Self {
        Self {
            libraries,
            user,
            unloaded,
        }
    }

    /// Borrows the shipped `libraries` and the session `user` store with no
    /// library unloaded. Consumer: the phrase export iterator, which checks a
    /// library's visibility once when it probes and then reads the items it
    /// probed whatever the mask says.
    #[must_use]
    pub fn unmasked(libraries: &'a PhraseLibraries, user: Option<&'a UserStore>) -> Self {
        static ALL_LOADED: AtomicU32 = AtomicU32::new(0);
        Self::new(libraries, user, &ALL_LOADED)
    }

    fn library_loaded(&self, nibble: u32) -> bool {
        let mask = self.unloaded.load(Ordering::SeqCst);
        mask == 0 || nibble >= 32 || mask & (1 << nibble) == 0
    }

    /// Whether the session holds items beyond the shipped chunks.
    /// Consumer: the pricing overlay gate, `RuntimeDict::effective_query`.
    #[must_use]
    pub fn has_overlay(&self) -> bool {
        self.user.is_some_and(UserStore::has_system_items)
    }

    /// Whether any entry-level filter can hide a token: a library is
    /// unloaded or the session holds a removed item. Consumers:
    /// `RuntimeDict::token_filter_active`.
    #[must_use]
    pub fn filter_active(&self) -> bool {
        self.unloaded.load(Ordering::SeqCst) != 0
            || self.user.is_some_and(UserStore::has_removed_system_items)
    }

    /// Whether the token's library is loaded and no journal REMOVE took its
    /// item out. Consumers: `RuntimeDict::library_visible_token`, the export
    /// probe's removed check.
    #[must_use]
    pub fn is_visible(&self, token: Token) -> bool {
        self.library_loaded(token >> 24)
            && !self
                .user
                .is_some_and(|store| store.system_item_removed(token))
    }

    /// Reads the effective item behind `token` of libraries `1..=4`; any
    /// other library answers [`Lookup::Absent`].
    ///
    /// The session's item wins over the shipped one. `read` runs at most
    /// once, inside the borrow. For an arena or overlay item, the callback
    /// runs while a store mutex is held. Do not call store accessors from the
    /// callback; copy out what is needed and query the store after it returns.
    /// A callback borrowing the shipped chunk directly runs after those
    /// locks are released.
    /// Consumers: pricing
    /// (`effective_possibility`), `token_text`, `token_nth_pronunciation`,
    /// the export probe and row reads.
    pub fn read<R>(&self, token: Token, read: impl FnOnce(SystemItem<'_>) -> R) -> Lookup<R> {
        if !(1..=4).contains(&(token >> 24)) {
            return Lookup::Absent;
        }
        if !self.library_loaded(token >> 24) {
            return Lookup::Unloaded;
        }
        let mut read = Some(read);
        if let Some(store) = self.user {
            if store.system_item_removed(token) {
                return Lookup::Removed;
            }
            if let Some(found) = store
                .with_user_item(token, |view| {
                    read.take().map(|f| f(SystemItem::Arena(view)))
                })
                .flatten()
            {
                return Lookup::Found(found);
            }
            if let Some(found) = store
                .with_overlay_item(token, |item| {
                    read.take().map(|f| f(SystemItem::Overlay(item)))
                })
                .flatten()
            {
                return Lookup::Found(found);
            }
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
        if !(1..=4).contains(&(token >> 24)) {
            return Lookup::Absent;
        }
        if !self.library_loaded(token >> 24) {
            return Lookup::Unloaded;
        }
        if let Some(store) = self.user {
            if store.system_item_removed(token) {
                return Lookup::Removed;
            }
            let count = |len: usize| u32::try_from(len).unwrap_or(u32::MAX);
            if let Some(found) = store.with_user_item(token, |view| count(view.n_pronunciations()))
            {
                return Lookup::Found(found);
            }
            if let Some(found) = store.with_overlay_item(token, |item| count(item.prons.len())) {
                return Lookup::Found(found);
            }
        }
        self.libraries
            .library(token)
            .and_then(|library| library.pronunciation_count(token))
            .map_or(Lookup::Absent, Lookup::Found)
    }

    /// The stored unigram field of the shipped item, `gen_unigram`'s `+1`
    /// included: the chunk's own value, with no session change applied and
    /// no visibility check. Consumers: `predict.rs`'s `item_count` for an item
    /// the chunk holds (its caller has gated visibility), the test that
    /// restates that formula, and [`Self::effective_unigram`].
    #[must_use]
    pub fn stored_unigram(&self, token: Token) -> Option<u64> {
        self.libraries.unigram_count(token)
    }

    /// The live unigram of `token`'s effective item: the shipped stored value
    /// plus the store's unigram delta, or, for an item the chunk lacks, the
    /// store's delta alone. A delta the store cannot read counts as zero, as
    /// the predicted-frequency path always has. Runtime bumps are not
    /// included; a caller adds `RuntimeDict::unigram_delta` once. `None` when
    /// the token is hidden or holds no item. Consumer: the predicted-candidate
    /// frequency, `predict.rs`'s `item_count`.
    #[must_use]
    pub fn effective_unigram(&self, token: Token) -> Option<u64> {
        if !(1..=4).contains(&(token >> 24)) || !self.is_visible(token) {
            return None;
        }
        let delta = || {
            self.user
                .and_then(|store| store.unigram_delta(token).ok())
                .unwrap_or(0)
        };
        if let Some(stored) = self.stored_unigram(token) {
            return Some(stored.wrapping_add(delta()));
        }
        self.read(token, |_| ()).found().map(|()| delta())
    }

    /// Whether the session holds an item for `token`, in its arena or among
    /// its decoded items, whatever the mask or a removal says: an item the
    /// shipped chunk may lack. The store's unigram delta of a held item is
    /// read as well, only for its error, because the check this replaces built
    /// the item with its live count and failed when that read did.
    /// Consumer: the n-best step's imported-item flag, `RuntimeLm`.
    ///
    /// # Errors
    /// Returns the store's error when the unigram delta of a held item cannot
    /// be read.
    pub fn is_imported(&self, token: Token) -> Result<bool, UserStoreError> {
        let Some(store) = self.user else {
            return Ok(false);
        };
        if !(1..=4).contains(&(token >> 24)) {
            return Ok(false);
        }
        let held = store.with_user_item(token, |_| ()).is_some()
            || store.with_overlay_item(token, |_| ()).is_some();
        if held {
            store.unigram_delta(token)?;
        }
        Ok(held)
    }

    /// Exclusive token end of the session items one library holds. Consumer:
    /// the export range probe, `appended_end`.
    #[must_use]
    pub fn overlay_end(&self, library: u8) -> Option<Token> {
        self.user?.system_item_end(library)
    }
}
