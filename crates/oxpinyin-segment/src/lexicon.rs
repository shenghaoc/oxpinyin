//! Character-domain phrase table built from `phrase_index.redb`.
//!
//! `ngseg` classifies a run as segmentable by
//! `FacadePhraseTable3::search(1, char, tokens) & SEARCH_OK`
//! (`utils/segment/ngseg.cpp:204-210`). The Rust export has no character
//! trie; the same answer is an exact lookup of the one-character phrase
//! text in the inverted `phrase_index.redb` map. `SEARCH_CONTINUED` is
//! "a stored phrase is a strict extension of this span".

use std::collections::{HashMap, HashSet};
use std::path::Path;

use oxpinyin_data::LookupTable;

use crate::error::SegmentError;

/// Longest phrase the pin stores, in characters (`novel_types.h:119`).
pub const MAX_PHRASE_LENGTH: usize = 16;

/// Phrase table over Han text: exact span → tokens, plus a prefix probe.
#[derive(Clone, Debug, Default)]
pub struct PhraseLexicon {
    exact: HashMap<String, Vec<u32>>,
    continued: HashSet<String>,
    texts: HashMap<u32, String>,
}

impl PhraseLexicon {
    /// Builds the inverted index from `phrase_index.redb` (`token → UTF-8`).
    ///
    /// # Errors
    ///
    /// Returns [`SegmentError`] when the table cannot be opened or a value
    /// is not UTF-8.
    pub fn from_phrase_index(path: &Path) -> Result<Self, SegmentError> {
        let table = LookupTable::open(path)?;
        let mut pairs = Vec::new();
        for (key, value) in table.iter() {
            if key.len() != 4 {
                return Err(SegmentError::Config(format!(
                    "phrase_index key length {} is not 4",
                    key.len()
                )));
            }
            let token = u32::from_le_bytes([key[0], key[1], key[2], key[3]]);
            let text = std::str::from_utf8(value).map_err(|_| {
                SegmentError::Config(format!("phrase text for token {token} is not UTF-8"))
            })?;
            pairs.push((token, text.to_owned()));
        }
        Ok(Self::from_pairs(pairs))
    }

    /// Builds a lexicon from `(token, phrase text)` pairs.
    ///
    /// Tokens that share a text are all kept, sorted by token so expansion
    /// order matches `search_unigram2`'s library-then-index walk
    /// (`phrase_lookup.cpp:237-249`).
    #[must_use]
    pub fn from_pairs(pairs: Vec<(u32, String)>) -> Self {
        let mut exact: HashMap<String, Vec<u32>> = HashMap::new();
        let mut continued = HashSet::new();
        let mut texts = HashMap::new();

        for (token, text) in pairs {
            if text.is_empty() {
                continue;
            }
            let chars: Vec<char> = text.chars().collect();
            for length in 1..chars.len() {
                continued.insert(chars[..length].iter().collect());
            }
            exact.entry(text.clone()).or_default().push(token);
            texts.insert(token, text);
        }

        for tokens in exact.values_mut() {
            tokens.sort_unstable();
            tokens.dedup();
        }

        Self {
            exact,
            continued,
            texts,
        }
    }

    /// Whether a single character is a dictionary phrase (`SEARCH_OK` at
    /// length 1). That is `ngseg`'s segmentable test.
    #[must_use]
    pub fn is_char_segmentable(&self, character: char) -> bool {
        let mut buffer = [0_u8; 4];
        self.exact.contains_key(character.encode_utf8(&mut buffer))
    }

    /// Phrase-table search of an exact character span.
    ///
    /// Returns `(SEARCH_OK, SEARCH_CONTINUED, tokens)`.
    #[must_use]
    pub fn search(&self, span: &str) -> (bool, bool, &[u32]) {
        let tokens = self.exact.get(span).map_or(&[] as &[u32], Vec::as_slice);
        (!tokens.is_empty(), self.continued.contains(span), tokens)
    }

    /// Stored phrase text for `token`.
    #[must_use]
    pub fn text(&self, token: u32) -> Option<&str> {
        self.texts.get(&token).map(String::as_str)
    }

    /// Every token the lexicon knows, sorted. Used to install the
    /// `gen_unigram` freq-1 floor over the loaded system libraries.
    #[must_use]
    pub fn tokens(&self) -> Vec<u32> {
        let mut tokens: Vec<u32> = self.texts.keys().copied().collect();
        tokens.sort_unstable();
        tokens
    }

    /// Number of distinct phrase texts.
    #[must_use]
    pub fn phrase_count(&self) -> usize {
        self.exact.len()
    }
}

#[cfg(test)]
mod tests {
    use super::PhraseLexicon;

    #[test]
    fn one_character_is_segmentable_only_when_stored() {
        let lexicon = PhraseLexicon::from_pairs(vec![
            (10, "中".to_owned()),
            (11, "中国".to_owned()),
            (12, "国".to_owned()),
        ]);
        assert!(lexicon.is_char_segmentable('中'));
        assert!(lexicon.is_char_segmentable('国'));
        assert!(!lexicon.is_char_segmentable('a'));
        assert!(!lexicon.is_char_segmentable('好'));
    }

    #[test]
    fn search_reports_exact_and_continued() {
        let lexicon = PhraseLexicon::from_pairs(vec![
            (10, "中".to_owned()),
            (11, "中国".to_owned()),
            (12, "国".to_owned()),
        ]);
        let (ok, continued, tokens) = lexicon.search("中");
        assert!(ok);
        assert!(continued);
        assert_eq!(tokens, &[10]);

        let (ok, continued, tokens) = lexicon.search("中国");
        assert!(ok);
        assert!(!continued);
        assert_eq!(tokens, &[11]);

        let (ok, continued, _) = lexicon.search("中华");
        assert!(!ok);
        assert!(!continued);
    }

    #[test]
    fn homographs_are_all_kept_and_sorted() {
        let lexicon = PhraseLexicon::from_pairs(vec![
            (30, "行".to_owned()),
            (10, "行".to_owned()),
            (20, "行".to_owned()),
        ]);
        let (ok, _, tokens) = lexicon.search("行");
        assert!(ok);
        assert_eq!(tokens, &[10, 20, 30]);
    }
}
