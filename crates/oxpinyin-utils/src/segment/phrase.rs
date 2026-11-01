//! `FacadePhraseTable3::search` and `FacadePhraseIndex::get_phrase_item`
//! as the segmentation utilities use them.
//!
//! `PhraseTableEntry::search` appends a stored token only when its library
//! array is non-`NULL` (`phrase_large_table3.cpp:29-52`), and
//! `reduce_tokens` walks the sixteen arrays in library order
//! (`phrase_large_table3.h:77-112`). The stored DBM value's own order is
//! kept inside a library, so a stable sort by nibble reproduces the
//! pin's order exactly. `SEARCH_CONTINUED` comes from the key's presence
//! alone (`phrase_large_table3_tkrzwdb.cpp:124-153`).

use oxpinyin_data::{PhraseIndexTable, PhraseLibraries};

/// One phrase-table search's answer.
#[derive(Debug, Default)]
pub(crate) struct Found {
    /// `SEARCH_CONTINUED`: the key is present (a marker included).
    pub continued: bool,
    /// The tokens of the loaded libraries, in `reduce_tokens` order.
    pub tokens: Vec<u32>,
}

impl Found {
    /// `SEARCH_OK`: at least one token survives the library filter.
    pub(crate) fn ok(&self) -> bool {
        !self.tokens.is_empty()
    }

    /// `get_first_token` (`phrase_large_table3.h:101-112`), including its
    /// `assert(0 <= num && num <= 4)`.
    pub(crate) fn first_token(&self) -> u32 {
        if self.tokens.len() > 4 {
            // `reduce_tokens`'s reachable assertion.
            crate::segment::abort_on_pin_assert();
        }
        self.tokens
            .first()
            .copied()
            .unwrap_or(crate::segment::NULL_TOKEN)
    }
}

/// One phrase item's outward fields.
pub(crate) struct Item {
    /// `PhraseItem::get_phrase_string`'s stored words.
    pub words: Vec<u32>,
    /// `PhraseItem::get_phrase_length`.
    pub length: usize,
    /// `PhraseItem::get_unigram_frequency`.
    pub unigram: u32,
}

/// The table and the loaded-library set the pin's searches are made over.
pub(crate) struct PhraseSearch<'a> {
    table: Option<&'a PhraseIndexTable>,
    libraries: &'a PhraseLibraries,
}

impl<'a> PhraseSearch<'a> {
    pub(crate) const fn new(
        table: Option<&'a PhraseIndexTable>,
        libraries: &'a PhraseLibraries,
    ) -> Self {
        Self { table, libraries }
    }

    /// `FacadePhraseTable3::search` with the library filter.
    pub(crate) fn search(&self, words: &[u32]) -> Found {
        let Some(table) = self.table else {
            return Found::default();
        };
        let Ok(Some(tokens)) = table.search_ucs4(words) else {
            return Found::default();
        };
        let mut kept: Vec<u32> = tokens
            .into_iter()
            .filter(|token| self.libraries.is_loaded(((token >> 24) & 0x0F) as u8))
            .collect();
        // `reduce_tokens`: library nibble ascending, the DBM value's order
        // inside one library. `sort_by_key` is stable.
        kept.sort_by_key(|token| (token >> 24) & 0x0F);
        Found {
            continued: true,
            tokens: kept,
        }
    }

    /// `FacadePhraseIndex::get_phrase_index_total_freq()`: the loaded
    /// libraries' stored totals, a wrapping `guint32`.
    pub(crate) fn unigram_total(&self) -> u32 {
        self.libraries.unigram_total()
    }

    /// `FacadePhraseIndex::get_phrase_item`: `None` where the pin answers a
    /// non-`ERROR_OK` code (unloaded library, no item, or a stored word the
    /// reader cannot hand out).
    pub(crate) fn item(&self, token: u32) -> Option<Item> {
        // `PHRASE_INDEX_LIBRARY_INDEX(token)` masks the library nibble to
        // four bits (`novel_types.h:44`); the further bits are not part of
        // it.
        let library = self.libraries.library(((token >> 24) & 0x0F) << 24)?;
        let item = library.item(token)?;
        let words = item.phrase_codes().collect::<Option<Vec<u32>>>()?;
        Some(Item {
            words,
            length: item.phrase_length(),
            unigram: item.unigram(),
        })
    }
}
