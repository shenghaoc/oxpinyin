//! System dictionary backed by lazy DBM readers for the pinyin and phrase
//! indexes, with token → phrase resolution through the mmap'd phrase
//! libraries.
//!
//! P2 replaced the eager pinyin-index materialization with a lazy
//! `ChewingTable`. P3 completes the split upstream itself has:
//!
//! * pinyin keys → candidates: the lazy `ChewingTable` over
//!   `pinyin_index.bin`;
//! * phrase text → tokens: the lazy [`PhraseTable`] over
//!   `phrase_index.bin` (`tokens_for_text`);
//! * token → phrase text / pronunciations / unigram: the P1
//!   [`crate::phrase_libraries::PhraseLibraries`] facade over the
//!   mmap'd per-library chunk files (`gb_char.bin` family) — the exact
//!   seam upstream's `FacadePhraseIndex` covers with its
//!   `SubPhraseIndex`es.
//!
//! Opening this dictionary scans nothing: two DBM handles and (at most)
//! four chunk-file mappings.

use std::path::Path;

use oxpinyin_core::{ChewingKey, Completeness, Dictionary, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_store::{DefaultStore, ReadStore};

use crate::chewing_table::{ChewingTable, RawChewingDbm};
use crate::dict::DictError;
use crate::phrase_libraries::{PhraseLibraries, SYSTEM_LIBRARY_STEMS};
use crate::phrase_table::PhraseTable;
use crate::table::TableError;

/// A system dictionary backed by lazy `ChewingTable` and `PhraseTable`
/// readers plus the mmap'd phrase libraries.
///
/// Lookups convert `SyllableKey` → `ChewingKey` → packed DBM key →
/// point read → decode `PinyinIndexItem2` → resolve tokens to
/// `PhraseEntry` through the phrase libraries.
///
/// Opening this dictionary does NOT scan the pinyin index, the phrase
/// DBM, or any chunk file beyond its mmap-time checksum pass.
pub struct ChewingDictionary {
    chewing_table: ChewingTable,
    phrase_table: Option<PhraseTable>,
    libraries: PhraseLibraries,
}

impl ChewingDictionary {
    /// Opens a chewing dictionary from a pinyin-index DBM and a
    /// directory carrying the per-library chunk files.
    ///
    /// Both halves are opened lazily (no scan).
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM cannot be opened or a present
    /// chunk file does not verify.
    pub fn open(pinyin_index_path: &Path, library_dir: &Path) -> Result<Self, DictError> {
        let store = DefaultStore::open_read_only(pinyin_index_path).map_err(TableError::from)?;
        let dbm = RawChewingDbm::new(store);
        let chewing_table = ChewingTable::new(Box::new(dbm));
        let libraries = PhraseLibraries::open(library_dir, SYSTEM_LIBRARY_STEMS)?;
        Ok(Self {
            chewing_table,
            phrase_table: None,
            libraries,
        })
    }

    /// Opens a chewing dictionary with a phrase DBM for text→tokens
    /// lookups.
    ///
    /// `phrase_dbm_path` is the libpinyin `phrase_index.bin` file (UCS-4
    /// keys, `u32 token[]` values). When provided, `tokens_for_text`
    /// does a direct lazy DBM lookup; without one it answers empty, the
    /// upstream shape when no phrase table is attached.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when a DBM cannot be opened or a present
    /// chunk file does not verify.
    pub fn open_with_phrase_dbm(
        pinyin_index_path: &Path,
        library_dir: &Path,
        phrase_dbm_path: &Path,
    ) -> Result<Self, DictError> {
        let pinyin_store =
            DefaultStore::open_read_only(pinyin_index_path).map_err(TableError::from)?;
        let pinyin_dbm = RawChewingDbm::new(pinyin_store);
        let chewing_table = ChewingTable::new(Box::new(pinyin_dbm));

        let phrase_store =
            DefaultStore::open_read_only(phrase_dbm_path).map_err(TableError::from)?;
        let phrase_dbm = RawChewingDbm::new(phrase_store);
        let phrase_table = PhraseTable::new(Box::new(phrase_dbm));

        let libraries = PhraseLibraries::open(library_dir, SYSTEM_LIBRARY_STEMS)?;
        Ok(Self {
            chewing_table,
            phrase_table: Some(phrase_table),
            libraries,
        })
    }

    /// Phrase text for `token`, if the token's library is loaded and
    /// owns it.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<String> {
        self.libraries.phrase_text(token)
    }

    /// The library item's stored unigram count for `token`
    /// (`PhraseItem::get_unigram_frequency`), if the item exists.
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        self.libraries.unigram_count(token)
    }

    /// The total unigram frequency across the loaded libraries —
    /// `FacadePhraseIndex::get_phrase_index_total_freq` over its
    /// sub-indexes.
    #[must_use]
    pub fn unigram_total(&self) -> u64 {
        self.libraries.unigram_total()
    }

    /// `token`'s pronunciations as `(spelling, freq)` pairs, in stored
    /// order — the item's `get_nth_pronunciation` list rendered the way
    /// the export surface produces them.
    #[must_use]
    pub fn pronunciations(&self, token: u32) -> Vec<(String, u64)> {
        self.libraries
            .pronunciations(token)
            .map(|prons| {
                prons
                    .into_iter()
                    .map(|pron| (pron.spelling, pron.freq))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Tokens whose phrase text is exactly `text`.
    ///
    /// When a `PhraseTable` is attached, a direct DBM lookup; empty
    /// otherwise (upstream answers nothing when no phrase table is
    /// attached).
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or the value decode
    /// fails.
    pub fn tokens_for_text(&self, text: &str) -> Result<Vec<u32>, DictError> {
        match self.phrase_table {
            Some(ref table) => table.search(text),
            None => Ok(Vec::new()),
        }
    }

    fn fill_lookup(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        out.clear();
        if syllables.is_empty() {
            return Ok(());
        }
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(());
        };
        let (result, items) = self.chewing_table.search(&keys)?;
        if !result.has_ok() {
            return Ok(());
        }
        for item in &items {
            // Upstream's entry search drops tokens whose sub-index is
            // not loaded (`PhraseTableEntry::search`'s NULL-array skip);
            // a token with no resident library item is invisible here.
            let Some(text) = self.libraries.phrase_text(item.token) else {
                continue;
            };
            out.push(PhraseEntry::new(PhraseToken::new(item.token), text));
        }
        Ok(())
    }
}

impl Dictionary for ChewingDictionary {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, DictError> {
        let mut entries = Vec::new();
        self.fill_lookup(syllables, &mut entries)?;
        Ok(entries)
    }

    fn lookup_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        self.fill_lookup(syllables, out)
    }

    fn lookup_addon_into(
        &self,
        _syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        out.clear();
        Ok(())
    }

    fn phrase_index_item_count(&self) -> Result<u64, DictError> {
        Ok(self.libraries.item_count())
    }

    fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> Result<bool, DictError> {
        if syllables.is_empty() {
            return Ok(true);
        }
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(false);
        };
        let has_partial = syllables
            .iter()
            .any(|s| s.completeness() == Completeness::Partial);
        let (result, _) = if has_partial {
            self.chewing_table.search_incomplete(&keys)?
        } else {
            self.chewing_table.search(&keys)?
        };
        Ok(result.has_ok() || result.has_continued())
    }

    fn tokens_for_text(&self, text: &str) -> Vec<PhraseToken> {
        self.tokens_for_text(text)
            .unwrap_or_default()
            .into_iter()
            .map(PhraseToken::new)
            .collect()
    }
}

/// Converts syllables to ChewingKeys. Returns `None` if any syllable
/// cannot be resolved — matching upstream, where an unrecognized
/// syllable prevents the lookup entirely rather than substituting a
/// zero key that could collide with prefix markers in the DBM.
fn syllables_to_chewing_keys(syllables: &[SyllableKey]) -> Option<Vec<ChewingKey>> {
    syllables
        .iter()
        .map(|s| ChewingKey::from_pinyin(s.text()))
        .collect()
}
