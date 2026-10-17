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

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{OnceLock, RwLock};

use oxpinyin_core::{ChewingKey, TrellisItem};

use crate::chewing_table::key_matches;
use crate::phrase_library::{LibraryError, PhraseItemView, PhraseLibrary, RowFault, verify_chunk};

pub use crate::system_files::SYSTEM_LIBRARY_FILES as SYSTEM_LIBRARY_STEMS;

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

/// A loaded library plus its lazily tallied item count.
struct Loaded {
    library: PhraseLibrary,
    /// The file it was mapped from, as `load` was given it.
    path: PathBuf,
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
    by_nibble: [RwLock<Option<Loaded>>; 16],
    /// Files `open` found absent.
    unmapped: Vec<PathBuf>,
    /// `FacadePhraseIndex::m_total_freq` (074a2219 `phrase_index.h:440`)
    /// over every loaded library: each library's stored `total_freq` is
    /// added when it loads and subtracted when it unloads or is replaced,
    /// with no overflow check (`phrase_index.cpp:237-248`, `:261-269`).
    /// A `guint32`, so it wraps.
    total: AtomicU32,
    /// Bit `n` set when library `n` is loaded and holds at least one
    /// item: whether a frequency table is there to price with, which the
    /// wrapping [`Self::total`] cannot tell when it lands on zero.
    stocked: AtomicU32,
}

/// The fault and diagnostic paths from one validation of each selected
/// library. The first fault determines the pin's stopping site.
#[derive(Default)]
pub struct LibraryCheck {
    /// The first failed validation reaches `memory_chunk.h:493`.
    pub chunk_header_fault: bool,
    /// Files that produce the pin's ordinary `mmap failed` diagnostic.
    pub unmappable: Vec<PathBuf>,
}

impl PhraseLibraries {
    /// A facade with no library loaded.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            by_nibble: Default::default(),
            unmapped: Vec::new(),
            total: AtomicU32::new(0),
            stocked: AtomicU32::new(0),
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
            // `g_build_filename`: a name never leaves the directory.
            let path = crate::table_info::pin_join(dir, file.as_ref());
            match std::fs::metadata(&path) {
                // A directory is `MemoryChunk::mmap`'s to judge by its
                // reported size (`phrase_library::directory_chunk_fault`);
                // any other file that is not a regular one is skipped
                // without a word, as before.
                #[cfg(unix)]
                Ok(meta) if meta.is_dir() => {}
                Ok(meta) if !meta.is_file() => continue,
                Ok(_) => {}
                // The pin's `open` fails and `MemoryChunk::mmap` answers
                // `false`; the library stays unloaded here.
                Err(_) => {
                    this.unmapped.push(path);
                    continue;
                }
            }
            this.load(nibble, &path)?;
        }
        Ok(this)
    }

    /// The files [`PhraseLibraries::open`] found absent, in the order it
    /// met them: where the pin writes `mmap %s failed!` at init and goes on
    /// with an empty chunk (`pinyin.cpp:256`).
    #[must_use]
    pub fn unmapped(&self) -> &[PathBuf] {
        &self.unmapped
    }

    /// The files of the loaded libraries that `MemoryChunk::mmap` would
    /// refuse now, in nibble order: where the pin maps a system library
    /// again and writes `mmap %s failed!` (save, mask-out, reload,
    /// `pinyin.cpp:956`, `:1265`). Only the nibbles `visible` accepts are
    /// looked at — an unloaded library is skipped, as the pin's
    /// `get_range` skips it — and with `only` set, only that one.
    #[must_use]
    pub fn unmappable_now(&self, visible: impl Fn(u8) -> bool, only: Option<u8>) -> Vec<PathBuf> {
        (0u8..)
            .zip(&self.by_nibble)
            .filter(|&(nibble, _)| only.is_none_or(|wanted| wanted == nibble) && visible(nibble))
            .filter_map(|(_, slot)| {
                let slot = slot
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let loaded = slot.as_ref()?;
                verify_chunk(&loaded.path)
                    .err()
                    .is_some_and(|error| error.unmappable_path().is_some())
                    .then(|| loaded.path.clone())
            })
            .collect()
    }

    /// Validate each selected loaded library once, retaining both the
    /// first fault's classification and the reportable paths. The caller
    /// applies its operation's table-row and visibility guards.
    #[must_use]
    pub fn check_now(&self, selected: impl Fn(u8) -> bool, only: Option<u8>) -> LibraryCheck {
        let mut checked = LibraryCheck::default();
        let mut first_fault = false;
        for (_, slot) in (0u8..)
            .zip(&self.by_nibble)
            .filter(|&(nibble, _)| only.is_none_or(|wanted| wanted == nibble) && selected(nibble))
        {
            let slot = slot
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(loaded) = slot.as_ref() else {
                continue;
            };
            if let Err(error) = verify_chunk(&loaded.path) {
                if !first_fault {
                    checked.chunk_header_fault =
                        matches!(error, LibraryError::Row(RowFault::ChunkHeaderRead));
                    first_fault = true;
                }
                if error.unmappable_path().is_some() {
                    checked.unmappable.push(loaded.path.clone());
                }
            }
        }
        checked
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
        let slot = self.by_nibble[slot]
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if slot.is_some() {
            return Ok(false);
        }
        let library = PhraseLibrary::open(path)?;
        let total = self.total.get_mut();
        *total = total.wrapping_add(library.total_freq());
        if library.has_items() {
            *self.stocked.get_mut() |= 1 << nibble;
        }
        *slot = Some(Loaded {
            library,
            path: path.to_path_buf(),
            item_count: OnceLock::new(),
        });
        Ok(true)
    }

    /// Replaces a loaded sub-index with a fresh mapping of its system
    /// chunk, as `mask_out` does at 074a2219 (`pinyin.cpp:1253-1273`,
    /// `zhuyin.cpp:788-808`). An unloaded library stays unloaded.
    ///
    /// # Errors
    /// Returns [`LibraryError`] when the chunk cannot be mapped or decoded.
    pub fn reload(&self, nibble: u8, path: &Path) -> Result<bool, LibraryError> {
        let Some(slot) = self.by_nibble.get(usize::from(nibble)) else {
            return Ok(false);
        };
        let mut slot = slot
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(old) = slot.as_ref().map(|loaded| loaded.library.total_freq()) else {
            return Ok(false);
        };
        let library = PhraseLibrary::open(path)?;
        // The old sub-index's total leaves and the new one's enters, as
        // `FacadePhraseIndex::load` does over a loaded slot.
        self.total
            .fetch_add(library.total_freq().wrapping_sub(old), Ordering::SeqCst);
        if library.has_items() {
            self.stocked.fetch_or(1 << nibble, Ordering::SeqCst);
        } else {
            self.stocked.fetch_and(!(1 << nibble), Ordering::SeqCst);
        }
        *slot = Some(Loaded {
            library,
            path: path.to_path_buf(),
            item_count: OnceLock::new(),
        });
        Ok(true)
    }

    /// Drops library `nibble` — `FacadePhraseIndex::unload`. `true` when
    /// a library was loaded there.
    pub fn unload(&mut self, nibble: u8) -> bool {
        let Some(loaded) = self
            .by_nibble
            .get_mut(usize::from(nibble))
            .and_then(|slot| {
                slot.get_mut()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .take()
            })
        else {
            return false;
        };
        let total = self.total.get_mut();
        *total = total.wrapping_sub(loaded.library.total_freq());
        *self.stocked.get_mut() &= !(1 << nibble);
        true
    }

    /// Whether library `nibble` is loaded.
    #[must_use]
    pub fn is_loaded(&self, nibble: u8) -> bool {
        self.by_nibble.get(usize::from(nibble)).is_some_and(|slot| {
            slot.read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        })
    }

    /// Whether any library is loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_nibble.iter().all(|slot| {
            slot.read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        })
    }

    /// The library owning `token`'s nibble, when loaded. Nibbles at or
    /// beyond the 16-library count answer `None`, matching upstream's
    /// `PHRASE_INDEX_LIBRARY_INDEX` bound.
    #[must_use]
    pub fn library(&self, token: u32) -> Option<PhraseLibrary> {
        self.by_nibble
            .get((token >> 24) as usize)?
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|loaded| loaded.library.clone())
    }

    /// `FacadePhraseIndex::get_phrase_item`'s text half: the phrase
    /// behind `token`, if its library is loaded and owns the token.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<String> {
        self.library(token)?.item(token)?.phrase_text()
    }

    /// The character count of [`Self::phrase_text`], read without
    /// building the string; `None` where that text is `None`.
    #[must_use]
    pub fn phrase_char_count(&self, token: u32) -> Option<u32> {
        self.library(token)?
            .item(token)?
            .phrase_chars()
            .try_fold(0_u32, |count, char| char.map(|_| count.saturating_add(1)))
    }

    /// `PhraseItem::get_unigram_frequency` for `token` — the stored
    /// field, which carries `gen_unigram`'s `+1` on every system item.
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        Some(u64::from(self.library(token)?.item(token)?.unigram()))
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq` (074a2219
    /// `phrase_index.h:615-617`): the loaded libraries' stored totals as
    /// the facade keeps them, a wrapping `guint32`. Kept as the libraries
    /// load and unload, so a read is one load.
    #[must_use]
    pub fn unigram_total(&self) -> u32 {
        self.total.load(Ordering::SeqCst)
    }

    /// Whether a loaded library outside the `unloaded` bits holds an
    /// item: there is a unigram table to price with, whatever the
    /// wrapping total adds up to.
    #[must_use]
    pub fn has_items_unloaded(&self, unloaded: u32) -> bool {
        self.stocked.load(Ordering::SeqCst) & !unloaded != 0
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
            let slot = self.by_nibble[nibble]
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(loaded) = slot.as_ref() {
                total = total.wrapping_sub(loaded.library.total_freq());
            }
        }
        total
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
        self.sum_loaded(visible, Loaded::item_count)
    }

    fn sum_loaded(&self, visible: impl Fn(u8) -> bool, count: impl Fn(&Loaded) -> u64) -> u64 {
        self.by_nibble
            .iter()
            .enumerate()
            .filter_map(|(nibble, slot)| {
                if !visible(u8::try_from(nibble).unwrap_or(u8::MAX)) {
                    return None;
                }
                let slot = slot
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let loaded = slot.as_ref()?;
                Some(count(loaded))
            })
            .fold(0_u64, u64::saturating_add)
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
    /// [`Self::phrase_char_count`] and [`Self::pronunciation_possibility`]
    /// pair, from one item read. `None` where the character count is.
    #[must_use]
    pub fn trellis_item(&self, token: u32, query: &[ChewingKey]) -> Option<TrellisItem> {
        let library = self.library(token)?;
        let item = library.item(token)?;
        let chars = item
            .phrase_chars()
            .try_fold(0_u32, |count, char| char.map(|_| count.saturating_add(1)))?;
        Some(TrellisItem {
            chars,
            pronunciation: Some(item_possibility(&item, query)),
        })
    }

    /// `token`'s stored pronunciations as `(keys, freq)` rows in stored
    /// order — `get_nth_pronunciation` and `get_nth_pronunciation_freq`
    /// (`phrase_index.h` at the pin `074a2219`), tones kept. `None` when
    /// the token's library is not loaded or owns no such item.
    #[must_use]
    pub fn pronunciation_rows(&self, token: u32) -> Option<oxpinyin_core::PronunciationRows> {
        let library = self.library(token)?;
        let item = library.item(token)?;
        Some(
            item.pronunciations()
                .filter(|view| view.keys.len().is_multiple_of(2))
                .map(|view| {
                    let keys = view
                        .keys
                        .chunks_exact(2)
                        .map(|pair| ChewingKey::from_packed(u16::from_le_bytes([pair[0], pair[1]])))
                        .collect();
                    (keys, view.freq)
                })
                .collect(),
        )
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

/// [`PhraseLibraries::pronunciation_possibility`] over a resolved item,
/// comparing each stored pronunciation's packed keys in place.
fn item_possibility(item: &PhraseItemView<'_>, query: &[ChewingKey]) -> (u64, u64) {
    let mut matched: u64 = 0;
    let mut total: u64 = 0;
    for view in item.pronunciations() {
        if !view.keys.len().is_multiple_of(2) {
            continue;
        }
        total = total.saturating_add(u64::from(view.freq));
        if view.keys.len() / 2 == query.len()
            && query
                .iter()
                .zip(view.keys.chunks_exact(2))
                .all(|(q, pair)| {
                    key_matches(
                        q,
                        &ChewingKey::from_packed(u16::from_le_bytes([pair[0], pair[1]])),
                    )
                })
        {
            matched = matched.saturating_add(u64::from(view.freq));
        }
    }
    (matched, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_of_an_empty_directory_loads_nothing() {
        let dir = tempfile::Builder::new()
            .prefix("oxpinyin-empty-libs-")
            .tempdir()
            .unwrap();
        let mut libs = PhraseLibraries::open(dir.path(), SYSTEM_LIBRARY_STEMS).unwrap();
        assert!(libs.is_empty());
        assert!(libs.phrase_text(0x0100_0001).is_none());
        assert_eq!(libs.unigram_total(), 0);
        assert_eq!(libs.item_count(), 0);
        assert!(!libs.is_loaded(1));
        assert!(!libs.unload(1));
        dir.close().unwrap();
    }

    #[test]
    fn out_of_range_nibbles_are_refused() {
        let mut libs = PhraseLibraries::empty();
        assert!(libs.load(16, Path::new("/no/such/file")).is_err());
        assert!(!libs.is_loaded(16));
        assert!(libs.library(0xFF00_0001).is_none());
    }
}
