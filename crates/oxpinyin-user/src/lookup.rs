//! In-memory reverse index over [`crate::UserStore`] phrases.
//!
//! Rebuilds from the store when the write generation changes. Lookup order
//! is ascending library nibble then token, matching
//! `_append_items` (`docs/findings/phrase-union.md` §3.3).

use std::collections::BTreeMap;

use oxpinyin_core::{Completeness, PhraseEntry, PhraseToken, SyllableKey, syllable_initial};

use crate::store::{UserStore, UserStoreError};

/// Default-facade lookup over user-file phrases (network nibble 6, user 7).
#[derive(Clone, Debug, Default)]
pub struct UserLookup {
    generation: u64,
    exact: BTreeMap<String, Vec<PhraseEntry>>,
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
    pub fn from_store(store: &UserStore) -> Result<Self, UserStoreError> {
        let generation = store.generation();
        let mut exact: BTreeMap<String, Vec<PhraseEntry>> = BTreeMap::new();
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
                exact
                    .entry(pinyin.clone())
                    .or_default()
                    .push(PhraseEntry::new(
                        PhraseToken::new(token),
                        phrase.text().to_owned(),
                    ));
                pinyin_keys.push(pinyin.clone());
                initial_keys.push(initial_of(&pinyin));
            }
        }

        for entries in exact.values_mut() {
            entries.sort_by_key(|entry| entry.token().value());
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
            pinyin_keys: pinyin_keys.into_boxed_slice(),
            initial_keys: initial_keys.into_boxed_slice(),
            text_tokens,
            token_text,
        })
    }

    /// Rebuilds `cache` when `store`'s write generation has moved.
    pub fn refresh_in(
        cache: &mut Option<(u64, Self)>,
        store: &UserStore,
    ) -> Result<(), UserStoreError> {
        let generation = store.generation();
        match cache {
            Some((seen, _)) if *seen == generation => Ok(()),
            _ => {
                let lookup = Self::from_store(store)?;
                *cache = Some((generation, lookup));
                Ok(())
            }
        }
    }

    /// Store generation this snapshot was built against.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Phrases whose stored pinyin is `syllables` joined by `'`.
    #[must_use]
    pub fn lookup(&self, syllables: &[SyllableKey]) -> Vec<PhraseEntry> {
        if syllables.is_empty() {
            return Vec::new();
        }
        self.exact
            .get(&index_key(syllables))
            .cloned()
            .unwrap_or_default()
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
        out.sort_by_key(|(token, _)| *token);
        out
    }
}

fn index_key(syllables: &[SyllableKey]) -> String {
    let mut key = String::new();
    for (position, syllable) in syllables.iter().enumerate() {
        if position > 0 {
            key.push('\'');
        }
        key.push_str(syllable.text());
    }
    key
}

fn initial_key(syllables: &[SyllableKey]) -> String {
    let mut key = String::new();
    for (position, syllable) in syllables.iter().enumerate() {
        if position > 0 {
            key.push('\'');
        }
        match syllable_initial(syllable.text()) {
            Some(initial) => key.push_str(initial),
            None => key.push('0'),
        }
    }
    key
}

fn initial_of(pinyin: &str) -> String {
    let mut initial = String::new();
    for (position, syllable) in pinyin.split('\'').enumerate() {
        if position > 0 {
            initial.push('\'');
        }
        match syllable_initial(syllable) {
            Some(prefix) => initial.push_str(prefix),
            None => initial.push('0'),
        }
    }
    initial
}

fn prefix_probe(sorted: &[String], joined: &str) -> bool {
    match sorted.binary_search_by(|candidate| candidate.as_str().cmp(joined)) {
        Ok(_) => true,
        Err(index) => {
            sorted
                .get(index)
                .is_some_and(|candidate| candidate.starts_with(joined))
                && sorted[index].as_bytes().get(joined.len()) == Some(&b'\'')
        }
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
        let ni = SyllableKey::from_text("ni").unwrap().index() as u16;
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
}
