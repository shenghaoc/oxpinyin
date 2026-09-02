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

use std::path::Path;
use std::sync::OnceLock;

use oxpinyin_core::ChewingKey;

use crate::chewing_table::keys_match;
use crate::phrase_library::{LibraryError, PhraseLibrary};

pub use crate::system_files::SYSTEM_LIBRARY_FILES as SYSTEM_LIBRARY_STEMS;

/// One pronunciation of a library item: the joined pinyin spelling and
/// the pronunciation frequency — the rendering surface the export and
/// token-introspection paths need (`FacadePhraseIndex::get_phrase_item`
/// over `PhraseItem::get_nth_pronunciation`).
pub struct LibraryPronunciation {
    /// `'`-joined pinyin spelling, tone digits dropped (the consumers
    /// resolve it back to tone-less syllable keys).
    pub spelling: String,
    /// The pronunciation's stored count.
    pub freq: u64,
}

/// A loaded library plus its lazily tallied item count.
struct Loaded {
    library: PhraseLibrary,
    /// Resident items, tallied once on the first query — upstream keeps
    /// `m_length` as O(1) bookkeeping; the reader pays one offset-array
    /// scan per library at the first count query, never at open.
    item_count: OnceLock<u64>,
}

impl Loaded {
    fn item_count(&self) -> u64 {
        *self
            .item_count
            .get_or_init(|| self.library.items().count() as u64)
    }
}

/// The phrase libraries of one facade, dispatched by token nibble.
///
/// Every reader maps its chunk file on open and holds it; nothing here
/// scans anything eagerly beyond the mmap-time checksum pass upstream
/// itself performs. Libraries whose file is absent are simply unloaded —
/// token resolution answers `None` for their nibble, matching upstream's
/// `ERROR_NO_SUB_PHRASE_INDEX`.
pub struct PhraseLibraries {
    /// nibble → library, `None` = not loaded.
    by_nibble: [Option<Loaded>; 16],
}

impl PhraseLibraries {
    /// A facade with no library loaded.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_nibble: Default::default(),
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
    pub fn open(dir: &Path, stems: &[(u8, &str)]) -> Result<Self, LibraryError> {
        let mut this = Self::empty();
        for &(nibble, file) in stems {
            if this.is_loaded(nibble) {
                return Err(LibraryError::Format(format!(
                    "library nibble {nibble} named twice"
                )));
            }
            let path = dir.join(file);
            if !path.is_file() {
                continue;
            }
            this.load(nibble, &path)?;
        }
        Ok(this)
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
        if self.by_nibble[slot].is_some() {
            return Ok(false);
        }
        let library = PhraseLibrary::open(path)?;
        self.by_nibble[slot] = Some(Loaded {
            library,
            item_count: OnceLock::new(),
        });
        Ok(true)
    }

    /// Drops library `nibble` — `FacadePhraseIndex::unload`. `true` when
    /// a library was loaded there.
    pub fn unload(&mut self, nibble: u8) -> bool {
        match self.by_nibble.get_mut(usize::from(nibble)) {
            Some(slot) => slot.take().is_some(),
            None => false,
        }
    }

    /// Whether library `nibble` is loaded.
    #[must_use]
    pub fn is_loaded(&self, nibble: u8) -> bool {
        self.by_nibble
            .get(usize::from(nibble))
            .is_some_and(Option::is_some)
    }

    /// Whether any library is loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_nibble.iter().all(Option::is_none)
    }

    /// The library owning `token`'s nibble, when loaded. Nibbles at or
    /// beyond the 16-library count answer `None`, matching upstream's
    /// `PHRASE_INDEX_LIBRARY_INDEX` bound.
    #[must_use]
    pub fn library(&self, token: u32) -> Option<&PhraseLibrary> {
        self.by_nibble
            .get((token >> 24) as usize)?
            .as_ref()
            .map(|loaded| &loaded.library)
    }

    /// `FacadePhraseIndex::get_phrase_item`'s text half: the phrase
    /// behind `token`, if its library is loaded and owns the token.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<String> {
        self.library(token)?.item(token)?.phrase_text()
    }

    /// `PhraseItem::get_unigram_frequency` for `token` — the stored
    /// field, which carries `gen_unigram`'s `+1` on every system item.
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        Some(u64::from(self.library(token)?.item(token)?.unigram()))
    }

    /// `SubPhraseIndex::get_phrase_index_total_freq` summed over the
    /// loaded libraries — the facade total upstream's amplified-law
    /// denominator divides by.
    #[must_use]
    pub fn unigram_total(&self) -> u64 {
        self.unigram_total_where(|_| true)
    }

    /// [`Self::unigram_total`] over the loaded libraries `visible`
    /// accepts — the facade total after `unload` freed a sub-index.
    pub fn unigram_total_where(&self, visible: impl Fn(u8) -> bool) -> u64 {
        self.loaded_where(visible)
            .map(|loaded| u64::from(loaded.library.total_freq()))
            .fold(0_u64, u64::saturating_add)
    }

    /// The item count across the loaded libraries — the parity surface
    /// the ranking denominator reproduces (`FacadePhraseIndex` item
    /// count).
    #[must_use]
    pub fn item_count(&self) -> u64 {
        self.item_count_where(|_| true)
    }

    /// [`Self::item_count`] over the loaded libraries `visible` accepts.
    pub fn item_count_where(&self, visible: impl Fn(u8) -> bool) -> u64 {
        self.loaded_where(visible)
            .map(Loaded::item_count)
            .fold(0_u64, u64::saturating_add)
    }

    fn loaded_where(&self, visible: impl Fn(u8) -> bool) -> impl Iterator<Item = &Loaded> {
        self.by_nibble
            .iter()
            .enumerate()
            .filter_map(move |(nibble, slot)| {
                let loaded = slot.as_ref()?;
                visible(nibble as u8).then_some(loaded)
            })
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
        let item = self.library(token)?.item(token)?;
        let mut matched: u64 = 0;
        let mut total: u64 = 0;
        for view in item.pronunciations() {
            total = total.saturating_add(u64::from(view.freq));
            let stored: Vec<ChewingKey> = view
                .keys
                .chunks_exact(2)
                .map(|pair| ChewingKey::from_packed(u16::from_le_bytes([pair[0], pair[1]])))
                .collect();
            if keys_match(query, &stored) {
                matched = matched.saturating_add(u64::from(view.freq));
            }
        }
        Some((matched, total))
    }

    /// `token`'s pronunciations as `(spelling, freq)` pairs, in stored
    /// order — `get_nth_pronunciation` rendered through the chewing key
    /// tables (syllables `'`-joined, no tone digits).
    #[must_use]
    pub fn pronunciations(&self, token: u32) -> Option<Vec<LibraryPronunciation>> {
        let item = self.library(token)?.item(token)?;
        let mut out = Vec::new();
        for view in item.pronunciations() {
            let mut syllables = Vec::new();
            let mut ok = true;
            for pair in view.keys.chunks_exact(2) {
                let packed = u16::from_le_bytes([pair[0], pair[1]]);
                let key = ChewingKey::from_packed(packed);
                let spelling = key.pinyin_spelling();
                if spelling.is_empty() {
                    ok = false;
                    break;
                }
                syllables.push(spelling);
            }
            if ok && view.keys.len().is_multiple_of(2) && !syllables.is_empty() {
                out.push(LibraryPronunciation {
                    spelling: syllables.join("'"),
                    freq: u64::from(view.freq),
                });
            }
        }
        Some(out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn open_of_an_empty_directory_loads_nothing() {
        let dir = std::env::temp_dir().join(format!("oxpinyin-empty-libs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut libs = PhraseLibraries::open(&dir, SYSTEM_LIBRARY_STEMS).unwrap();
        assert!(libs.is_empty());
        assert!(libs.phrase_text(0x01000001).is_none());
        assert_eq!(libs.unigram_total(), 0);
        assert_eq!(libs.item_count(), 0);
        assert!(!libs.is_loaded(1));
        assert!(!libs.unload(1));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn out_of_range_nibbles_are_refused() {
        let mut libs = PhraseLibraries::empty();
        assert!(libs.load(16, Path::new("/no/such/file")).is_err());
        assert!(!libs.is_loaded(16));
        assert!(libs.library(0xFF00_0001).is_none());
    }
}
