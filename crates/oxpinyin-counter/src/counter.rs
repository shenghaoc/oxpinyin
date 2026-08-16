//! The `gen_ngram` counting loop (`utils/training/gen_ngram.cpp:78-125`).

use std::collections::{BTreeMap, BTreeSet};

use oxpinyin_segment::{NULL_TOKEN, SENTENCE_START};

use crate::counts::Counts;

/// Counts unigrams and bigrams over an ngseg token stream.
///
/// Carries the `gen_unigram` freq-1 floor token set so the unigram values
/// match the pin's `phrase_index` after `add_unigram_frequency` runs on top
/// of the already-seeded index.
#[derive(Clone, Debug)]
pub struct Counter {
    train_pi_gram: bool,
    floor_tokens: BTreeSet<u32>,
}

impl Counter {
    /// Builds a counter. `floor_tokens` is the seeded token set (the
    /// `SYSTEM_FILE` + `DICTIONARY` tokens `gen_unigram` raises by one);
    /// `train_pi_gram` mirrors `--skip-pi-gram-training` (default `true`).
    #[must_use]
    pub fn new(train_pi_gram: bool, floor_tokens: impl IntoIterator<Item = u32>) -> Self {
        Self {
            train_pi_gram,
            floor_tokens: floor_tokens.into_iter().collect(),
        }
    }

    /// Counts over the token stream, reproducing `gen_ngram.cpp:78-125`.
    #[must_use]
    pub fn count(&self, tokens: &[u32]) -> Counts {
        let mut occurrences: BTreeMap<u32, u64> = BTreeMap::new();
        let mut bigrams: BTreeMap<(u32, u32), u64> = BTreeMap::new();

        // Both running tokens start as `null_token` (zero); the rules below
        // are the table in `docs/findings/counter-port.md`.
        let mut cur_token = NULL_TOKEN;

        for &token in tokens {
            // Carry previous token: `last_token` gets the old `cur_token`.
            let last_token = std::mem::replace(&mut cur_token, token);

            // Skip null tokens: as the second word they contribute neither a
            // unigram nor a bigram.
            if NULL_TOKEN == cur_token {
                continue;
            }

            *occurrences.entry(cur_token).or_insert(0) += 1;

            // At a sentence boundary, substitute `sentence_start` for the
            // previous token; with pi-gram training off, drop the boundary
            // bigram (the unigram above is kept).
            let last_token = if NULL_TOKEN == last_token {
                if !self.train_pi_gram {
                    continue;
                }
                SENTENCE_START
            } else {
                last_token
            };

            *bigrams.entry((last_token, cur_token)).or_insert(0) += 1;
        }

        let mut unigrams: BTreeMap<u32, u64> = BTreeMap::new();
        // Freq-1 floor over every seeded token.
        for &token in &self.floor_tokens {
            let occ = occurrences.get(&token).copied().unwrap_or(0);
            unigrams.insert(token, 1 + occ);
        }
        // Corpus tokens absent from the seeded index (rare, but faithful:
        // `add_unigram_frequency` creates the item at the occurrence count).
        for (&token, &occ) in &occurrences {
            unigrams.entry(token).or_insert(occ);
        }

        Counts { unigrams, bigrams }
    }
}

#[cfg(test)]
mod tests {
    use super::Counter;

    fn count(
        tokens: &[u32],
        floor: &[u32],
        train_pi_gram: bool,
    ) -> (
        std::collections::BTreeMap<u32, u64>,
        std::collections::BTreeMap<(u32, u32), u64>,
    ) {
        let counts = Counter::new(train_pi_gram, floor.iter().copied()).count(tokens);
        (counts.unigrams, counts.bigrams)
    }

    #[test]
    fn null_as_second_word_is_skipped() {
        // [A, B, null] → null contributes neither a unigram nor a bigram
        // as the second word; only (start,A) and (A,B) are counted.
        let (uni, bi) = count(&[20, 10, 0], &[], true);
        assert_eq!(uni.get(&20), Some(&1));
        assert_eq!(uni.get(&10), Some(&1));
        assert!(!uni.contains_key(&0));
        assert_eq!(
            bi,
            std::collections::BTreeMap::from([((1, 20), 1), ((20, 10), 1)])
        );
    }

    #[test]
    fn sentence_boundary_substitutes_sentence_start() {
        // [A, null, B] → bigram (start, B) plus (A, start? no).
        let (uni, bi) = count(&[10, 0, 20], &[], true);
        assert_eq!(uni.get(&10), Some(&1));
        assert_eq!(uni.get(&20), Some(&1));
        assert_eq!(bi.get(&(1, 20)), Some(&1)); // sentence_start = 1
        assert!(!bi.contains_key(&(10, 20)));
    }

    #[test]
    fn adjacent_pair_increments_bigram() {
        let (_, bi) = count(&[10, 20, 20], &[], true);
        assert_eq!(bi.get(&(10, 20)), Some(&1));
        assert_eq!(bi.get(&(20, 20)), Some(&1));
    }

    #[test]
    fn first_token_of_stream_is_after_sentence_start() {
        // Initial last_token == null_token, so the first token pairs with
        // sentence_start.
        let (_, bi) = count(&[10, 20], &[], true);
        assert_eq!(bi.get(&(1, 10)), Some(&1));
        assert_eq!(bi.get(&(10, 20)), Some(&1));
    }

    #[test]
    fn skip_pi_gram_drops_boundary_bigrams_but_keeps_unigrams() {
        // With pi-gram off, a token after null still gets its unigram.
        let (uni, bi) = count(&[10, 0, 20], &[], false);
        assert_eq!(uni.get(&10), Some(&1));
        assert_eq!(uni.get(&20), Some(&1));
        assert!(bi.is_empty());
    }

    #[test]
    fn floor_adds_one_to_seeded_tokens_only() {
        let (uni, _) = count(&[10, 10], &[10, 99], true);
        // seeded 10: 1 + 2 occurrences; seeded 99: 1 + 0; corpus 20 absent.
        assert_eq!(uni.get(&10), Some(&3));
        assert_eq!(uni.get(&99), Some(&1));
    }
}
