//! The system and addon dictionaries over the backend's native data files.
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

use crate::vocab::phrase_index_library_index;
use std::path::Path;

use crate::{
    model::{Dictionary, PhraseIndexRanges, PhraseItemRead, TokenPhrase, TrellisItem},
    parser::ChewingKey,
    vocab::{PhraseEntry, PhraseText, PhraseToken, SyllableKey},
};

use crate::data::PhraseLibraries;
use crate::data::SystemDbm;
use crate::data::TableError;
use crate::data::chewing_table::{ChewingTable, prefix_keys_match};
use crate::data::phrase_library::LibraryError;
use crate::data::phrase_library::RowFault;
use crate::data::phrase_table::PhraseTable;
use crate::data::{FileType, TableConf, TableRow};

/// Error conditions for dictionary lookups.
#[derive(Debug, thiserror::Error)]
pub enum DictError {
    /// A table-level error (I/O, the store backend).
    #[error("table error: {0}")]
    Table(#[from] TableError),
    /// Stored bytes did not decode under libpinyin's record layout.
    #[error("parse error")]
    Parse,
    /// A phrase-library chunk file failed to verify.
    #[error("phrase library error: {0}")]
    Library(#[source] LibraryError),
}

/// `token`'s entry: its text, priced against `keys`; `None` when the
/// library is unloaded or the item cannot be read.
///
/// The text is `PhraseItem::get_phrase_string` then `g_ucs4_to_utf8`
/// (`_token_get_phrase`, `pinyin.cpp:1627-1647` at 074a2219); the entry
/// keeps the item's stored words and its stored character count, so a zero
/// or a non-scalar word changes neither the ranking nor the token's item.
/// The C adapters' `phrase_text_bytes` produces the pin's bytes.
fn resolve_item(
    libraries: &PhraseLibraries,
    pricing_keys: Option<&[ChewingKey]>,
    token: u32,
) -> Option<PhraseEntry> {
    let library = libraries.library(token)?;
    let item = library.item(token)?;
    let mut entry = PhraseEntry::new(token, item.phrase_text()?);
    if let Some(keys) = pricing_keys
        && let Some((matched, total)) = libraries.pronunciation_possibility(token, keys)
    {
        entry = entry.with_pronunciation_possibility(matched, total);
    }
    Some(entry)
}

/// Whether `token`'s library is loaded: a system chunk, or one of the
/// `user_nibbles` — the pin's non-`NULL` range array.
fn library_loaded(libraries: &PhraseLibraries, user_nibbles: u32, token: u32) -> bool {
    libraries.library(token).is_some()
        || 1_u32
            .checked_shl(u32::from(phrase_index_library_index(token)))
            .is_some_and(|bit| user_nibbles & bit != 0)
}

// ── the default facade ───────────────────────────────────────────

/// The default dictionary's pinyin index, phrase index and phrase libraries selected by
/// the system rows in the table configuration.
#[derive(Clone, Copy)]
pub struct SystemDictionary<'a> {
    tables: &'a SystemTables,
    libraries: &'a PhraseLibraries,
}

/// System DBM handles and row classification, separate from chunk ownership.
pub struct SystemTables {
    pinyin: ChewingTable,
    phrase: PhraseTable,
    /// The default sub-indices `table.conf` types `USER_FILE`: libraries the
    /// pin has loaded (a user chunk, or a fresh empty one) although no
    /// system chunk backs them, bit `n` for sub-index `n`.
    user_nibbles: u32,
}

impl std::ops::Deref for SystemDictionary<'_> {
    type Target = SystemTables;
    fn deref(&self) -> &Self::Target {
        self.tables
    }
}

impl SystemTables {
    /// Combines the native table handles and the configured user-library bits.
    #[must_use]
    #[doc(hidden)]
    pub const fn new(pinyin: ChewingTable, phrase: PhraseTable, user_nibbles: u32) -> Self {
        Self {
            pinyin,
            phrase,
            user_nibbles,
        }
    }

    /// Borrows the native pinyin index for phonetic reads.
    #[must_use]
    #[doc(hidden)]
    pub const fn pinyin(&self) -> &ChewingTable {
        &self.pinyin
    }

    /// Whether the main pinyin-table attach failed.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn pinyin_attachment_failed(&self) -> bool {
        self.pinyin.attachment_failed()
    }

    /// Whether the main phrase-table attach failed.
    #[must_use]
    pub(crate) fn phrase_attachment_failed(&self) -> bool {
        self.phrase.attachment_failed()
    }

    /// Closes the main indexes in the pin's pinyin/phrase order
    /// (`pinyin.cpp:1205-1206`, `zhuyin.cpp:744-745` at 074a2219).
    pub(crate) fn finish_attachments(&mut self) {
        self.pinyin.finish_attachment();
        self.phrase.finish_attachment();
    }

    /// Whether both main indexes are closed.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn attachments_finished(&self) -> bool {
        self.pinyin.attachment_finished() && self.phrase.attachment_finished()
    }

    /// Borrow these tables and the owner's phrase storage for one operation.
    pub fn read<'a>(&'a self, libraries: &'a PhraseLibraries) -> SystemDictionary<'a> {
        SystemDictionary {
            tables: self,
            libraries,
        }
    }
    /// The system half of the first index pair in init (pinyin.cpp:357).
    pub(crate) fn attach_pinyin(system_dir: &Path) -> Result<ChewingTable, DictError> {
        Ok(ChewingTable::new(
            crate::data::chewing_table::DefaultDbm::attach(
                &crate::input::build_path(
                    system_dir,
                    std::ffi::OsStr::new(&SystemDbm::PinyinIndex.file_name()),
                ),
                crate::store::DbmKind::Tree,
            )?,
        ))
    }

    /// Attach the system phrase index after the user pinyin index (pinyin.cpp:369).
    pub(crate) fn attach_phrase(
        system_dir: &Path,
        conf: &TableConf,
        pinyin: ChewingTable,
    ) -> Result<Self, DictError> {
        Ok(Self::new(
            pinyin,
            PhraseTable::new(crate::data::chewing_table::DefaultDbm::attach(
                &crate::input::build_path(
                    system_dir,
                    std::ffi::OsStr::new(&SystemDbm::PhraseIndex.file_name()),
                ),
                crate::store::DbmKind::Tree,
            )?),
            user_nibbles(conf),
        ))
    }
}

/// The sub-indexes of `conf`'s `USER_FILE` rows, as a bit mask.
fn user_nibbles(conf: &TableConf) -> u32 {
    conf.user_libraries()
        .fold(0, |mask, (index, _)| mask | (1_u32 << index))
}

impl SystemDictionary<'_> {
    /// The system lookup: the tokens `ChewingLargeTable2::search` finds for
    /// `syllables`, each with its text, in the order the table stores them
    /// (`chewing_large_table2.h:91-133` at 074a2219). The pin's search
    /// carries tokens only; a pronunciation's price is read later, for one
    /// token, through [`Self::trellis_item`]. A token of a detached library
    /// resolves through `live`, its live owner. The search makes one DBM
    /// get, into `row`, the caller's buffer. The answer is whether a token
    /// of a loaded library had no readable item, and whether the query's
    /// index key exists (`SEARCH_CONTINUED`, from that get).
    ///
    /// # Errors
    /// Returns a dictionary error when the system index cannot be searched.
    pub(crate) fn lookup_with_live(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
        mut visible: impl FnMut(u32) -> bool,
        mut live: impl FnMut(u32) -> Option<PhraseEntry>,
    ) -> Result<(crate::model::UnreadableRows, bool), DictError> {
        out.clear();
        let mut unreadable = crate::model::UnreadableRows::default();
        if syllables.is_empty() {
            return Ok((unreadable, false));
        }
        let mut keys: Vec<_> = syllables.iter().map(|s| s.chewing_key()).collect();
        for (key, &tone) in keys.iter_mut().zip(tones) {
            *key = key.with_tone(tone);
        }
        let present = self.pinyin.search_into(&keys, row, |token| {
            // Unloaded or removed tokens are not malformed loaded items.
            // Their live owner can be absent precisely because it was unloaded.
            if !visible(token) {
                return;
            }
            let entry = if self
                .libraries()
                .library(token)
                .is_some_and(|library| library.is_detached())
            {
                live(token)
            } else {
                resolve_item(self.libraries(), None, token)
            };
            if let Some(entry) = entry {
                out.push(entry);
            } else if library_loaded(self.libraries(), self.user_nibbles, token) {
                unreadable.count += 1;
            }
        })?;
        Ok((unreadable, present))
    }

    /// Whether `token`'s library is loaded in this facade — a system
    /// chunk, or a `table.conf` `USER_FILE` sub-index — the pin's
    /// non-`NULL` `PhraseIndexRanges` array for it.
    #[must_use]
    pub(crate) fn library_loaded(&self, token: u32) -> bool {
        library_loaded(self.libraries(), self.user_nibbles, token)
    }

    /// The facade's phrase libraries, shared with the language model
    /// (the unigram counts live in the chunk items).
    #[must_use]
    pub fn libraries(&self) -> &PhraseLibraries {
        self.libraries
    }

    /// Phrase text for `token`, if its library is loaded and owns it.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<PhraseText> {
        self.libraries().phrase_text(token)
    }

    /// `PhraseItem::get_unigram_frequency` for `token` — the stored field
    /// (`gen_unigram`'s `+1` included).
    #[must_use]
    #[cfg(test)]
    pub(crate) fn unigram_count(&self, token: u32) -> Option<u64> {
        self.libraries().unigram_count(token)
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq` over the loaded
    /// libraries.
    #[cfg(test)]
    #[must_use]
    pub fn unigram_total(&self) -> u64 {
        u64::from(self.libraries().unigram_total())
    }

    /// `token`'s pronunciations as `(spelling, freq)` pairs, in stored
    /// order.
    #[cfg(test)]
    #[must_use]
    pub fn pronunciations(&self, token: u32) -> Vec<(String, u64)> {
        self.libraries()
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

    /// Tokens whose raw UCS-4 span is exactly `words` — the non-scalar
    /// counterpart of [`Self::tokens_for_text`], compared as stored keys.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or the value decode fails.
    pub fn tokens_for_ucs4(&self, words: &[u32]) -> Result<Vec<u32>, DictError> {
        self.phrase.search_ucs4(words)
    }

    /// Raw phrase-table suggestion tokens in upstream's order (library
    /// groups, cursor order inside) — `PhraseLargeTable3::search_suggestion`.
    ///
    /// Tokens whose live item is absent are kept: the prediction caller
    /// needs the pin's stale-length filter and its sole-missing/class-(b)
    /// split (`pinyin.cpp:2389-2405`, `:2443-2451`; #1231).
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM walk or a value decode fails.
    pub(crate) fn suggestion_tokens(&self, prefix: &[u32]) -> Result<Vec<u32>, DictError> {
        self.phrase.search_suggestion(prefix)
    }

    /// The suggestion tokens and overlong-key classification from one
    /// system-index lookup and range walk. `None` is the pin's abort.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM walk or a value decode fails.
    pub(crate) fn checked_extension_tokens(
        &self,
        syllables: &[SyllableKey],
    ) -> Result<Option<Vec<u32>>, DictError> {
        if syllables.is_empty() {
            return Ok(Some(Vec::new()));
        }
        let keys: Vec<_> = syllables.iter().map(|s| s.chewing_key()).collect();
        let mut tokens: Vec<u32> = Vec::new();
        let overlong = self.pinyin.walk_suggestions(&keys, |length, items| {
            // The query-length row is the phrase itself — the pin's cursor
            // starts one past it (`Jump` + `Next`).
            if length > keys.len() {
                tokens.extend(
                    items
                        .iter()
                        .filter(|item| prefix_keys_match(&keys, &item.keys))
                        .map(|item| item.token),
                );
            }
            Ok(false)
        })?;
        if overlong {
            return Ok(None);
        }
        // `reduce_tokens`'s library grouping: tokens filed by nibble
        // ascending, cursor order within a library.
        crate::model::sort_tokens_by_library(&mut tokens);
        Ok(Some(tokens))
    }

    /// `ChewingTableEntry::search` then `convert`
    /// (`chewing_large_table2.h:91-133` at the pin `074a2219`) over the
    /// table's items in stored order, tone-filtered by the search: tokens
    /// only, as the pin's ranges are. An item whose library is not loaded
    /// — not in this facade, or not `visible` to the caller's unload mask
    /// — is skipped without closing the open range, the pin's
    /// `NULL == head` skip. A loaded item keeps its place whether or not
    /// [`Self::trellis_item`] can read it.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or a value decode fails.
    #[cfg(test)]
    pub(crate) fn trellis_ranges_with(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        visible: impl Fn(u32) -> bool,
    ) -> Result<(), DictError> {
        let keys: Vec<_> = syllables.iter().map(|s| s.chewing_key()).collect();
        let query: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| key.with_tone(tones.get(index).copied().unwrap_or(0)))
            .collect();
        let items = self.pinyin.search(&query)?;
        ranges.convert(
            items
                .iter()
                .copied()
                .filter(|&token| self.library_loaded(token) && visible(token))
                .map(|token| (token, Some(()))),
        );
        Ok(())
    }

    /// [`Self::trellis_ranges_with`] and the prefix probe from one DBM get:
    /// the ranges, and whether the query's index key exists
    /// (`SEARCH_CONTINUED`, set from the same get that serves the search).
    /// The get reads into `scratch`, the caller's buffer.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the DBM read or a value decode fails.
    pub(crate) fn trellis_ranges_continued_with(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        visible: impl Fn(u32) -> bool,
        scratch: &mut Vec<u8>,
    ) -> Result<bool, DictError> {
        if syllables.is_empty() {
            return Ok(true);
        }
        let keys: Vec<_> = syllables.iter().map(|s| s.chewing_key()).collect();
        let query: Vec<_> = keys
            .iter()
            .enumerate()
            .map(|(index, key)| key.with_tone(tones.get(index).copied().unwrap_or(0)))
            .collect();
        let (continued, tokens) = self.pinyin().search_continued(&query, scratch)?;
        ranges.convert(
            tokens
                .into_iter()
                .filter(|&token| self.library_loaded(token) && visible(token))
                .map(|token| (token, Some(()))),
        );
        Ok(continued)
    }

    /// What a step prices of `token` over `syllables`: its character count
    /// and its possibility against the query's tone-less keys — the
    /// ordinary lookup's pricing of that token. `None` when the library is
    /// unloaded or the item cannot be read.
    #[must_use]
    pub fn trellis_item(&self, syllables: &[SyllableKey], token: u32) -> Option<TrellisItem> {
        self.trellis_item_keys(
            &syllables
                .iter()
                .map(|s| s.chewing_key())
                .collect::<Vec<_>>(),
            token,
        )
    }

    /// [`Self::trellis_item`] over the query's keys, already converted
    /// from the syllables.
    #[must_use]
    pub fn trellis_item_keys(&self, keys: &[ChewingKey], token: u32) -> Option<TrellisItem> {
        self.libraries().trellis_item(token, keys)
    }

    /// [`Self::lookup_into_flagged`]'s body: [`Self::lookup_with_live`]
    /// with every library visible and each item read from its own chunk.
    #[cfg(test)]
    fn fill_lookup(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<crate::model::UnreadableRows, DictError> {
        self.lookup_with_live(
            syllables,
            &[],
            out,
            &mut Vec::new(),
            |_| true,
            |token| resolve_item(self.libraries(), None, token),
        )
        .map(|(unreadable, _)| unreadable)
    }
}

impl SystemDictionary<'_> {
    /// Looks up the entries matching `syllables` in the system table.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub fn lookup(&self, syllables: &[SyllableKey]) -> Result<Vec<PhraseEntry>, DictError> {
        let mut entries = Vec::new();
        self.fill_lookup(syllables, &mut entries)?;
        Ok(entries)
    }

    /// Every matching pronunciation index record in library, range and item
    /// order, each resolved to its entry. `tones` supplies the query tones.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub fn trellis_records(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
    ) -> Result<Vec<PhraseEntry>, DictError> {
        let keys: Vec<_> = syllables.iter().map(|s| s.chewing_key()).collect();
        let mut ranges = PhraseIndexRanges::new();
        self.trellis_ranges(syllables, tones, &mut ranges)?;
        Ok(ranges
            .walk_tokens()
            .filter_map(|(token, ())| resolve_item(self.libraries(), Some(&keys), token))
            .collect())
    }

    /// `ChewingTableEntry::search` then `convert` over the system table:
    /// `SystemDictionary::trellis_ranges_with` with every loaded
    /// library visible.
    #[cfg(test)]
    pub fn trellis_ranges(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
    ) -> Result<(), DictError> {
        self.trellis_ranges_with(syllables, tones, ranges, |_| true)
    }

    /// [`Self::lookup`] into a caller-owned buffer, also answering whether the
    /// lookup met a token of a loaded library whose item could not be read.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when the dictionary backend cannot serve the lookup.
    #[cfg(test)]
    pub fn lookup_into_flagged(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
    ) -> Result<bool, DictError> {
        self.fill_lookup(syllables, out).map(|rows| rows.count != 0)
    }

    /// `SEARCH_CONTINUED`: the query's index key exists in the DBM, as a
    /// phrase or as a marker for longer phrases — one point read, no
    /// record decode (the bit is set whenever the key exists).
    #[cfg(test)]
    #[must_use]
    pub fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> bool {
        if syllables.is_empty() {
            return true;
        }
        let keys: Vec<_> = syllables.iter().map(|s| s.chewing_key()).collect();
        self.pinyin.key_exists(&keys)
    }
}

impl Dictionary for SystemDictionary<'_> {
    type Syllable = SyllableKey;
    type Error = DictError;
    type PathMemo = ();

    fn trellis_ranges_continued(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        ranges: &mut PhraseIndexRanges<()>,
        scratch: &mut Vec<u8>,
    ) -> Result<bool, DictError> {
        self.trellis_ranges_continued_with(syllables, tones, ranges, |_| true, scratch)
    }

    /// [`Self::trellis_item`]; the tones, the memo and the facts are not
    /// used.
    fn trellis_item_with_facts(
        &self,
        syllables: &[SyllableKey],
        _tones: &[u8],
        token: u32,
        _memo: &(),
        _facts: &mut crate::sentence_facts::SentenceFacts,
    ) -> Result<Option<TrellisItem>, DictError> {
        Ok(self.trellis_item(syllables, token))
    }

    /// The stored length and text of the token in its loaded library.
    fn trellis_text_with_facts(
        &self,
        _syllables: &[SyllableKey],
        _tones: &[u8],
        token: u32,
        _facts: &mut crate::sentence_facts::SentenceFacts,
    ) -> Result<Option<TokenPhrase>, DictError> {
        Ok(self.libraries().token_phrase(token))
    }

    fn candidate_search_into(
        &self,
        syllables: &[SyllableKey],
        tones: &[u8],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<(crate::model::UnreadableRows, bool), Self::Error> {
        self.lookup_with_live(
            syllables,
            tones,
            out,
            row,
            |_| true,
            |token| resolve_item(self.libraries(), None, token),
        )
    }

    /// The system dictionary has no addon facade: no entries and no
    /// continuation bit.
    fn search_addon_into(
        &self,
        _syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
        _row: &mut Vec<u8>,
    ) -> Result<bool, DictError> {
        out.clear();
        Ok(false)
    }

    fn checked_extension_tokens(
        &self,
        syllables: &[SyllableKey],
    ) -> Result<Option<Vec<PhraseToken>>, DictError> {
        Ok(SystemDictionary::checked_extension_tokens(self, syllables)?
            .map(|tokens| tokens.into_iter().collect()))
    }

    /// Converts only an all-scalar span to text and answers
    /// [`Self::tokens_for_text`] for it; a span with a non-scalar is empty.
    fn tokens_for_ucs4(&self, ucs4: &[u32]) -> Vec<PhraseToken> {
        let mut text = String::with_capacity(ucs4.len());
        for &word in ucs4 {
            let Some(character) = char::from_u32(word) else {
                return Vec::new();
            };
            text.push(character);
        }
        self.tokens_for_text(&text)
            .unwrap_or_default()
            .into_iter()
            .collect()
    }

    fn phrase_span_searches_are_observable(&self) -> bool {
        false
    }

    fn phrase_span_continues(&self, _ucs4: &[u32]) -> bool {
        true
    }

    /// `_token_get_phrase`'s system half: the loaded library item's text.
    fn phrase_text_for_token(&self, token: u32) -> Option<PhraseText> {
        self.libraries().phrase_text(token)
    }

    /// The length of [`Dictionary::phrase_text_for_token`]'s text.
    fn phrase_length_for_token(&self, token: u32) -> Option<usize> {
        Dictionary::phrase_text_for_token(self, token).map(|text| text.phrase_length())
    }

    /// `get_phrase_item` then the item's stored pronunciations, read from
    /// the loaded library ([`PhraseLibraries::pronunciation_rows`]).
    fn phrase_item_pronunciations(&self, token: PhraseToken) -> Result<PhraseItemRead, DictError> {
        Ok(self
            .libraries()
            .pronunciation_rows(token)
            .map_or(PhraseItemRead::NoItem, PhraseItemRead::Item))
    }
}

// ── the addon facade ─────────────────────────────────────────────

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
    /// The `addon` rows of `table.conf`, by sub-index.
    rows: Vec<TableRow>,
}

fn addon_rows(conf: &TableConf) -> Vec<TableRow> {
    (0..=u8::MAX)
        .take(usize::from(crate::vocab::PHRASE_INDEX_LIBRARY_COUNT))
        .map(|index| {
            conf.addon_row(index)
                .cloned()
                .unwrap_or_else(TableRow::unused)
        })
        .collect()
}

impl AddonDictionary {
    /// Attaches both runtime addon indexes even when a file is absent.
    /// Zhuyin must continue to use [`Self::without_indexes`].
    ///
    /// # Errors
    /// Returns failures before native attach.
    pub(crate) fn attach_with(system_dir: &Path, conf: &TableConf) -> Result<Self, DictError> {
        let path = |kind: SystemDbm| {
            crate::input::build_path(system_dir, std::ffi::OsStr::new(&kind.file_name()))
        };
        Ok(Self {
            pinyin: Some(ChewingTable::new(
                crate::data::chewing_table::DefaultDbm::attach(
                    &path(SystemDbm::AddonPinyinIndex),
                    crate::store::DbmKind::Tree,
                )?,
            )),
            phrase: Some(PhraseTable::new(
                crate::data::chewing_table::DefaultDbm::attach(
                    &path(SystemDbm::AddonPhraseIndex),
                    crate::store::DbmKind::Tree,
                )?,
            )),
            libraries: PhraseLibraries::empty(),
            rows: addon_rows(conf),
        })
    }

    /// Closes the addon pinyin then phrase attachments
    /// (`pinyin.cpp:1211-1212` at 074a2219).
    pub(crate) fn finish_attachments(&mut self) {
        if let Some(table) = &mut self.pinyin {
            table.finish_attachment();
        }
        if let Some(table) = &mut self.phrase {
            table.finish_attachment();
        }
    }

    /// Whether every addon index this facade holds is closed.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn attachments_finished(&self) -> bool {
        self.pinyin
            .as_ref()
            .is_none_or(ChewingTable::attachment_finished)
            && self
                .phrase
                .as_ref()
                .is_none_or(PhraseTable::attachment_finished)
    }

    /// An addon facade with neither DBM nor libraries.
    #[cfg(test)]
    #[must_use]
    pub fn empty() -> Self {
        Self::without_indexes(&TableConf::stock(0.312_699))
    }

    /// An addon facade that keeps `conf`'s `addon` rows and opens nothing:
    /// no index DBM, no library. For a facade whose pin never opens the
    /// addon indexes (`zhuyin.cpp`'s context has no addon facade); the rows
    /// stay those `table.conf` configured rather than the stock ones.
    #[must_use]
    pub(crate) fn without_indexes(conf: &TableConf) -> Self {
        Self {
            pinyin: None,
            phrase: None,
            libraries: PhraseLibraries::empty(),
            rows: addon_rows(conf),
        }
    }

    /// The configured `addon` row of sub-index `index`.
    #[cfg(test)]
    #[must_use]
    pub fn row(&self, index: u8) -> Option<&TableRow> {
        self.rows.get(usize::from(index))
    }

    /// Whether either addon index has an open DBM handle.
    ///
    /// Independent of whether any addon phrase library is loaded.
    #[cfg(test)]
    #[must_use]
    pub const fn has_open_indexes(&self) -> bool {
        self.pinyin.is_some() || self.phrase.is_some()
    }

    /// Loads addon library `index`'s chunk file from `dir` —
    /// `pinyin_load_addon_phrase_library`: `Ok(true)` loaded, `Ok(false)`
    /// for an index that names no
    /// addon library or one already loaded (the pin's guard, silent), and
    /// an error otherwise — [`LibraryError::Unmappable`] when the chunk
    /// does not map, where the pin writes `mmap %s failed!`
    /// (`pinyin.cpp:290`). A directory is judged as `MemoryChunk::mmap`
    /// does (Unix: [`LibraryError::Row`] from its reported size, else the
    /// unmappable refusal); any other file that is not a regular file is not
    /// an error: it answers `Ok(false)`.
    ///
    /// # Errors
    ///
    /// [`LibraryError`] when the file is absent or does not verify.
    pub(crate) fn load_reporting(&mut self, index: u8, dir: &Path) -> Result<bool, LibraryError> {
        // `pinyin_load_addon_phrase_library`: an unused row answers false
        // (`pinyin.cpp:487`); any other row that is not a DICTIONARY
        // asserts (`:491`), loaded or not.
        let Some(row) = self.rows.get(usize::from(index)) else {
            return Ok(false);
        };
        match row.file_type {
            FileType::NotUsed => return Ok(false),
            FileType::Dictionary => {}
            FileType::SystemFile | FileType::UserFile => {
                std::process::abort();
            }
        }
        let file = row.system.clone();
        if self.libraries.is_loaded(index) {
            return Ok(false);
        }
        let Some(file) = file else {
            return Err(LibraryError::Row(RowFault::NullChunkName));
        };
        let path = crate::input::build_chunk_path(dir, file.as_ref());
        match std::fs::metadata(&path) {
            // A directory is `MemoryChunk::mmap`'s to judge by its reported
            // size where the boundary says the loader opens directories; on
            // Windows the open is refused and the row is skipped below.
            Ok(meta) if meta.is_dir() && crate::input::chunk::opens_directories() => {}
            Ok(meta) if !meta.is_file() => return Ok(false),
            Ok(_) => {}
            Err(error) => {
                return Err(LibraryError::Unmappable {
                    path,
                    cause: Box::new(LibraryError::Io(error)),
                });
            }
        }
        self.libraries.load(index, &path)
    }

    /// Drops addon library `index` — `pinyin_unload_addon_phrase_library`.
    /// Answers `true` whether or not it was loaded, the pin's unconditional
    /// `unload` (`pinyin.cpp:124-131`).
    pub fn unload(&mut self, index: u8) -> bool {
        self.libraries.unload(index);
        true
    }

    /// Whether no library is loaded (the facade owns no items).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.libraries.is_empty()
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
        Some(u64::from(self.libraries.unigram_total()))
    }

    /// The addon item behind `token` as the pin's `get_phrase_item` hands
    /// it out: the item's stored bytes, copied verbatim (`phrase_index.cpp:
    /// 33-43`, the choose-promotion read at `pinyin.cpp:2534-2549`). The
    /// words, packed keys with their tones and frequencies stay as stored.
    #[must_use]
    pub fn phrase_item(&self, token: u32) -> Option<Vec<u8>> {
        Some(
            self.libraries
                .library(token)?
                .item(token)?
                .as_bytes()
                .to_vec(),
        )
    }

    /// Reads the addon index once for rows and `SEARCH_CONTINUED`
    /// (`pinyin.cpp:2231-2256` at 074a2219). The row determines
    /// continuation even if its library is unloaded. The loaded-library
    /// filter applies to the returned candidates.
    ///
    /// # Errors
    ///
    /// Returns [`DictError`] when a value decode fails.
    pub(crate) fn search_into(
        &self,
        syllables: &[SyllableKey],
        out: &mut Vec<PhraseEntry>,
        row: &mut Vec<u8>,
    ) -> Result<bool, DictError> {
        out.clear();
        let Some(pinyin) = self.pinyin.as_ref() else {
            return Ok(false);
        };
        let keys: Vec<_> = syllables.iter().map(|s| s.chewing_key()).collect();
        pinyin.search_into(&keys, row, |token| {
            out.extend(resolve_item(&self.libraries, None, token));
        })
    }
}

#[cfg(test)]
pub(crate) mod tests;
