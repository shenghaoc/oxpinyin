//! In-memory reverse index over [`crate::UserStore`] phrases.
//!
//! Rebuilds from the store when its *phrase* generation changes — a
//! phrase or pronunciation row added, imported, or removed. Count-only
//! training writes move the store's write generation but not this one,
//! so a keystroke that trains never rebuilds the index. Lookup order
//! is ascending library nibble then token, matching
//! `_append_items` (`docs/findings/phrase-union.md` §3.3).

use std::collections::BTreeMap;
use std::sync::Arc;

use oxpinyin_core::{PhraseEntry, PhraseText, PhraseToken, SyllableKey};

use crate::store::{UserStore, UserStoreError};

/// Default-facade lookup over user-file phrases (network nibble 6, user 7).
///
/// The lookup carries two parallel indices over the stored pronunciations,
/// mirroring upstream's `ChewingLargeTable2::add_index` at
/// `chewing_large_table2.cpp:184-197`, which writes each phrase into both
/// its incomplete-projected and its complete keyspaces so a query with an
/// incomplete syllable answers in one probe:
///
/// - `exact` — complete-index: full apostrophe-joined pinyin
///   text → the phrases stored under it, token-ascending.
/// - `by_initial` — incomplete-projected index: the
///   pinyin's initial-only projection (each syllable reduced to its
///   `syllable_initial`) → `(stored full pinyin, entry)` pairs, so
///   [`Self::lookup`] on a query whose keys mix complete and incomplete
///   syllables can filter the bucket by syllable equality without
///   enumerating completions in the scan.
#[derive(Clone, Debug, Default)]
pub struct UserLookup {
    generation: u64,
    index: crate::user_index::UserIndex,
    text_tokens: BTreeMap<Vec<u8>, Vec<u32>>,
    token_text: BTreeMap<u32, PhraseText>,
}

impl UserLookup {
    /// Empty lookup (zero user-file data).
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Builds a lookup from every stored user-file phrase.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn from_store(store: &UserStore) -> Result<Self, UserStoreError> {
        let generation = store.phrase_generation();
        // A session owns the loaded DBM table; the standalone store derives
        // the same records from its indexed readings.
        let index = match store.user_index_snapshot() {
            Some(index) => index,
            None => {
                let mut index = crate::user_index::UserIndex::new();
                for phrase in store.phrases()? {
                    for pronunciation in phrase
                        .pronunciations()
                        .iter()
                        .filter(|reading| reading.indexed())
                    {
                        let keys: Vec<_> = pronunciation
                            .keys()
                            .iter()
                            .filter_map(|&key| {
                                let syllable = oxpinyin_core::SyllableKey::from_index(
                                    crate::key_syllable(key),
                                )?;
                                let chewing =
                                    oxpinyin_core::ChewingKey::from_pinyin(syllable.text())?;
                                Some(chewing.with_tone(crate::key_tone(key)))
                            })
                            .collect();
                        if keys.is_empty() || keys.len() > crate::phrase::MAX_PHRASE_LENGTH {
                            continue;
                        }
                        index.add(&keys, phrase.token());
                    }
                }
                index
            }
        };
        let mut text_tokens: BTreeMap<Vec<u8>, Vec<u32>> = BTreeMap::new();
        for (key, value) in store.phrase_table_records()? {
            let entry =
                crate::phrase_table::PhraseTableEntry::from_bytes(value).unwrap_or_default();
            let mut tokens = Vec::new();
            entry.search(&mut tokens, |_| true);
            text_tokens.insert(key, tokens);
        }
        let mut token_text = BTreeMap::new();
        for phrase in store.phrases()? {
            if let Some(text) = phrase.phrase_text() {
                token_text.insert(phrase.token(), text.clone());
            }
        }
        Ok(Self {
            generation,
            index,
            text_tokens,
            token_text,
        })
    }

    /// Rebuilds `cache` when `store`'s phrase generation has moved.
    ///
    /// # Errors
    ///
    /// Returns [`UserStoreError`] when the store cannot be read.
    pub fn refresh_in(
        cache: &mut Option<(u64, Arc<Self>)>,
        store: &UserStore,
    ) -> Result<(), UserStoreError> {
        let generation = store.phrase_generation();
        if let Some((seen, _)) = cache.as_ref()
            && *seen == generation
        {
            return Ok(());
        }
        let lookup = Self::from_store(store)?;
        *cache = Some((generation, Arc::new(lookup)));
        Ok(())
    }

    /// Store phrase generation this snapshot was built against.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Phrases whose stored pinyin matches `syllables` under upstream's
    /// `pinyin_compare_with_tones`
    /// (`docs/findings/pinyin-dbm-format-2026-09-01.md`): equal length, and
    /// syllable-by-syllable, the stored pronunciation's syllable equals the
    /// query when the query syllable is complete, or shares its initial
    /// when the query syllable is incomplete.
    ///
    /// A query with any incomplete syllable is routed through the
    /// initial-projected index `by_initial`, the same "one
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
        self.index
            .search(keys)
            .into_iter()
            .map(|(token, _)| token)
            .collect()
    }

    fn entries_for(&self, keys: &[oxpinyin_core::ChewingKey]) -> Vec<PhraseEntry> {
        self.index
            .search(keys)
            .into_iter()
            .filter_map(|(token, _)| {
                let text = self.token_text.get(&token)?;
                Some(PhraseEntry::new(PhraseToken::new(token), text.clone()))
            })
            .collect()
    }

    /// Whether the user pinyin table holds a non-empty record under the key
    /// a search for `syllables` probes: the complete key, or the initials'
    /// when any syllable is incomplete.
    #[must_use]
    pub fn has_record(&self, syllables: &[SyllableKey]) -> bool {
        query_keys(syllables).is_some_and(|keys| self.index.has_record(&keys))
    }

    /// `SEARCH_CONTINUED` probe over user-file pinyin keys.
    #[must_use]
    pub fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> bool {
        query_keys(syllables).is_some_and(|keys| self.index.has_prefix(&keys))
    }

    /// Phrase text for `token`, if this snapshot holds it.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<&PhraseText> {
        self.token_text.get(&token)
    }

    /// Tokens whose stored UCS-4 key is exactly `words`, non-scalar words
    /// included.
    #[must_use]
    pub fn tokens_for_ucs4(&self, words: &[u32]) -> &[u32] {
        self.text_tokens
            .get(&crate::phrase_table::key_ucs4(words))
            .map_or(&[], Vec::as_slice)
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
    /// exists" is exactly "some stored pronunciation equals or extends
    /// it" — this lookup's own `phrase_prefix_exists`. The rows after it:
    /// stored pinyins with **more syllables** whose first
    /// `syllables.len()` syllables match under
    /// `pinyin_compare_with_tones` (a complete query syllable equals the
    /// stored text; an incomplete one shares its initial) — a prefix
    /// variant of `stored_matches_query`. Order: the stored map's
    /// pinyin-ascending walk (`exact` / `by_initial` are `BTreeMap`s),
    /// token-ascending within one pronunciation — the deterministic
    /// stand-in for the pin table's cursor order over the same
    /// pronunciation set; the runtime's composition re-groups by library
    /// nibble the way `reduce_tokens` does.
    #[must_use]
    pub fn suggest_extensions(&self, syllables: &[SyllableKey]) -> Vec<u32> {
        let Some(keys) = query_keys(syllables) else {
            return Vec::new();
        };
        self.index.suggest(&keys)
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
        if !self.text_tokens.contains_key(&prefix) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (key, tokens) in self.text_tokens.range(prefix.clone()..) {
            if !key.starts_with(&prefix) {
                break;
            }
            if key == &prefix {
                continue;
            }
            // The pin compares raw UCS-4 bytes without Unicode validation
            // (074a2219 phrase_large_table3_bdb.cpp:249-252). Resolve display
            // text through the token's live item after ordering these rows.
            for token in tokens {
                out.push((*token, key.clone()));
            }
        }
        out
    }
}

fn query_keys(syllables: &[SyllableKey]) -> Option<Vec<oxpinyin_core::ChewingKey>> {
    syllables
        .iter()
        .map(|syllable| oxpinyin_core::ChewingKey::from_pinyin(syllable.text()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phrase::{NETWORK_DICTIONARY, USER_DICTIONARY};
    use crate::store::UserStore;
    use oxpinyin_core::Completeness;

    /// A store path inside a fresh, uniquely named dir. Bind the dir before
    /// the store so it drops (and is removed) after the store's handle.
    fn temp_path(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::Builder::new()
            .prefix(&format!("oxpinyin-user-lookup-{tag}-"))
            .tempdir()
            .unwrap();
        let path = dir.path().join(format!("{tag}.store"));
        (dir, path)
    }

    fn key(text: &str) -> SyllableKey {
        SyllableKey::from_text(text).expect("frozen syllable")
    }

    #[test]
    fn lookup_surfaces_user_then_network_by_token() {
        let (_dir, path) = temp_path("order");
        let mut store = UserStore::open(&path).unwrap();
        let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        store
            .add_phrase_in(USER_DICTIONARY, &"你".into(), &[ni], Some(5))
            .unwrap();
        store
            .add_phrase_in(NETWORK_DICTIONARY, &"拟".into(), &[ni], Some(5))
            .unwrap();
        let lookup = UserLookup::from_store(&store).unwrap();
        let entries = lookup.lookup(&[key("ni")]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text(), "拟");
        assert_eq!(entries[1].text(), "你");
        assert!(lookup.phrase_prefix_exists(&[key("ni")]));
    }

    /// `lookup` with an incomplete syllable at the head of the query must
    /// find every user entry whose stored first-syllable initial matches
    /// the query, without callers enumerating completions — the
    /// [`by_initial`](UserLookup::by_initial) index mirrors upstream's
    /// incomplete keyspace at `chewing_large_table2.cpp:184-197`. Regresses
    /// shenghaoc/oxpinyin#403's fix.
    #[test]
    fn lookup_answers_a_single_syllable_incomplete_query_from_by_initial() {
        let (_dir, path) = temp_path("incomplete-single");
        let mut store = UserStore::open(&path).unwrap();
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
        let lookup = UserLookup::from_store(&store).unwrap();

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
        let (_dir, path) = temp_path("incomplete-mixed");
        let mut store = UserStore::open(&path).unwrap();
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
        let lookup = UserLookup::from_store(&store).unwrap();

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
