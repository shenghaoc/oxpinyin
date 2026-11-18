//! Lookup over the store's own session tables and item arena.
//!
//! Session queries borrow the canonical tables for the duration of each
//! operation.

use std::cell::RefCell;

use crate::{PhraseEntry, PhraseText, PhraseToken, SyllableKey};

use crate::user::user_index::UserIndex;
use crate::user::user_library::SessionPhrases;

/// A query handle over the user store. Session handles borrow the canonical
/// owner; they never hold a second index, phrase table or copy of item text.
/// Each method ends its phrase borrow before returning to its caller.
#[derive(Clone, Debug)]
pub struct UserLookup<'a> {
    /// The session's arenas.
    phrases: &'a RefCell<SessionPhrases>,
    /// For system libraries with an original, the live buffers that hold
    /// their appended items.
    items: &'a RefCell<crate::user::system_items::SystemItems>,
}

impl<'a> UserLookup<'a> {
    pub(crate) fn session(
        phrases: &'a RefCell<SessionPhrases>,
        items: &'a RefCell<crate::user::system_items::SystemItems>,
    ) -> Self {
        Self { phrases, items }
    }

    fn with_index<R>(&self, read: impl FnOnce(&UserIndex) -> R) -> R {
        read(&self.phrases.borrow().index)
    }

    /// Phrases whose stored pinyin matches `syllables` under upstream's
    /// `pinyin_compare_with_tones`
    /// (`docs/findings/data-formats.md` §4): equal length, and
    /// syllable-by-syllable, the stored pronunciation's syllable equals the
    /// query when the query syllable is complete, or shares its initial
    /// when the query syllable is incomplete.
    ///
    /// A query with any incomplete syllable is routed through the
    /// initial-projected keyspace, the same "one
    /// incomplete-index probe per key path" upstream's
    /// `ChewingLargeTable2::search` (`chewing_large_table2.cpp:161-172` at
    /// pin `074a2219`) uses — so the scan never needs to enumerate the
    /// key's completions to find matching user entries.
    #[must_use]
    pub fn lookup(&self, syllables: &[SyllableKey]) -> Vec<PhraseEntry> {
        let keys: Vec<_> = syllables.iter().map(|key| key.chewing_key()).collect();
        let mut entries = Vec::new();
        self.lookup_into(&keys, &mut entries);
        entries
    }

    /// The trellis's with-tones query over the same stored records.
    #[must_use]
    pub(crate) fn search_toned(&self, keys: &[crate::ChewingKey]) -> Vec<u32> {
        self.with_index(|index| index.search(keys))
    }

    /// Append the matching live entries in stored token order.
    pub(crate) fn lookup_into(&self, keys: &[crate::ChewingKey], out: &mut Vec<PhraseEntry>) {
        self.lookup_resolving_missing(keys, out, |_| None);
    }

    /// Appends live matches and calls the fallback for tokens without a live item.
    pub(crate) fn lookup_resolving_missing(
        &self,
        keys: &[crate::ChewingKey],
        out: &mut Vec<PhraseEntry>,
        mut missing: impl FnMut(u32) -> Option<PhraseText>,
    ) {
        let phrases = self.phrases.borrow();
        phrases.index.visit_matches(keys, |token| {
            if let Some(text) = session_text(&phrases, self.items, token).or_else(|| missing(token))
            {
                out.push(PhraseEntry::new(PhraseToken::new(token), text));
            }
        });
    }

    /// `SEARCH_CONTINUED` probe over user-file pinyin keys.
    #[must_use]
    pub fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> bool {
        let keys: Vec<_> = syllables.iter().map(|key| key.chewing_key()).collect();
        self.with_index(|index| index.has_prefix(&keys))
    }

    /// Phrase text from the current item, without decoding its readings.
    #[cfg(test)]
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<PhraseText> {
        session_text(&self.phrases.borrow(), self.items, token)
    }

    /// Tokens whose stored UCS-4 key is exactly `words`, non-scalar words
    /// included.
    #[must_use]
    pub fn tokens_for_ucs4(&self, words: &[u32]) -> Vec<u32> {
        self.phrases
            .borrow()
            .phrase_table
            .tokens(&crate::user::phrase_table::key_ucs4(words))
            .unwrap_or_default()
    }

    /// `ChewingLargeTable2::search_suggestion` over the user seam — the
    /// user chewing table `_add_phrase` populates
    /// (`pinyin.cpp:599-601` at the pin writes `m_pinyin_table->add_index`,
    /// whose facade routes to the user table): tokens of pronunciations
    /// whose pinyin **strictly extends** `syllables`, under the pin's
    /// walk contract.
    ///
    /// The existence gate (`m_db->Get(query_key)`): `add_index` plants a
    /// marker row for every proper prefix of a stored pronunciation
    /// (`chewing_large_table2_kyotodb.cpp:311-327`), so "the query key
    /// exists" is checked even for foreign files missing a prefix marker,
    /// via the canonical index's `phrase_prefix_exists`. The rows after it:
    /// stored pinyins with **more syllables** whose first
    /// `syllables.len()` syllables match under
    /// `pinyin_compare_with_tones` (a complete query syllable equals the
    /// stored text; an incomplete one shares its initial) — a prefix
    /// variant of `stored_matches_query`. Order: the stored index's raw-key
    /// walk, with each row's stored token order. The runtime re-groups by library
    /// nibble the way `reduce_tokens` does.
    #[must_use]
    pub(crate) fn suggest_extensions(&self, syllables: &[SyllableKey]) -> Vec<u32> {
        let keys: Vec<_> = syllables.iter().map(|key| key.chewing_key()).collect();
        self.with_index(|index| index.suggest(&keys))
    }

    /// Tokens and raw UCS-4 keys extending the stored `prefix` key.
    ///
    /// Import creates an empty phrase-index key for every proper prefix
    /// (`074a2219 phrase_large_table3_bdb.cpp:297-315`, with KC/tkrzw twins).
    /// A prefix need not have a phrase token of its own. Rows are merged into
    /// the DBM walk order by the runtime before the stable prediction sort.
    #[must_use]
    pub(crate) fn suggest_after(&self, prefix: &[u32]) -> Vec<(u32, Vec<u8>)> {
        let prefix = crate::user::phrase_table::key_ucs4(prefix);
        let Ok(rows) = self.phrases.borrow().phrase_table.suggest_after(&prefix) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (key, tokens) in rows {
            // The pin compares raw UCS-4 bytes without Unicode validation
            // (074a2219 phrase_large_table3_bdb.cpp:249-252). Resolve display
            // text through the token's live item after ordering these rows.
            for token in tokens {
                out.push((token, key.clone()));
            }
        }
        out
    }
}

// `get_phrase_item` selects the library with `PHRASE_INDEX_LIBRARY_INDEX`
// and the slot with `PHRASE_MASK` (074a2219 phrase_index.h:647,
// phrase_index.cpp:186), so a token's high four bits do not take part.
fn session_text(
    phrases: &SessionPhrases,
    items: &RefCell<crate::user::system_items::SystemItems>,
    token: u32,
) -> Option<PhraseText> {
    if let Some(item) = phrases
        .libraries
        .get(&crate::phrase_index_library_index(token))
        .and_then(|library| library.item(token & crate::PHRASE_MASK))
    {
        return item.phrase_text();
    }
    // An item a system library's live buffer holds and its chunk lacks.
    items
        .borrow()
        .with_live(token, |bytes| {
            crate::data::phrase_library::PhraseItemView::from_bytes(bytes)?.phrase_text()
        })
        .flatten()
}

#[cfg(test)]
mod tests;
