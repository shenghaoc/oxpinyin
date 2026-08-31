//! Correction-rate evaluation: decode each evaluation sentence from its
//! tokens' pinyin and count how many decode back to the original
//! (`utils/training/eval_correction_rate.cpp:34-215`).
//!
//! For each sentence `eval_correction_rate` picks each token's
//! highest-frequency pronunciation (`get_possible_pinyin`), lays the keys
//! out one-per-cell, and runs `PhoneticLookup<1,1>::get_nbest_match` with
//! the prefix `sentence_start` to get the single best token sequence, then
//! compares its UTF-8 to the source sentence. The correction rate is
//! passed / tested.
//!
//! Because the keys are a clean chain of complete syllables (one per token
//! syllable), the decode is the sentence Viterbi over that chain — exactly
//! [`oxpinyin_engine`'s `collect_sentences_with_tokens`], reproduced here
//! over the public [`Scorer::rank_phrases`], seeded with `sentence_start` so
//! the first phrase is scored against it as upstream's prefix is.

use oxpinyin_core::graph::EdgeKind;
use oxpinyin_core::scoring::{Scorer, ScoringConfig};
use oxpinyin_core::{Cost, Dictionary, LanguageModel, PhraseEntry, PhraseToken, SyllableKey};

use crate::error::EvalError;

/// `sentence_start` (`novel_types.h:122`).
pub const SENTENCE_START: u32 = 1;
/// `null_token` (`novel_types.h:121`).
pub const NULL_TOKEN: u32 = 0;
/// Longest phrase the decoder considers, in keys (`MAX_PHRASE_LENGTH`).
const MAX_PHRASE_KEYS: usize = 16;

/// Supplies each token's best pronunciation and its text — the
/// `get_possible_pinyin` and `convert_to_utf8` inputs.
pub trait PhraseSource {
    /// The highest-frequency pronunciation of `token` as syllable keys, or
    /// `None` when the token has no pronunciation.
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>>;

    /// The phrase text of `token`, or `None` when it is unknown.
    fn text(&self, token: PhraseToken) -> Option<String>;
}

/// The evaluation result.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EvalReport {
    /// Sentences tested (`tested_count`).
    pub tested: usize,
    /// Sentences whose decode matched the source (`passed_count`).
    pub passed: usize,
    /// `passed / tested` (0 when nothing was tested).
    pub rate: f64,
    /// The `(expected, decoded)` text of each failed sentence.
    pub mismatches: Vec<(String, String)>,
}

impl EvalReport {
    /// The `correction rate:%f` line `evaluate.py` parses.
    #[must_use]
    pub fn correction_rate_line(&self) -> String {
        format!("correction rate:{:.6}", self.rate)
    }
}

/// Parses an evaluation corpus (segmented token lines, `null_token`
/// separated) into sentences (`eval_correction_rate.cpp:175-205`).
///
/// # Errors
///
/// Returns [`EvalError::Malformed`] when a non-empty line is not a
/// `token …` record.
pub fn parse_eval_corpus(text: &str) -> Result<Vec<Vec<PhraseToken>>, EvalError> {
    let mut sentences = Vec::new();
    let mut current = Vec::new();
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let token = parse_token(line)?;
        if token == NULL_TOKEN {
            if !current.is_empty() {
                sentences.push(std::mem::take(&mut current));
            }
        } else {
            current.push(PhraseToken::new(token));
        }
    }
    if !current.is_empty() {
        sentences.push(current);
    }
    Ok(sentences)
}

fn parse_token(line: &str) -> Result<u32, EvalError> {
    let head = line.split([' ', '\t']).next().unwrap_or(line);
    head.parse::<u32>().map_err(|_| EvalError::Malformed {
        detail: format!("token field {head:?} is not an integer"),
    })
}

/// Evaluates the correction rate over `sentences`, decoding each with the
/// dictionary and language model.
///
/// # Errors
///
/// Returns [`EvalError`] when a token has no pronunciation
/// (`get_possible_pinyin` would abort) or a decode backend fails.
pub fn correction_rate<D, L, P>(
    dictionary: &D,
    model: &L,
    phrases: &P,
    sentences: &[Vec<PhraseToken>],
) -> Result<EvalReport, EvalError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
    P: PhraseSource,
{
    let scorer = Scorer::new(ScoringConfig::default(), dictionary, model).map_err(|error| {
        EvalError::Backend {
            detail: error.to_string(),
        }
    })?;

    let mut tested = 0;
    let mut passed = 0;
    let mut mismatches = Vec::new();

    for sentence in sentences {
        if sentence.is_empty() {
            continue;
        }
        let keys = sentence_keys(phrases, sentence)?;
        let expected = sentence_text(phrases, sentence);
        let decoded = decode(&scorer, &keys)?;

        tested += 1;
        if decoded == expected {
            passed += 1;
        } else {
            mismatches.push((expected, decoded));
        }
    }

    let rate = if tested == 0 {
        0.0
    } else {
        passed as f64 / tested as f64
    };
    Ok(EvalReport {
        tested,
        passed,
        rate,
        mismatches,
    })
}

/// Each token's best-pronunciation keys, concatenated (`get_possible_pinyin`).
fn sentence_keys<P: PhraseSource>(
    phrases: &P,
    sentence: &[PhraseToken],
) -> Result<Vec<SyllableKey>, EvalError> {
    let mut keys = Vec::new();
    for &token in sentence {
        let token_keys = phrases.best_keys(token).ok_or(EvalError::NoPronunciation {
            token: token.value(),
        })?;
        keys.extend(token_keys);
    }
    Ok(keys)
}

/// The source sentence text (`convert_to_utf8`).
fn sentence_text<P: PhraseSource>(phrases: &P, sentence: &[PhraseToken]) -> String {
    sentence
        .iter()
        .filter_map(|&token| phrases.text(token))
        .collect()
}

/// Decode a clean complete-syllable key chain to its best phrase cover,
/// seeded with `sentence_start`, returning the decoded UTF-8
/// (`get_nbest_match` + `convert_to_utf8`).
fn decode<D, L>(scorer: &Scorer<'_, D, L>, keys: &[SyllableKey]) -> Result<String, EvalError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    if keys.is_empty() {
        return Ok(String::new());
    }
    let kinds = vec![EdgeKind::Exact; keys.len()];

    // best[i] = cheapest way to spell keys[..i]: (cost, text, history). The
    // history is seeded with sentence_start so the first phrase is scored
    // against it (the get_nbest_match prefix).
    let mut best: Vec<Option<(Cost, String, Vec<PhraseToken>)>> = vec![None; keys.len() + 1];
    best[0] = Some((0, String::new(), vec![PhraseToken::new(SENTENCE_START)]));

    for end in 1..=keys.len() {
        let first = end.saturating_sub(MAX_PHRASE_KEYS);
        for start in first..end {
            let Some((prefix_cost, prefix_text, prefix_history)) = best[start].clone() else {
                continue;
            };
            let ranked = scorer
                .rank_phrases(&prefix_history, &keys[start..end], &kinds[start..end])
                .map_err(|error| EvalError::Backend {
                    detail: error.to_string(),
                })?;
            let Some((entry, cost)) = ranked.first() else {
                continue;
            };
            let total = prefix_cost.saturating_add(*cost);
            if best[end].as_ref().is_none_or(|(seen, ..)| total < *seen) {
                let mut text = prefix_text.clone();
                text.push_str(entry.text());
                let mut history = prefix_history.clone();
                history.push(entry.token());
                best[end] = Some((total, text, history));
            }
        }
    }

    Ok(best[keys.len()]
        .clone()
        .map(|(_, text, _)| text)
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{PhraseSource, correction_rate, parse_eval_corpus};
    use oxpinyin_core::{PhraseToken, SyllableKey};
    use std::collections::BTreeMap;

    // A trivial fixture PhraseSource + Dictionary + LanguageModel is exercised
    // in the crate's integration test; here we only pin the corpus parser and
    // the empty-source error path.

    struct EmptySource;
    impl PhraseSource for EmptySource {
        fn best_keys(&self, _token: PhraseToken) -> Option<Vec<SyllableKey>> {
            None
        }
        fn text(&self, _token: PhraseToken) -> Option<String> {
            None
        }
    }

    #[test]
    fn corpus_splits_on_null_tokens() {
        let corpus = "1 甲\n2 乙\n0 \n3 丙\n0 \n";
        let sentences = parse_eval_corpus(corpus).expect("parse");
        assert_eq!(sentences.len(), 2);
        assert_eq!(sentences[0], vec![PhraseToken::new(1), PhraseToken::new(2)]);
        assert_eq!(sentences[1], vec![PhraseToken::new(3)]);
    }

    #[test]
    fn a_trailing_sentence_without_a_separator_is_kept() {
        let sentences = parse_eval_corpus("5 戊\n6 己\n").expect("parse");
        assert_eq!(
            sentences,
            vec![vec![PhraseToken::new(5), PhraseToken::new(6)]]
        );
    }

    /// A minimal map-backed PhraseSource for the error test.
    struct MapSource(BTreeMap<u32, Vec<SyllableKey>>);
    impl PhraseSource for MapSource {
        fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>> {
            self.0.get(&token.value()).cloned()
        }
        fn text(&self, _token: PhraseToken) -> Option<String> {
            Some(String::new())
        }
    }

    #[test]
    fn a_token_without_pronunciation_is_an_error() {
        // Uses the real fixture dictionary/model so Scorer::new succeeds, then
        // fails on the missing pronunciation.
        let _ = EmptySource;
        let source = MapSource(BTreeMap::new());
        // Build via the integration path is heavier; here assert the keys
        // helper surfaces the error through correction_rate using the
        // testsupport fixtures.
        let vocab = include_str!("../../../fixtures/w4/mini-vocab.txt");
        let bigram = include_str!("../../../fixtures/w4/mini-bigram.txt");
        let dict = oxpinyin_testsupport::FixtureDictionary::parse(vocab).expect("vocab");
        let model =
            oxpinyin_testsupport::FixtureLanguageModel::parse(vocab, bigram).expect("model");
        let sentences = vec![vec![PhraseToken::new(1)]];
        let err = correction_rate(&dict, &model, &source, &sentences).unwrap_err();
        assert!(matches!(
            err,
            crate::error::EvalError::NoPronunciation { token: 1 }
        ));
    }
}
