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

use oxpinyin_core::{Completeness, PhraseEntry, PhraseToken, SyllableKey, syllable_initial};

use crate::store::{UserStore, UserStoreError};

/// Default-facade lookup over user-file phrases (network nibble 6, user 7).
///
/// The lookup carries two parallel indices over the stored pronunciations,
/// mirroring upstream's `ChewingLargeTable2::add_index` at
/// `chewing_large_table2.cpp:184-197`, which writes each phrase into both
/// its incomplete-projected and its complete keyspaces so a query with an
/// incomplete syllable answers in one probe:
///
/// - [`exact`](Self::exact) — complete-index: full apostrophe-joined pinyin
///   text → the phrases stored under it, token-ascending.
/// - [`by_initial`](Self::by_initial) — incomplete-projected index: the
///   pinyin's initial-only projection (each syllable reduced to its
///   `syllable_initial`) → `(stored full pinyin, entry)` pairs, so
///   [`Self::lookup`] on a query whose keys mix complete and incomplete
///   syllables can filter the bucket by syllable equality without
///   enumerating completions in the scan.
#[derive(Clone, Debug, Default)]
pub struct UserLookup {
    generation: u64,
    exact: BTreeMap<String, Vec<PhraseEntry>>,
    by_initial: BTreeMap<String, Vec<(String, PhraseEntry)>>,
    pinyin_keys: Box<[String]>,
    initial_keys: Box<[String]>,
    text_tokens: BTreeMap<String, Vec<u32>>,
    token_text: BTreeMap<u32, String>,
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
        let mut exact: BTreeMap<String, Vec<PhraseEntry>> = BTreeMap::new();
        let mut by_initial: BTreeMap<String, Vec<(String, PhraseEntry)>> = BTreeMap::new();
        let mut text_tokens: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        let mut token_text: BTreeMap<u32, String> = BTreeMap::new();
        let mut pinyin_keys: Vec<String> = Vec::new();
        let mut initial_keys: Vec<String> = Vec::new();

        for phrase in store.phrases()? {
            let token = phrase.token();
            token_text.insert(token, phrase.text().to_owned());
            text_tokens
                .entry(phrase.text().to_owned())
                .or_default()
                .push(token);
            for pronunciation in phrase.pronunciations() {
                let Some(pinyin) = pronunciation.render_pinyin() else {
                    continue;
                };
                let entry = PhraseEntry::new(PhraseToken::new(token), phrase.text().to_owned());
                exact.entry(pinyin.clone()).or_default().push(entry.clone());
                by_initial
                    .entry(initial_of(&pinyin))
                    .or_default()
                    .push((pinyin.clone(), entry));
                pinyin_keys.push(pinyin.clone());
                initial_keys.push(initial_of(&pinyin));
            }
        }

        for entries in exact.values_mut() {
            entries.sort_by_key(|entry| entry.token().value());
        }
        for bucket in by_initial.values_mut() {
            // Order matches `exact`'s per-bucket sort: token ascending.
            // Ties on token break on the stored pinyin, so a store with
            // two pronunciations of one phrase keeps a deterministic
            // order.
            bucket.sort_by(|left, right| {
                left.1
                    .token()
                    .value()
                    .cmp(&right.1.token().value())
                    .then_with(|| left.0.cmp(&right.0))
            });
        }
        for tokens in text_tokens.values_mut() {
            tokens.sort_unstable();
        }
        pinyin_keys.sort_unstable();
        pinyin_keys.dedup();
        initial_keys.sort_unstable();
        initial_keys.dedup();

        Ok(Self {
            generation,
            exact,
            by_initial,
            pinyin_keys: pinyin_keys.into_boxed_slice(),
            initial_keys: initial_keys.into_boxed_slice(),
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
    /// initial-projected index [`Self::by_initial`], the same "one
    /// incomplete-index probe per key path" upstream's
    /// `ChewingLargeTable2::search` (`chewing_large_table2.cpp:161-172` at
    /// pin `074a2219`) uses — so the scan never needs to enumerate the
    /// key's completions to find matching user entries.
    #[must_use]
    pub fn lookup(&self, syllables: &[SyllableKey]) -> Vec<PhraseEntry> {
        if syllables.is_empty() {
            return Vec::new();
        }
        let has_incomplete = syllables
            .iter()
            .any(|key| key.completeness() == Completeness::Partial);
        if !has_incomplete {
            return self
                .exact
                .get(&index_key(syllables))
                .cloned()
                .unwrap_or_default();
        }
        let initial = initial_key(syllables);
        let Some(bucket) = self.by_initial.get(&initial) else {
            return Vec::new();
        };
        bucket
            .iter()
            .filter(|(pinyin, _)| stored_matches_query(pinyin, syllables))
            .map(|(_, entry)| entry.clone())
            .collect()
    }

    /// `SEARCH_CONTINUED` probe over user-file pinyin keys.
    #[must_use]
    pub fn phrase_prefix_exists(&self, syllables: &[SyllableKey]) -> bool {
        if syllables.is_empty() {
            return true;
        }
        if syllables
            .iter()
            .any(|key| key.completeness() == Completeness::Partial)
        {
            prefix_probe(&self.initial_keys, &initial_key(syllables))
        } else {
            prefix_probe(&self.pinyin_keys, &index_key(syllables))
        }
    }

    /// Phrase text for `token`, if this snapshot holds it.
    #[must_use]
    pub fn phrase_text(&self, token: u32) -> Option<&str> {
        self.token_text.get(&token).map(String::as_str)
    }

    /// Tokens whose phrase text is exactly `text`.
    #[must_use]
    pub fn tokens_for_text(&self, text: &str) -> &[u32] {
        self.text_tokens.get(text).map_or(&[], Vec::as_slice)
    }

    /// Tokens whose phrase text starts with `prefix` and is longer, when
    /// `prefix` itself is a stored phrase.
    ///
    /// Rows come out in the DEFINED prediction order — the reverse map's
    /// text-ascending walk, token-ascending within one text — the same
    /// order the system seam yields (`SystemDictionary::suggest_after`),
    /// so populated user stores cannot reorder the tie groups
    /// (`upstream-divergences.md`, "Predicted-candidate tie order").
    #[must_use]
    pub fn suggest_after(&self, prefix: &str) -> Vec<(u32, String)> {
        if prefix.is_empty() || !self.text_tokens.contains_key(prefix) {
            return Vec::new();
        }
        let mut out = Vec::new();
        for (text, tokens) in self.text_tokens.range(prefix.to_owned()..) {
            if !text.starts_with(prefix) {
                break;
            }
            if text == prefix {
                continue;
            }
            for token in tokens {
                out.push((*token, text.clone()));
            }
        }
        out
    }
}

fn index_key(syllables: &[SyllableKey]) -> String {
    join_with_apostrophe(syllables.iter().map(|syllable| syllable.text()))
}

/// `pinyin_compare_with_tones` (`pinyin_phrase3.h:68-115`) syllable-by-syllable
/// against a stored apostrophe-joined pinyin: same length; every complete
/// query syllable equals the stored syllable text; every incomplete query
/// syllable's initial equals the stored syllable's `syllable_initial`.
///
/// This is the filter the incomplete-space bucket needs, mirroring the
/// `keys_match` post-filter that `crates/oxpinyin-data/src/chewing_table.rs:342-356`
/// applies to `ChewingTable::search`'s returned records.
fn stored_matches_query(stored_pinyin: &str, query: &[SyllableKey]) -> bool {
    let mut parts = stored_pinyin.split('\'');
    for query_key in query {
        let Some(stored) = parts.next() else {
            return false;
        };
        let matches = if query_key.completeness() == Completeness::Complete {
            stored == query_key.text()
        } else {
            syllable_initial(stored) == Some(query_key.text())
        };
        if !matches {
            return false;
        }
    }
    parts.next().is_none()
}

fn initial_key(syllables: &[SyllableKey]) -> String {
    join_with_apostrophe(
        syllables
            .iter()
            .map(|syllable| syllable_initial(syllable.text()).unwrap_or("0")),
    )
}

fn initial_of(pinyin: &str) -> String {
    join_with_apostrophe(
        pinyin
            .split('\'')
            .map(|syllable| syllable_initial(syllable).unwrap_or("0")),
    )
}

fn join_with_apostrophe<I, S>(parts: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let mut parts = parts.into_iter();
    let Some(first) = parts.next() else {
        return String::new();
    };
    let mut joined = String::from(first.as_ref());
    for part in parts {
        joined.push('\'');
        joined.push_str(part.as_ref());
    }
    joined
}

fn prefix_probe(sorted: &[String], joined: &str) -> bool {
    match sorted.binary_search_by(|candidate| candidate.as_str().cmp(joined)) {
        Ok(_) => true,
        Err(index) => sorted.get(index).is_some_and(|candidate| {
            candidate.starts_with(joined) && candidate.as_bytes().get(joined.len()) == Some(&b'\'')
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phrase::{NETWORK_DICTIONARY, USER_DICTIONARY};
    use crate::store::UserStore;

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "oxpinyin-user-lookup-{tag}-{}.redb",
            std::process::id()
        ))
    }

    fn key(text: &str) -> SyllableKey {
        SyllableKey::from_text(text).expect("frozen syllable")
    }

    #[test]
    fn lookup_surfaces_user_then_network_by_token() {
        let path = temp_path("order");
        let mut store = UserStore::open(&path).unwrap();
        let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        store
            .add_phrase_in(USER_DICTIONARY, "你", &[ni], Some(5))
            .unwrap();
        store
            .add_phrase_in(NETWORK_DICTIONARY, "拟", &[ni], Some(5))
            .unwrap();
        let lookup = UserLookup::from_store(&store).unwrap();
        let entries = lookup.lookup(&[key("ni")]);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].text(), "拟");
        assert_eq!(entries[1].text(), "你");
        assert!(lookup.phrase_prefix_exists(&[key("ni")]));
        let _ = std::fs::remove_file(&path);
    }

    /// `lookup` with an incomplete syllable at the head of the query must
    /// find every user entry whose stored first-syllable initial matches
    /// the query, without callers enumerating completions — the
    /// [`by_initial`](UserLookup::by_initial) index mirrors upstream's
    /// incomplete keyspace at `chewing_large_table2.cpp:184-197`. Regresses
    /// shenghaoc/oxpinyin#403's fix.
    #[test]
    fn lookup_answers_a_single_syllable_incomplete_query_from_by_initial() {
        let path = temp_path("incomplete-single");
        let mut store = UserStore::open(&path).unwrap();
        let ni = u16::try_from(SyllableKey::from_text("ni").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let na = u16::try_from(SyllableKey::from_text("na").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        let hao = u16::try_from(SyllableKey::from_text("hao").unwrap().index())
            .expect("frozen syllable inventory fits u16");
        store
            .add_phrase_in(USER_DICTIONARY, "你", &[ni], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, "那", &[na], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, "号", &[hao], Some(5))
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
        let texts: Vec<&str> = entries.iter().map(|entry| entry.text()).collect();
        assert!(texts.contains(&"你"), "expected 你 in {texts:?}");
        assert!(texts.contains(&"那"), "expected 那 in {texts:?}");
        assert!(
            !texts.contains(&"号"),
            "hao does not start with n: {texts:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    /// A two-syllable query where the first key is incomplete and the
    /// second is complete must still filter by the second syllable
    /// (`keys_match`'s complete-only rule at
    /// `crates/oxpinyin-data/src/chewing_table.rs:431-451`). Regresses
    /// shenghaoc/oxpinyin#403's fix.
    #[test]
    fn lookup_filters_mixed_incomplete_and_complete_syllables() {
        let path = temp_path("incomplete-mixed");
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
            .add_phrase_in(USER_DICTIONARY, "你好", &[ni, hao], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, "那号", &[na, hao], Some(5))
            .unwrap();
        store
            .add_phrase_in(USER_DICTIONARY, "礼号", &[li, hao], Some(5))
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
        let texts: Vec<&str> = entries.iter().map(|entry| entry.text()).collect();
        assert!(texts.contains(&"你好"), "expected 你好 in {texts:?}");
        assert!(texts.contains(&"那号"), "expected 那号 in {texts:?}");
        assert!(
            !texts.contains(&"礼号"),
            "礼 starts with l, must not match n-partial: {texts:?}"
        );
        let _ = std::fs::remove_file(&path);
    }
}
