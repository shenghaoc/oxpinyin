//! System dictionary backed by a lazy `ChewingTable` for the pinyin index.
//!
//! This is the P2 replacement for the eager `PinyinIndex` loading in
//! [`crate::dict::SystemDictionary`]. Instead of materializing every
//! pinyin-index record at startup, it opens a `ChewingTable` handle
//! and does per-keystroke point reads — the same architecture libpinyin
//! uses, and the reason its init is sub-millisecond.
//!
//! The phrase index (token → text) is still loaded eagerly (P3 scope).

use std::path::Path;

use compact_str::CompactString;
use oxpinyin_core::{ChewingKey, Completeness, Dictionary, PhraseEntry, PhraseToken, SyllableKey};
use oxpinyin_store::{DefaultStore, ReadStore};

use crate::chewing_table::{ChewingTable, RawChewingDbm};
use crate::dict::DictError;
use crate::table::{self, LeByteKey, TableError};

type PhraseIndex = Vec<(LeByteKey, CompactString)>;

/// A system dictionary backed by a lazy `ChewingTable`.
///
/// Lookups convert `SyllableKey` → `ChewingKey` → packed DBM key →
/// point read → decode `PinyinIndexItem2` → resolve tokens to
/// `PhraseEntry` from the phrase index.
///
/// Opening this dictionary does NOT scan the pinyin index.
pub struct ChewingDictionary {
    chewing_table: ChewingTable,
    phrase_index: PhraseIndex,
}

impl ChewingDictionary {
    /// Opens a chewing dictionary from a pinyin-index DBM and a
    /// phrase-index store.
    ///
    /// The pinyin index is opened lazily (no scan). The phrase index
    /// is loaded eagerly into a sorted vector (P3 scope).
    pub fn open(pinyin_index_path: &Path, phrase_index_path: &Path) -> Result<Self, DictError> {
        let store = DefaultStore::open_read_only(pinyin_index_path).map_err(TableError::from)?;
        let dbm = RawChewingDbm::new(store);
        let chewing_table = ChewingTable::new(Box::new(dbm));
        let phrase_index = load_phrase_index(phrase_index_path)?;
        Ok(Self {
            chewing_table,
            phrase_index,
        })
    }

    /// Phrase text for `token`, if present.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<&str> {
        phrase_lookup(&self.phrase_index, token)
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
            if let Some(text) = phrase_lookup(&self.phrase_index, item.token) {
                out.push(PhraseEntry::new(PhraseToken::new(item.token), text));
            }
        }
        Ok(())
    }
}

impl Dictionary for ChewingDictionary {
    type Syllable = SyllableKey;
    type Entry = PhraseEntry;
    type Error = DictError;

    fn lookup(&self, syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
        let mut entries = Vec::new();
        self.fill_lookup(syllables, &mut entries)?;
        Ok(entries)
    }

    fn lookup_into(
        &self,
        syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        self.fill_lookup(syllables, out)
    }

    fn lookup_addon_into(
        &self,
        _syllables: &[Self::Syllable],
        out: &mut Vec<Self::Entry>,
    ) -> Result<(), Self::Error> {
        out.clear();
        Ok(())
    }

    fn phrase_index_item_count(&self) -> Result<u64, Self::Error> {
        Ok(self.phrase_index.len() as u64)
    }

    fn phrase_prefix_exists(&self, syllables: &[Self::Syllable]) -> Result<bool, Self::Error> {
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
}

/// Converts a `SyllableKey` slice to a `ChewingKey` slice.
///
/// `None` when any syllable cannot be resolved — matching upstream, where
/// an unrecognized syllable prevents the lookup entirely rather than
/// substituting a zero key that could collide with prefix markers in the
/// DBM.
fn syllables_to_chewing_keys(syllables: &[SyllableKey]) -> Option<Vec<ChewingKey>> {
    syllables
        .iter()
        .map(|s| ChewingKey::from_pinyin(s.text()))
        .collect()
}

fn phrase_lookup(index: &[(LeByteKey, CompactString)], token: u32) -> Option<&str> {
    let needle = LeByteKey::new(token);
    index
        .binary_search_by(|(stored, _)| stored.cmp(&needle))
        .ok()
        .map(|pos| index[pos].1.as_str())
}

fn load_phrase_index(path: &Path) -> Result<PhraseIndex, DictError> {
    let mut map = PhraseIndex::new();
    table::for_each_row(path, |key, value| {
        if key.len() != 4 {
            return Ok::<(), DictError>(());
        }
        let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
        let text = std::str::from_utf8(value).map_err(|_| {
            DictError::Parse(format!("phrase text for token {token:#010x} is not UTF-8"))
        })?;
        map.push((LeByteKey::new(token), CompactString::from(text)));
        Ok::<(), DictError>(())
    })?;
    table::ensure_sorted_unique(&mut map);
    Ok(map)
}
