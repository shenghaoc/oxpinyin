//! The mmap-backed phrase libraries as one token-dispatched facade —
//! the runtime seam `FacadePhraseIndex` provides over its
//! `SubPhraseIndex`es (`phrase_index.cpp`).
//!
//! token → phrase (text, pronunciations, unigram) resolves here, through
//! the P1 [`PhraseLibrary`] readers, instead of through a resident
//! token→text map. Upstream's split between the mmap'd chunk files (the
//! per-library phrase index) and the DBM phrase table (phrase → tokens)
//! is kept exactly: this facade covers only the first half.

use std::path::Path;

use oxpinyin_core::ChewingKey;

use crate::phrase_library::{LibraryError, PhraseLibrary};

/// The four system libraries' chunk-file stems, nibble-indexed —
/// `table.conf`'s `default …_DICTIONARY` rows' system files.
///
/// Nibbles 1..=4 are the system libraries. Addon libraries share the
/// nibble space (art is also nibble 4), but upstream resolves them
/// through a second, separate `FacadePhraseIndex` — this facade covers
/// only the system seam, exactly as the runtime dispatches.
pub const SYSTEM_LIBRARY_STEMS: &[(u8, &str)] = &[
    (1, "gb_char.bin"),
    (2, "gbk_char.bin"),
    (3, "opengram.bin"),
    (4, "merged.bin"),
];

/// One pronunciation of a library item: the joined pinyin spelling and
/// the pronunciation frequency — the rendering surface the export and
/// token-introspection paths need (`FacadePhraseIndex::get_phrase_item`
/// over `PhraseItem::get_nth_pronunciation`).
pub struct LibraryPronunciation {
    /// `'`-joined pinyin spelling, tone digits dropped (the renderers
    /// the old eager map produced carried none).
    pub spelling: String,
    /// The pronunciation's stored count.
    pub freq: u64,
}

/// The system phrase libraries, dispatched by token nibble.
///
/// Slot is the lazy half: every reader maps its chunk file on open and
/// holds it; nothing here scans anything eagerly beyond the
/// mmap-time checksum pass upstream itself performs. Libraries whose
/// file is absent are simply unloaded — token resolution answers `None`
/// for their nibble, matching upstream's `ERROR_NO_SUB_PHRASE_INDEX`.
pub struct PhraseLibraries {
    /// nibble → library, `None` = not loaded.
    by_nibble: [Option<PhraseLibrary>; 16],
    /// Resident items across the loaded libraries, tallied once on the
    /// first use — upstream maintains `m_length` as O(1) bookkeeping
    /// (`add_index`/`remove_index`), so the reader pays the slot scan at
    /// most once, at the first item-count query, never at open.
    item_count: std::sync::OnceLock<u64>,
}

impl PhraseLibraries {
    /// Opens the libraries named by `(nibble, file)` pairs from `dir`;
    /// a pair whose file is missing loads as unloaded.
    ///
    /// # Errors
    ///
    /// Returns [`LibraryError`] when a present file does not verify as a
    /// well-formed `SubPhraseIndex` chunk, or when a nibble is named
    /// twice.
    pub fn open(dir: &Path, stems: &[(u8, &str)]) -> Result<Self, LibraryError> {
        let mut by_nibble: [Option<PhraseLibrary>; 16] = Default::default();
        for &(nibble, file) in stems {
            let nibble = nibble as usize;
            if nibble >= 16 {
                return Err(LibraryError::Format(format!(
                    "library nibble {nibble} out of range"
                )));
            }
            if by_nibble[nibble].is_some() {
                return Err(LibraryError::Format(format!(
                    "library nibble {nibble} named twice"
                )));
            }
            let path = dir.join(file);
            if !path.is_file() {
                continue;
            }
            by_nibble[nibble] = Some(PhraseLibrary::open(&path)?);
        }
        Ok(Self {
            by_nibble,
            item_count: std::sync::OnceLock::new(),
        })
    }

    /// The library owning `token`'s nibble, when loaded. Nibbles at or
    /// beyond the 16-library count answer `None`, matching upstream's
    /// `PHRASE_INDEX_LIBRARY_INDEX` bound.
    #[must_use]
    pub fn library(&self, token: u32) -> Option<&PhraseLibrary> {
        self.by_nibble.get((token >> 24) as usize)?.as_ref()
    }

    /// `FacadePhraseIndex::get_phrase_item`'s text half: the phrase
    /// behind `token`, if its library is loaded and owns the token.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<String> {
        self.library(token)?.item(token)?.phrase_text()
    }

    /// `PhraseItem::get_unigram_frequency` for `token` (0 for absent).
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        Some(u64::from(self.library(token)?.item(token)?.unigram()))
    }

    /// `SubPhraseIndex::get_phrase_index_total_freq` summed over the
    /// loaded libraries — the facade total upstream's amplified-law
    /// denominator divides by.
    #[must_use]
    pub fn unigram_total(&self) -> u64 {
        self.by_nibble
            .iter()
            .flatten()
            .map(PhraseLibrary::total_freq)
            .map(u64::from)
            .fold(0_u64, u64::saturating_add)
    }

    /// The item count across the loaded libraries — the parity surface
    /// the ranking denominator reproduces (`FacadePhraseIndex` item
    /// count). Upstream keeps it as O(1) `m_length` bookkeeping; the
    /// file reader pays one offset-array scan per facade on the first
    /// query (offset arrays are sized to the libraries' token ranges,
    /// not to the full 16M slot space), and caches it thereafter.
    #[must_use]
    pub fn item_count(&self) -> u64 {
        let by_nibble = &self.by_nibble;
        *self.item_count.get_or_init(|| {
            by_nibble
                .iter()
                .flatten()
                .map(|library| library.items().count() as u64)
                .fold(0_u64, u64::saturating_add)
        })
    }

    /// `token`'s pronunciations as `(spelling, freq)` pairs, in stored
    /// order — `get_nth_pronunciation` rendered through the chewing
    /// key tables, joined the way the old eager map produced them
    /// (syllables `'`-joined, no tone digits).
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
            if ok && view.keys.len() % 2 == 0 && !syllables.is_empty() {
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
        let libs = PhraseLibraries::open(&dir, SYSTEM_LIBRARY_STEMS).unwrap();
        assert!(libs.phrase_text(0x01000001).is_none());
        assert_eq!(libs.unigram_total(), 0);
        assert_eq!(libs.item_count(), 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
