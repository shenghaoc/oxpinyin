//! Lookup over the store's own session tables and item arena.
//!
//! Session queries borrow the canonical tables for the duration of each
//! operation.

use std::sync::{Mutex, MutexGuard};

use oxpinyin_core::{PhraseEntry, PhraseText, PhraseToken, SyllableKey};

use crate::lock_result_or_abort;
use crate::user_index::UserIndex;
use crate::user_library::SessionPhrases;

/// A query handle over the user store. Session handles borrow the canonical
/// owner; they never hold a second index, phrase table or copy of item text.
/// Each method releases the phrase guard before returning to its caller.
#[derive(Clone, Debug)]
pub struct UserLookup<'a> {
    /// The session's arenas.
    phrases: &'a Mutex<SessionPhrases>,
    /// For system libraries with an original, the live buffers that hold
    /// their appended items.
    items: &'a Mutex<crate::system_items::SystemItems>,
}

impl<'a> UserLookup<'a> {
    pub(crate) fn session(
        phrases: &'a Mutex<SessionPhrases>,
        items: &'a Mutex<crate::system_items::SystemItems>,
    ) -> Self {
        Self { phrases, items }
    }

    fn with_index<R>(&self, read: impl FnOnce(&UserIndex) -> R) -> R {
        read(&lock_result_or_abort(self.phrases.lock()).index)
    }

    /// Phrases whose stored pinyin matches `syllables` under upstream's
    /// `pinyin_compare_with_tones`
    /// (`docs/findings/pinyin-dbm-format-2026-09-01.md`): equal length, and
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
        let Some(keys) = query_keys(syllables) else {
            return Vec::new();
        };
        self.entries_for(&keys)
    }

    /// The trellis's with-tones query over the same stored records.
    #[must_use]
    pub fn search_toned(&self, keys: &[oxpinyin_core::ChewingKey]) -> Vec<u32> {
        self.with_index(|index| index.search(keys))
    }

    fn entries_for(&self, keys: &[oxpinyin_core::ChewingKey]) -> Vec<PhraseEntry> {
        let phrases = lock_result_or_abort(self.phrases.lock());
        phrases
            .index
            .search(keys)
            .into_iter()
            .filter_map(|token| {
                let text = session_text(&phrases, self.items, token)?;
                Some(PhraseEntry::new(PhraseToken::new(token), text))
            })
            .collect()
    }

    /// `SEARCH_CONTINUED` probe over user-file pinyin keys.
    #[must_use]
    pub fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> bool {
        query_keys(syllables).is_some_and(|keys| self.with_index(|index| index.has_prefix(&keys)))
    }

    /// Phrase text from the current item, without decoding its readings.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<PhraseText> {
        session_text(
            &lock_result_or_abort::<MutexGuard<'_, SessionPhrases>>(self.phrases.lock()),
            self.items,
            token,
        )
    }

    /// Tokens whose stored UCS-4 key is exactly `words`, non-scalar words
    /// included.
    #[must_use]
    pub fn tokens_for_ucs4(&self, words: &[u32]) -> Vec<u32> {
        lock_result_or_abort(self.phrases.lock())
            .phrase_table
            .tokens(&crate::phrase_table::key_ucs4(words))
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
    pub fn suggest_extensions(&self, syllables: &[SyllableKey]) -> Vec<u32> {
        let Some(keys) = query_keys(syllables) else {
            return Vec::new();
        };
        self.with_index(|index| index.suggest(&keys))
    }

    /// Tokens and raw UCS-4 keys extending the stored `prefix` key.
    ///
    /// Import creates an empty phrase-index key for every proper prefix
    /// (`074a2219 phrase_large_table3_bdb.cpp:297-315`, with KC/tkrzw twins).
    /// A prefix need not have a phrase token of its own. Rows are merged into
    /// the DBM walk order by the runtime before the stable prediction sort.
    #[must_use]
    pub fn suggest_after(&self, prefix: &[u32]) -> Vec<(u32, Vec<u8>)> {
        let prefix = crate::phrase_table::key_ucs4(prefix);
        let Ok(rows) = lock_result_or_abort(self.phrases.lock())
            .phrase_table
            .suggest_after(&prefix)
        else {
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

// The old text map used the full token emitted by the library walk. Do not
// alias a foreign index token's high bits to an otherwise valid arena item.
fn session_text(
    phrases: &SessionPhrases,
    items: &Mutex<crate::system_items::SystemItems>,
    token: u32,
) -> Option<PhraseText> {
    if let Some(item) = phrases
        .libraries
        .get(&((token >> 24) as u8))
        .and_then(|library| library.item(token & crate::PHRASE_MASK))
    {
        return item.phrase_text();
    }
    // An item a system library's live buffer holds and its chunk lacks.
    lock_result_or_abort(items.lock())
        .with_live(token, |bytes| {
            oxpinyin_data::phrase_library::PhraseItemView::from_bytes(bytes)?.phrase_text()
        })
        .flatten()
}

fn query_keys(syllables: &[SyllableKey]) -> Option<Vec<oxpinyin_core::ChewingKey>> {
    Some(
        syllables
            .iter()
            .map(|syllable| syllable.chewing_key())
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phrase::{NETWORK_DICTIONARY, USER_DICTIONARY};
    use crate::store::UserStore;
    use oxpinyin_core::Completeness;

    fn key(text: &str) -> SyllableKey {
        SyllableKey::from_text(text).expect("frozen syllable")
    }

    /// The pin's NULL-user-directory session over the stock layout: the
    /// user and network libraries, and no system library.
    fn transient() -> UserStore {
        UserStore::open_transient(
            crate::persistence::SystemOriginals::new(
                Default::default(),
                oxpinyin_data::user_files::UserFileLayout::stock(),
            ),
            oxpinyin_data::user_files::SystemVersions::for_this_build(7, 14),
            crate::persistence::UserConfLaw::Pinyin,
        )
        .unwrap()
    }

    #[test]
    fn session_lookup_keeps_full_token_identity() {
        let mut phrases = SessionPhrases::default();
        let token = crate::FIRST_USER_TOKEN;
        let bytes = oxpinyin_data::chunk_write::encode_phrase_item(
            &oxpinyin_data::chunk_write::ChunkItem {
                phrase: vec![u32::from('你')],
                unigram: 1,
                prons: Vec::new(),
            },
        )
        .unwrap();
        phrases
            .libraries
            .entry(USER_DICTIONARY)
            .or_default()
            .replace(token & crate::PHRASE_MASK, &bytes)
            .unwrap();
        let keys = [oxpinyin_core::ChewingKey::from_pinyin("ni").unwrap()];
        phrases.index.add(&keys, token);
        phrases.index.add(&keys, token | 0x8000_0000);
        let phrases = Mutex::new(phrases);
        let items = Mutex::new(crate::system_items::SystemItems::default());
        let lookup = UserLookup::session(&phrases, &items);
        assert_eq!(lookup.search_toned(&keys), [token, token | 0x8000_0000]);
        let entries = lookup.lookup(&[key("ni")]);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].token().value(), token);
        assert_eq!(entries[0].text(), "你");
        assert!(lookup.phrase_text(token | 0x8000_0000).is_none());
    }

    #[test]
    fn lookup_surfaces_user_then_network_by_token() {
        let mut store = transient();
        let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        store
            .add_phrase_in(USER_DICTIONARY, &"你".into(), &[ni], Some(5))
            .unwrap();
        store
            .add_phrase_in(NETWORK_DICTIONARY, &"拟".into(), &[ni], Some(5))
            .unwrap();
        let lookup = store.lookup();
        let entries = lookup.lookup(&[key("ni")]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text(), "拟");
        assert_eq!(entries[1].text(), "你");
        assert!(lookup.phrase_prefix_exists(&[key("ni")]));
    }

    /// `_add_phrase` takes `ChewingKey`s (`pinyin.cpp:514-519`), each of
    /// which names a syllable; a dense id that names none has no packed
    /// form, so the session refuses the phrase before it stores or indexes
    /// anything, and the readings that do convert index as before.
    #[test]
    fn a_reading_with_an_unconvertible_syllable_is_refused_before_it_is_stored() {
        let mut store = transient();
        let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let hao = u16::try_from(SyllableKey::from_text("hao").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let unknown = crate::phrase::KEY_SYLLABLE_MASK;
        assert!(SyllableKey::from_index(usize::from(unknown)).is_none());
        assert!(matches!(
            store.add_phrase_in(USER_DICTIONARY, &"你好".into(), &[ni, unknown], Some(5)),
            Err(crate::UserStoreError::InvalidPhrase)
        ));
        store
            .add_phrase_in(USER_DICTIONARY, &"好好".into(), &[hao, hao], Some(5))
            .unwrap();
        let lookup = store.lookup();
        assert!(lookup.lookup(&[key("ni")]).is_empty());
        assert!(!lookup.phrase_prefix_exists(&[key("ni")]));
        assert_eq!(lookup.lookup(&[key("hao"), key("hao")]).len(), 1);
        assert_eq!(store.phrases().unwrap().len(), 1);
    }

    /// `lookup` with an incomplete syllable at the head of the query must
    /// find every user entry whose stored first-syllable initial matches
    /// the query, without callers enumerating completions — the
    /// incomplete-projected index mirrors upstream's
    /// incomplete keyspace at `chewing_large_table2.cpp:184-197`. Regresses
    /// shenghaoc/oxpinyin#403's fix.
    #[test]
    fn lookup_answers_a_single_syllable_incomplete_query_from_by_initial() {
        let mut store = transient();
        let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let na = u16::try_from(SyllableKey::from_text("na").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let hao = u16::try_from(SyllableKey::from_text("hao").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        store
            .add_phrase_in(USER_DICTIONARY, &"你".into(), &[ni], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, &"那".into(), &[na], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, &"号".into(), &[hao], Some(5))
            .unwrap();
        let lookup = store.lookup();

        let n_partial =
            SyllableKey::from_text("n").expect("initial-only 'n' is in the incomplete inventory");
        assert_eq!(
            n_partial.completeness(),
            Completeness::Partial,
            "'n' resolves to the initial-only key"
        );
        let entries = lookup.lookup(&[n_partial]);
        let texts: Vec<&str> = entries
            .iter()
            .filter_map(oxpinyin_core::PhraseEntry::text_str)
            .collect();
        assert!(texts.contains(&"你"), "expected 你 in {texts:?}");
        assert!(texts.contains(&"那"), "expected 那 in {texts:?}");
        assert!(
            !texts.contains(&"号"),
            "hao does not start with n: {texts:?}"
        );
    }

    /// A two-syllable query where the first key is incomplete and the
    /// second is complete must still filter by the second syllable
    /// (`keys_match`'s complete-only rule at
    /// `crates/oxpinyin-data/src/chewing_table.rs:431-451`). Regresses
    /// shenghaoc/oxpinyin#403's fix.
    #[test]
    fn lookup_filters_mixed_incomplete_and_complete_syllables() {
        let mut store = transient();
        let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let na = u16::try_from(SyllableKey::from_text("na").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let hao = u16::try_from(SyllableKey::from_text("hao").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let li = u16::try_from(SyllableKey::from_text("li").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        // Two matching phrases: (ni, hao) and (na, hao); one non-matching
        // (li, hao) — starts with 'l' not 'n'.
        store
            .add_phrase_in(USER_DICTIONARY, &"你好".into(), &[ni, hao], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, &"那号".into(), &[na, hao], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, &"礼号".into(), &[li, hao], Some(5))
            .unwrap();
        let lookup = store.lookup();

        let n_partial =
            SyllableKey::from_text("n").expect("initial-only 'n' is in the incomplete inventory");
        assert_eq!(
            n_partial.completeness(),
            Completeness::Partial,
            "'n' resolves to the initial-only key"
        );
        let entries = lookup.lookup(&[n_partial, key("hao")]);
        let texts: Vec<&str> = entries
            .iter()
            .filter_map(oxpinyin_core::PhraseEntry::text_str)
            .collect();
        assert!(texts.contains(&"你好"), "expected 你好 in {texts:?}");
        assert!(texts.contains(&"那号"), "expected 那号 in {texts:?}");
        assert!(
            !texts.contains(&"礼号"),
            "礼 starts with l, must not match n-partial: {texts:?}"
        );
    }
}
