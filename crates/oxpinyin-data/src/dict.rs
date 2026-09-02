//! The system and addon dictionaries over libpinyin's own data files.
//!
//! Upstream's `pinyin_init` opens three things per facade: a
//! `ChewingLargeTable2` (pinyin keys → `PinyinIndexItem2` records), a
//! `PhraseLargeTable3` (UCS-4 text → tokens), and a `FacadePhraseIndex`
//! of mmap'd per-library chunk files (token → phrase item). Nothing is
//! scanned at open: the two DBMs are handles, the chunk files are mapped
//! and checksummed. Every lookup is a point read plus a chunk-item read.
//!
//! [`SystemDictionary`] is the default facade (`pinyin_index.bin`,
//! `phrase_index.bin`, `gb_char.bin` … `merged.bin`);
//! [`AddonDictionary`] the addon facade (`addon_pinyin_index.bin`,
//! `addon_phrase_index.bin`, `art.bin` … `technology.bin`, libraries
//! loaded on demand by `pinyin_load_addon_phrase_library`).
//!
//! Lookups convert `SyllableKey` → `ChewingKey`, run
//! `ChewingLargeTable2::search` (incomplete-index selection and the
//! `pinyin_compare_with_tones` record filter included), and resolve every
//! surviving token through its library — a token whose library is not
//! loaded is invisible, upstream's `NULL == head` skip.

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use oxpinyin_core::{ChewingKey, Dictionary, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_store::{DefaultStore, ReadStore};

use crate::chewing_table::{ChewingTable, PinyinIndexItem, RawChewingDbm, prefix_keys_match};
use crate::phrase_libraries::PhraseLibraries;
use crate::phrase_table::PhraseTable;
use crate::system_files::{
    ADDON_LIBRARY_FILES, SYSTEM_LIBRARY_FILES, SystemDbm, addon_library_file,
};
use crate::table::TableError;

/// Error conditions for dictionary lookups.
#[derive(Debug)]
pub enum DictError {
    /// A table-level error (I/O, the store backend).
    Table(TableError),
    /// Stored bytes did not decode under libpinyin's record layout.
    Parse(String),
    /// A phrase-library chunk file failed to verify.
    Library(crate::phrase_library::LibraryError),
}

impl fmt::Display for DictError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Table(e) => write!(f, "table error: {e}"),
            Self::Parse(msg) => write!(f, "parse error: {msg}"),
            Self::Library(e) => write!(f, "phrase library error: {e}"),
        }
    }
}

impl std::error::Error for DictError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Table(e) => Some(e),
            Self::Parse(_) => None,
            Self::Library(e) => Some(e),
        }
    }
}

impl From<TableError> for DictError {
    fn from(e: TableError) -> Self {
        Self::Table(e)
    }
}

impl From<crate::phrase_library::LibraryError> for DictError {
    fn from(e: crate::phrase_library::LibraryError) -> Self {
        Self::Library(e)
    }
}

/// Converts syllables to `ChewingKey`s, tone zero. `None` when any
/// syllable is not a content-table spelling — upstream's parser never
/// produces such a key, so the lookup answers nothing rather than
/// substituting a zero key that could collide with a prefix marker.
fn syllables_to_chewing_keys(syllables: &[SyllableKey]) -> Option<Vec<ChewingKey>> {
    syllables
        .iter()
        .map(|s| ChewingKey::from_pinyin(s.text()))
        .collect()
}

fn open_tree(path: &Path) -> Result<ChewingTable, DictError> {
    let store = DefaultStore::open_read_only(path).map_err(TableError::from)?;
    Ok(ChewingTable::new(Box::new(RawChewingDbm::new(store))))
}

fn open_phrase_table(path: &Path) -> Result<PhraseTable, DictError> {
    let store = DefaultStore::open_read_only(path).map_err(TableError::from)?;
    Ok(PhraseTable::new(Box::new(RawChewingDbm::new(store))))
}

/// Resolves the surviving records of one search through `libraries`:
/// text from the chunk item, possibility over its pronunciations. A token
/// without a loaded item is dropped (upstream's `NULL == head` skip).
fn resolve_items(
    libraries: &PhraseLibraries,
    keys: &[ChewingKey],
    items: &[PinyinIndexItem],
    out: &mut Vec<PhraseEntry>,
) {
    for item in items {
        let Some(text) = libraries.phrase_text(item.token) else {
            continue;
        };
        let mut entry = PhraseEntry::new(PhraseToken::new(item.token), text);
        if let Some((matched, total)) = libraries.pronunciation_possibility(item.token, keys) {
            entry = entry.with_pronunciation_possibility(matched, total);
        }
        out.push(entry);
    }
}

/// `search_suggestion` tokens resolved to `(token, text)` rows in the
/// order upstream hands them to `_compute_predicted_prefix_candidates`:
/// `search_suggestion` files each token into its library's array
/// (`PhraseTableEntry::search`'s `tokens[PHRASE_INDEX_LIBRARY_INDEX]`)
/// and `reduce_tokens` concatenates those arrays library by library, so
/// the list is grouped by library nibble ascending, and inside a group
/// it is the DBM's cursor order — byte-lexical over the UCS-4 keys, the
/// order [`PhraseTable::search_suggestion`] already yields. A stable sort
/// by nibble over the walk is exactly that.
fn resolve_suggestions(libraries: &PhraseLibraries, tokens: Vec<u32>) -> Vec<(u32, String)> {
    let mut rows: Vec<(u32, String)> = tokens
        .into_iter()
        .filter_map(|token| Some((token, libraries.phrase_text(token)?)))
        .collect();
    rows.sort_by_key(|(token, _)| token >> 24);
    rows
}

/// The byte-lexical sort key of a phrase text in a UCS-4 keyed DBM: the
/// little-endian `u32` scalars concatenated — the cursor order
/// `PhraseLargeTable3` walks, which `pinyin_guess_predicted_candidates`
/// exposes as the order of tied rows. Public so the user seam's
/// suggestions can be merged in the same order.
#[must_use]
pub fn ucs4_walk_key(text: &str) -> Vec<u8> {
    text.chars()
        .flat_map(|ch| (ch as u32).to_le_bytes())
        .collect()
}

/// The `SEARCH_CONTINUED` probe restricted to visible phrases: whether
/// some phrase whose pinyin equals or extends `keys` has a token
/// `visible` accepts and a loaded library item.
fn visible_extension_exists(
    table: &ChewingTable,
    libraries: &PhraseLibraries,
    keys: &[ChewingKey],
    visible: &dyn Fn(u32) -> bool,
) -> Result<bool, DictError> {
    table.walk_extensions(keys, &mut |_, items| {
        Ok(items.iter().any(|item| {
            prefix_keys_match(keys, &item.keys)
                && visible(item.token)
                && libraries
                    .library(item.token)
                    .is_some_and(|library| library.item(item.token).is_some())
        }))
    })
}

// ── the default facade ───────────────────────────────────────────

/// The system dictionary: the default facade's pinyin DBM, phrase DBM,
/// and the four system phrase libraries.
pub struct SystemDictionary {
    pinyin: ChewingTable,
    phrase: PhraseTable,
    libraries: Arc<PhraseLibraries>,
}

impl SystemDictionary {
    /// Opens the default facade from a system data directory — a
    /// libpinyin install's `data/` on Kyoto Cabinet and tkrzw, an
    /// `oxpinyin-datagen` output directory on every backend
    /// ([`SystemDbm::file_name`] names the DBMs, [`SYSTEM_LIBRARY_FILES`]
    /// the chunk files).
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when either DBM cannot be opened or a present
    /// chunk file does not verify. A missing chunk file leaves that
    /// library unloaded, as `FacadePhraseIndex::load` failing does.
    pub fn open(system_dir: &Path) -> Result<Self, DictError> {
        Self::open_files(
            &system_dir.join(SystemDbm::PinyinIndex.file_name()),
            &system_dir.join(SystemDbm::PhraseIndex.file_name()),
            system_dir,
        )
    }

    /// [`Self::open`] with every path spelled out.
    ///
    /// # Errors
    ///
    /// As [`Self::open`].
    pub fn open_files(
        pinyin_index: &Path,
        phrase_index: &Path,
        library_dir: &Path,
    ) -> Result<Self, DictError> {
        let pinyin = open_tree(pinyin_index)?;
        let phrase = open_phrase_table(phrase_index)?;
        let libraries = PhraseLibraries::open(library_dir, SYSTEM_LIBRARY_FILES)?;
        Ok(Self {
            pinyin,
            phrase,
            libraries: Arc::new(libraries),
        })
    }

    /// The facade's phrase libraries, shared with the language model
    /// (the unigram counts live in the chunk items).
    #[must_use]
    pub fn libraries(&self) -> &Arc<PhraseLibraries> {
        &self.libraries
    }

    /// Phrase text for `token`, if its library is loaded and owns it.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<String> {
        self.libraries.phrase_text(token)
    }

    /// `PhraseItem::get_unigram_frequency` for `token` — the stored field
    /// (`gen_unigram`'s `+1` included).
    #[must_use]
    pub fn unigram_count(&self, token: u32) -> Option<u64> {
        self.libraries.unigram_count(token)
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq` over the loaded
    /// libraries.
    #[must_use]
    pub fn unigram_total(&self) -> u64 {
        self.libraries.unigram_total()
    }

    /// The facade's item count.
    #[must_use]
    pub fn item_count(&self) -> u64 {
        self.libraries.item_count()
    }

    /// The item count over the libraries `visible` accepts by nibble.
    pub fn item_count_where(&self, visible: impl Fn(u8) -> bool) -> u64 {
        self.libraries.item_count_where(visible)
    }

    /// `token`'s pronunciations as `(spelling, freq)` pairs, in stored
    /// order.
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

    /// Tokens whose phrase text is exactly `text` — `PhraseLargeTable3::search`.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or the value decode fails.
    pub fn tokens_for_text(&self, text: &str) -> Result<Vec<u32>, DictError> {
        self.phrase.search(text)
    }

    /// Tokens whose phrase text starts with `prefix` and is longer, when
    /// `prefix` itself is a stored key — `PhraseLargeTable3::search_suggestion`
    /// resolved to `(token, text)` rows, tokens without a loaded item
    /// dropped, in upstream's order (library groups, cursor order inside).
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM walk or a value decode fails.
    pub fn suggest_after(&self, prefix: &str) -> Result<Vec<(u32, String)>, DictError> {
        let tokens = self.phrase.search_suggestion(prefix)?;
        Ok(resolve_suggestions(&self.libraries, tokens))
    }

    /// The `SEARCH_CONTINUED` probe with a per-token visibility filter:
    /// whether a phrase `visible` accepts has a pinyin equal to or
    /// extending `syllables`. Routed by the runtime once a library has
    /// been unloaded, so the n-best widen probe never extends a path that
    /// only invisible phrases could complete. Costs a bounded walk over the
    /// prefix's extensions; the plain [`Dictionary::phrase_prefix_exists`]
    /// stays a point read.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM walk or a value decode fails.
    pub fn phrase_prefix_exists_visible(
        &self,
        syllables: &[SyllableKey],
        visible: impl Fn(u32) -> bool,
    ) -> Result<bool, DictError> {
        if syllables.is_empty() {
            return Ok(true);
        }
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(false);
        };
        visible_extension_exists(&self.pinyin, &self.libraries, &keys, &visible)
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
        let items = self.pinyin.search(&keys)?;
        resolve_items(&self.libraries, &keys, &items, out);
        Ok(())
    }
}

impl Dictionary for SystemDictionary {
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

    fn phrase_index_item_count(&self) -> Result<u64, DictError> {
        Ok(self.libraries.item_count())
    }

    /// `SEARCH_CONTINUED`: the query's index key exists in the DBM, as a
    /// phrase or as a marker for longer phrases — one point read, no
    /// record decode (the bit is set whenever the key exists).
    fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> Result<bool, DictError> {
        if syllables.is_empty() {
            return Ok(true);
        }
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(false);
        };
        self.pinyin.key_exists(&keys)
    }

    fn tokens_for_text(&self, text: &str) -> Vec<PhraseToken> {
        self.tokens_for_text(text)
            .unwrap_or_default()
            .into_iter()
            .map(PhraseToken::new)
            .collect()
    }
}

// ── the addon facade ─────────────────────────────────────────────

/// One addon phrase item, the `get_phrase_item` half of the
/// choose-promotion path (`pinyin.cpp:2534-2549`).
pub struct AddonPhraseItem {
    /// Phrase text.
    pub text: String,
    /// Pronunciations as `(spelling, count)` pairs, in stored order.
    pub pronunciations: Vec<(String, u64)>,
    /// The item's stored unigram frequency.
    pub unigram: u64,
}

/// The addon dictionary: the addon facade's DBM pair plus the addon
/// libraries loaded so far.
///
/// Upstream attaches `addon_pinyin_index.bin` / `addon_phrase_index.bin`
/// at init and loads chunk files per `pinyin_load_addon_phrase_library`;
/// every lookup consults the whole addon index and keeps only tokens
/// whose library is loaded. Both DBMs are optional: an install without
/// addon data has no addon candidates.
pub struct AddonDictionary {
    pinyin: Option<ChewingTable>,
    phrase: Option<PhraseTable>,
    libraries: PhraseLibraries,
}

impl AddonDictionary {
    /// Opens the addon DBM pair from `system_dir` when present; no
    /// library is loaded yet.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when a present DBM cannot be opened.
    pub fn open(system_dir: &Path) -> Result<Self, DictError> {
        let pinyin_path = system_dir.join(SystemDbm::AddonPinyinIndex.file_name());
        let phrase_path = system_dir.join(SystemDbm::AddonPhraseIndex.file_name());
        let pinyin = pinyin_path
            .is_file()
            .then(|| open_tree(&pinyin_path))
            .transpose()?;
        let phrase = phrase_path
            .is_file()
            .then(|| open_phrase_table(&phrase_path))
            .transpose()?;
        Ok(Self {
            pinyin,
            phrase,
            libraries: PhraseLibraries::empty(),
        })
    }

    /// An addon facade with neither DBM nor libraries.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            pinyin: None,
            phrase: None,
            libraries: PhraseLibraries::empty(),
        }
    }

    /// Loads addon library `index`'s chunk file from `dir` —
    /// `pinyin_load_addon_phrase_library`. `false` when the index names no
    /// addon library, the library is already loaded, or the file is
    /// missing or malformed.
    pub fn load(&mut self, index: u8, dir: &Path) -> bool {
        let Some(file) = addon_library_file(index) else {
            return false;
        };
        let path = dir.join(file);
        if !path.is_file() {
            return false;
        }
        self.libraries.load(index, &path).unwrap_or(false)
    }

    /// Drops addon library `index` — `pinyin_unload_addon_phrase_library`.
    /// Answers `true` whether or not it was loaded, the pin's unconditional
    /// `unload` (`pinyin.cpp:124-131`).
    pub fn unload(&mut self, index: u8) -> bool {
        self.libraries.unload(index);
        true
    }

    /// Whether library `index` is loaded.
    #[must_use]
    pub fn is_loaded(&self, index: u8) -> bool {
        self.libraries.is_loaded(index)
    }

    /// Whether no library is loaded (the facade owns no items).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.libraries.is_empty()
    }

    /// The addon chunk files `table.conf` names, for callers that want to
    /// load them all.
    #[must_use]
    pub const fn library_files() -> &'static [(u8, &'static str)] {
        ADDON_LIBRARY_FILES
    }

    /// The stored unigram of `token`'s addon item, if its library is
    /// loaded.
    #[must_use]
    pub fn unigram_freq(&self, token: u32) -> Option<u64> {
        self.libraries.unigram_count(token)
    }

    /// The addon facade's total frequency, `None` while no library is
    /// loaded.
    #[must_use]
    pub fn unigram_total(&self) -> Option<u64> {
        if self.libraries.is_empty() {
            return None;
        }
        Some(self.libraries.unigram_total())
    }

    /// Phrase text for `token`, if its addon library is loaded.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<String> {
        self.libraries.phrase_text(token)
    }

    /// The addon item behind `token`.
    #[must_use]
    pub fn phrase_item(&self, token: u32) -> Option<AddonPhraseItem> {
        let text = self.libraries.phrase_text(token)?;
        let pronunciations = self
            .libraries
            .pronunciations(token)?
            .into_iter()
            .map(|pron| (pron.spelling, pron.freq))
            .collect();
        let unigram = self.libraries.unigram_count(token).unwrap_or(0);
        Some(AddonPhraseItem {
            text,
            pronunciations,
            unigram,
        })
    }

    /// The addon lookup pass: every addon-index record for `syllables`
    /// whose library is loaded.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or a value decode fails.
    pub fn lookup_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<(), DictError> {
        out.clear();
        let Some(pinyin) = self.pinyin.as_ref() else {
            return Ok(());
        };
        if self.libraries.is_empty() || syllables.is_empty() {
            return Ok(());
        }
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(());
        };
        let items = pinyin.search(&keys)?;
        resolve_items(&self.libraries, &keys, &items, out);
        Ok(())
    }

    /// The addon facade's `SEARCH_CONTINUED` probe.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read fails.
    pub fn prefix_exists(&self, syllables: &[SyllableKey]) -> Result<bool, DictError> {
        let Some(pinyin) = self.pinyin.as_ref() else {
            return Ok(false);
        };
        if self.libraries.is_empty() {
            return Ok(false);
        }
        if syllables.is_empty() {
            return Ok(true);
        }
        let Some(keys) = syllables_to_chewing_keys(syllables) else {
            return Ok(false);
        };
        pinyin.key_exists(&keys)
    }

    /// Tokens whose addon phrase text is exactly `text`.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or the value decode fails.
    pub fn tokens_for_text(&self, text: &str) -> Result<Vec<u32>, DictError> {
        match self.phrase.as_ref() {
            Some(table) => table.search(text),
            None => Ok(Vec::new()),
        }
    }
}
