//! The mmap-backed phrase libraries as one token-dispatched facade —
//! the runtime seam `FacadePhraseIndex` provides over its
//! `SubPhraseIndex`es (`phrase_index.cpp`).
//!
//! token → phrase (text, pronunciations, unigram) resolves here, through
//! the P1 [`PhraseLibrary`] readers, instead of through a resident
//! token→text map. Upstream's split between the mmap'd chunk files (the
//! per-library phrase index) and the DBM phrase table (phrase → tokens)
//! is kept exactly: this facade covers only the first half.
//!
//! One facade serves the four system libraries (nibbles 1–4) and a second
//! one the addon libraries (`m_addon_phrase_index`, indexes 4–15, loaded
//! on demand by `pinyin_load_addon_phrase_library`).

use crate::data::phrase_library::CHEWING_KEY_SIZE;
use crate::vocab::phrase_index_library_index;
use std::path::{Path, PathBuf};

use crate::{
    model::{TokenPhrase, TrellisItem},
    parser::ChewingKey,
    vocab::PhraseText,
};

use crate::data::phrase_library::{LibraryError, PhraseItemView, PhraseLibrary};

#[cfg(test)]
pub use crate::data::SYSTEM_LIBRARY_FILES as SYSTEM_LIBRARY_STEMS;

/// One pronunciation of a library item.
///
/// The joined pinyin spelling and the pronunciation frequency — the
/// rendering surface the export and token-introspection paths need
/// (`FacadePhraseIndex::get_phrase_item` over
/// `PhraseItem::get_nth_pronunciation`).
pub struct LibraryPronunciation {
    /// `'`-joined pinyin spelling, tone digits dropped (the consumers
    /// resolve it back to tone-less syllable keys).
    pub spelling: String,
    /// The pronunciation's stored count.
    pub freq: u64,
}

/// A loaded library and the file it came from.
#[derive(Debug)]
struct Loaded {
    library: PhraseLibrary,
    /// The file it was mapped from, as `load` was given it.
    path: PathBuf,
}

/// The phrase libraries of one facade, dispatched by token nibble.
///
/// Every reader maps its chunk on open and holds it; nothing here scans
/// anything eagerly beyond the mmap-time checksum pass upstream itself
/// performs. A layout the pin's `SubPhraseIndex::load` refuses inside the
/// chunk is present but empty, as the pin leaves it; a file that is absent
/// is simply unloaded, and token resolution answers `None` for its nibble,
/// matching upstream's `ERROR_NO_SUB_PHRASE_INDEX`.
#[derive(Debug)]
pub struct PhraseLibraries {
    /// nibble → library, `None` = not loaded.
    by_nibble: [Option<Loaded>; 16],
    /// `FacadePhraseIndex::m_total_freq` (074a2219 `phrase_index.h:440`)
    /// over every loaded library: each library's stored `total_freq` is
    /// added when it loads and subtracted when it unloads or is replaced,
    /// with no overflow check (`phrase_index.cpp:237-248`, `:261-269`).
    /// A `guint32`, so it wraps.
    total: u32,
}

impl PhraseLibraries {
    /// A facade with no library loaded.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_nibble: Default::default(),
            total: 0,
        }
    }

    /// Opens the libraries named by `(nibble, file)` pairs from `dir`;
    /// a pair whose file is missing loads as unloaded.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError`] when a present file does not verify as a
    /// well-formed `SubPhraseIndex` chunk, or when a nibble is named
    /// twice or lies outside the sixteen-library space.
    #[cfg(test)]
    pub fn open<S: AsRef<std::ffi::OsStr>>(
        dir: &Path,
        stems: &[(u8, S)],
    ) -> Result<Self, LibraryError> {
        let mut this = Self::empty();
        let mut seen = [false; 16];
        for (nibble, file) in stems {
            let nibble = *nibble;
            let slot = usize::from(nibble);
            if slot >= 16 {
                return Err(LibraryError::Format(format!(
                    "library nibble {nibble} out of range"
                )));
            }
            if seen[slot] {
                return Err(LibraryError::Format(format!(
                    "library nibble {nibble} named twice"
                )));
            }
            seen[slot] = true;
            this.map_library(nibble, dir, file.as_ref(), &mut |_| {})?;
        }
        Ok(this)
    }

    /// Maps one default library from `dir`, the mapping half of the pin's
    /// `_load_phrase_library` (`pinyin.cpp:254-264`, `zhuyin.cpp:198-208`).
    /// Reports a refused map where it occurs. An absent file leaves the
    /// library unloaded. Other non-regular files are skipped silently.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError`] when the present file does not verify as a
    /// well-formed `SubPhraseIndex` chunk, or when it is a directory whose
    /// header read `MemoryChunk::mmap` asserts on (`memory_chunk.h:493`).
    pub(crate) fn map_library(
        &mut self,
        nibble: u8,
        dir: &Path,
        file: &std::ffi::OsStr,
        unmapped: &mut dyn FnMut(&Path),
    ) -> Result<(), LibraryError> {
        // `g_build_filename`: a name never leaves the directory.
        let path = crate::input::build_path(dir, file);
        match std::fs::metadata(&path) {
            // A directory is `MemoryChunk::mmap`'s to judge by its
            // reported size where the boundary says the loader opens
            // directories; any other file that is not a regular one, and a
            // directory on Windows, is skipped without a word.
            Ok(meta) if meta.is_dir() && crate::input::chunk::opens_directories() => {}
            Ok(meta) if !meta.is_file() => return Ok(()),
            Ok(_) => {}
            // The pin's `open` fails and `MemoryChunk::mmap` answers
            // `false`; the library stays unloaded here.
            Err(_) => {
                unmapped(&path);
                return Ok(());
            }
        }
        self.load(nibble, &path).map(|_| ()).inspect_err(|error| {
            if let Some(path) = error.unmappable_path() {
                unmapped(path);
            }
        })
    }

    /// Loads `path` as library `nibble` — `FacadePhraseIndex::load`.
    /// `Ok(false)` when that nibble is already loaded (upstream's
    /// `ERROR_ALREADY_EXISTS`, which the C ABI reports as `false`).
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError`] when the file cannot be mapped or does
    /// not verify, or when `nibble` is outside the sixteen-library space.
    pub fn load(&mut self, nibble: u8, path: &Path) -> Result<bool, LibraryError> {
        let slot = usize::from(nibble);
        if slot >= self.by_nibble.len() {
            return Err(LibraryError::Format(format!(
                "library nibble {nibble} out of range"
            )));
        }
        if self.is_loaded(nibble) {
            return Ok(false);
        }
        let library = PhraseLibrary::open_retaining_layout(path)?;
        // A failed layout leaves the mapped chunk in the empty sub-index,
        // but its total does not enter the facade total (phrase_index.cpp:244-247).
        let accepted = !library.is_layout_failed();
        let slot = &mut self.by_nibble[slot];
        if accepted {
            let total = &mut self.total;
            *total = total.wrapping_add(library.total_freq());
        }
        *slot = Some(Loaded {
            library,
            path: path.to_path_buf(),
        });
        Ok(true)
    }

    /// Drops library `nibble` — `FacadePhraseIndex::unload`. `true` when
    /// a library was loaded there.
    pub fn unload(&mut self, nibble: u8) -> bool {
        let Some(loaded) = self
            .by_nibble
            .get_mut(usize::from(nibble))
            .and_then(Option::take)
        else {
            return false;
        };
        let total = &mut self.total;
        *total = total.wrapping_sub(loaded.library.total_freq());
        true
    }

    /// Installs an already opened library, transferring its byte ownership.
    /// The path is retained for operations that remap the loaded file.
    #[doc(hidden)]
    pub fn insert_mapping(&mut self, nibble: u8, path: PathBuf, library: PhraseLibrary) {
        let Some(slot) = self.by_nibble.get_mut(usize::from(nibble)) else {
            return;
        };
        if let Some(old) = slot.take() {
            self.total = self.total.wrapping_sub(old.library.total_freq());
        }
        if !library.is_layout_failed() {
            self.total = self.total.wrapping_add(library.total_freq());
        }
        *slot = Some(Loaded { library, path });
    }

    /// Transfers one loaded mapping, retaining no reader of its bytes.
    pub(crate) fn take_mapping(&mut self, nibble: u8) -> Option<(PathBuf, PhraseLibrary)> {
        let loaded = self.by_nibble.get_mut(usize::from(nibble))?.take()?;
        self.total = self.total.wrapping_sub(loaded.library.total_freq());
        Some((loaded.path, loaded.library))
    }

    /// Transfer a loaded library's mapping into the session's live sub-index.
    /// The library entry keeps its load-time metadata and path.
    pub(crate) fn take_live_content(
        &mut self,
        nibble: u8,
    ) -> Option<crate::data::phrase_library::PhraseContent> {
        self.by_nibble
            .get_mut(usize::from(nibble))?
            .as_mut()?
            .library
            .take_live_content()
    }

    /// The loaded libraries as a bit set, bit `n` for library `n`
    /// ([`Self::is_loaded`] of each).
    #[must_use]
    pub(crate) fn loaded_set(&self) -> u16 {
        self.by_nibble
            .iter()
            .zip(0_u32..)
            .filter(|(loaded, _)| loaded.is_some())
            .fold(0, |set, (_, nibble)| set | (1 << nibble))
    }

    /// Whether library `nibble` is loaded.
    #[must_use]
    pub fn is_loaded(&self, nibble: u8) -> bool {
        self.by_nibble
            .get(usize::from(nibble))
            .is_some_and(Option::is_some)
    }

    /// Returns whether all library slots are empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_nibble.iter().all(Option::is_none)
    }

    // PHRASE_INDEX_LIBRARY_INDEX, novel_types.h:44 and phrase_index.h:647.
    /// Returns the loaded library selected by bits 24 through 27 of `token`. See
    /// [`crate::vocab::phrase_index_library_index`].
    #[must_use]
    pub fn library(&self, token: u32) -> Option<&PhraseLibrary> {
        self.by_nibble
            .get(usize::from(phrase_index_library_index(token)))?
            .as_ref()
            .map(|loaded| &loaded.library)
    }

    /// A loaded library's on-disk path and its snapshot, for the save's
    /// current-chunk remap (`_write_files` maps every loaded system
    /// library again at `pinyin.cpp:956`, `zhuyin.cpp:589`). The path is
    /// the one `load` was given; the snapshot is the mapping that load
    /// made, which keeps the replaced file's old bytes alive.
    #[must_use]
    pub(crate) fn loaded_path_library(&self, nibble: u8) -> Option<(&Path, &PhraseLibrary)> {
        let slot = self.by_nibble.get(usize::from(nibble))?;
        let loaded = slot.as_ref()?;
        Some((&loaded.path, &loaded.library))
    }

    /// `FacadePhraseIndex::get_phrase_item`'s text half: the phrase
    /// behind `token`, if its library is loaded and owns the token.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<PhraseText> {
        self.library(token)?.item(token)?.phrase_text()
    }

    /// `token`'s stored length and text, as `convert_to_utf8` reads them
    /// (`lookup/lookup.cpp:39-68` at 074a2219): `Some` for a present item
    /// whose text does not convert, with `text` `None`.
    #[must_use]
    pub fn token_phrase(&self, token: u32) -> Option<TokenPhrase> {
        let item = self.library(token)?.item(token)?;
        Some(TokenPhrase {
            phrase_length: item.phrase_length(),
            text: item.phrase_text(),
        })
    }

    /// `PhraseItem::get_unigram_frequency` for `token` — the stored
    /// field, which carries `gen_unigram`'s `+1` on every system item.
    #[must_use]
    pub(crate) fn unigram_count(&self, token: u32) -> Option<u64> {
        Some(u64::from(self.library(token)?.item(token)?.unigram()))
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq` (074a2219
    /// `phrase_index.h:615-617`): the loaded libraries' stored totals as
    /// the facade keeps them, a wrapping `guint32`. Kept as the libraries
    /// load and unload, so a read is one load.
    #[must_use]
    pub fn unigram_total(&self) -> u32 {
        self.total
    }

    /// `FacadePhraseIndex::compact` over the layout-failed empty
    /// sub-indices (`phrase_index.cpp:648-675`, called by `pinyin_save`
    /// at `pinyin.cpp:1139`, `zhuyin_save` at `zhuyin.cpp:554`,
    /// `end_add_phrases` at `pinyin.cpp:657` / `zhuyin.cpp:542`, and
    /// `mask_out` at `pinyin.cpp:1294` / `zhuyin.cpp:829`). Their local
    /// total becomes zero; the facade aggregate already excludes it. A
    /// well-formed library's stored total equals its item sum, so it is
    /// left alone. A library whose bit `unloaded` sets has no sub-index
    /// after `FacadePhraseIndex::unload` (`phrase_index.cpp:261-269`), and
    /// the pin's compact skips it (`:651-652`): its total stays as the
    /// unload took it out. Returns the compacted nibbles.
    #[must_use]
    pub(crate) fn compact_layout_failed(&mut self, unloaded: u32) -> Vec<u8> {
        let mut compacted = Vec::new();
        for (nibble, slot) in self.by_nibble.iter_mut().enumerate() {
            if unloaded & (1 << nibble) != 0 {
                continue;
            }
            if let Some(loaded) = slot.as_mut()
                && loaded.library.is_layout_failed()
            {
                loaded.library.compact();
                compacted.push(nibble as u8);
            }
        }
        compacted
    }

    /// Whether a loaded library outside the `unloaded` bits holds an
    /// item: there is a unigram table to price with, whatever the
    /// wrapping total adds up to.
    #[cfg(test)]
    #[must_use]
    pub fn has_items_unloaded(&self, unloaded: u32) -> bool {
        self.by_nibble.iter().enumerate().any(|(nibble, loaded)| {
            unloaded & (1 << nibble) == 0
                && loaded
                    .as_ref()
                    .is_some_and(|loaded| loaded.library.has_items())
        })
    }

    /// [`Self::unigram_total`] with the libraries whose bit `unloaded`
    /// sets taken out — the facade total after `unload` freed those
    /// sub-indexes (`phrase_index.cpp:261-269`). Only the set bits are
    /// visited, so the common all-loaded mask costs nothing more.
    #[must_use]
    pub fn unigram_total_unloaded(&self, unloaded: u32) -> u32 {
        let mut total = self.unigram_total();
        let mut bits = unloaded & 0xFFFF;
        while bits != 0 {
            let nibble = bits.trailing_zeros() as usize;
            bits &= bits - 1;
            let slot = &self.by_nibble[nibble];
            if let Some(loaded) = slot.as_ref() {
                total = total.wrapping_sub(loaded.library.total_freq());
            }
        }
        total
    }

    /// `PhraseItem::get_pronunciation_possibility(keys)`
    /// (`phrase_index.h:135-160`): `(matched, total)` over the item's
    /// stored pronunciations, where `matched` sums the frequencies of the
    /// pronunciations that compare equal to `query` under
    /// `pinyin_compare_with_tones` (a tone-less query accepts every tone,
    /// an incomplete syllable every final) and `total` sums them all.
    /// `None` when the token's library is not loaded or owns no such item.
    #[must_use]
    pub fn pronunciation_possibility(
        &self,
        token: u32,
        query: &[ChewingKey],
    ) -> Option<(u64, u64)> {
        let library = self.library(token)?;
        let item = library.item(token)?;
        Some(item_possibility(&item, query))
    }

    /// What one trellis step prices of `token` over `query`: the
    /// `phrase_char_count` and [`Self::pronunciation_possibility`]
    /// pair, from one item read. `None` where the character count is.
    #[must_use]
    pub fn trellis_item(&self, token: u32, query: &[ChewingKey]) -> Option<TrellisItem> {
        let library = self.library(token)?;
        let item = library.item(token)?;
        let chars = item
            .phrase_codes()
            .try_fold(0_u32, |count, code| code.map(|_| count.saturating_add(1)))?;
        Some(TrellisItem {
            chars,
            pronunciation: Some(item_possibility(&item, query)),
            unigram: None,
        })
    }

    /// `token`'s pronunciations as `(spelling, freq)` pairs, in stored
    /// order — `get_nth_pronunciation` rendered through the chewing key
    /// tables (syllables `'`-joined, no tone digits).
    #[must_use]
    pub fn pronunciations(&self, token: u32) -> Option<Vec<LibraryPronunciation>> {
        let library = self.library(token)?;
        let item = library.item(token)?;
        let mut out = Vec::new();
        for view in item.pronunciations() {
            let mut syllables = Vec::new();
            let mut ok = true;
            for pair in view.keys.as_chunks::<2>().0.iter() {
                let packed = u16::from_le_bytes(*pair);
                let key = ChewingKey::from_packed(packed);
                let spelling = key.pinyin_spelling();
                if spelling.is_empty() {
                    ok = false;
                    break;
                }
                syllables.push(spelling);
            }
            if ok && view.keys.len().is_multiple_of(CHEWING_KEY_SIZE) && !syllables.is_empty() {
                out.push(LibraryPronunciation {
                    spelling: syllables.join("'"),
                    freq: u64::from(view.freq),
                });
            }
        }
        Some(out)
    }
}

/// [`PhraseLibraries::pronunciation_possibility`] over a resolved item,
/// comparing each stored pronunciation's packed keys in place.
fn item_possibility(item: &PhraseItemView<'_>, query: &[ChewingKey]) -> (u64, u64) {
    let mut matched: u64 = 0;
    let mut total: u64 = 0;
    for view in item.pronunciations() {
        if !view.keys.len().is_multiple_of(CHEWING_KEY_SIZE) {
            continue;
        }
        total = total.saturating_add(u64::from(view.freq));
        if view.keys.len() / CHEWING_KEY_SIZE == query.len()
            && query
                .iter()
                .zip(view.keys.as_chunks::<2>().0.iter())
                .all(|(q, pair)| {
                    q.matches_with_tones(ChewingKey::from_packed(u16::from_le_bytes(*pair)))
                })
        {
            matched = matched.saturating_add(u64::from(view.freq));
        }
    }
    (matched, total)
}

#[cfg(test)]
mod tests;
