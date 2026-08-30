//! `gen_k_mixture_model` — per-document corpus counting into a KMM
//! (`utils/training/gen_k_mixture_model.cpp`).
//!
//! Each input *file* is one document. A document is first aggregated into
//! per-document unigram counts (`token → freq`) and pair counts
//! (`token1 → token2 → count`) with the same boundary logic as `gen_ngram`
//! (`:72-137`); then the pair counts are folded into the persistent model
//! (`train_word_pair`, `:144-217`), the document count `m_N` is bumped, and
//! the surviving unigram freqs are added to the array headers
//! (`post_processing_unigram`, `:290-318`).
//!
//! Determinism: the per-document aggregation and the fold are
//! order-independent within a document (each `(t1,t2)` pair occurs once;
//! the maximum-occurs unigram subtraction is commutative), so iterating the
//! ordered maps token-ascending reproduces the same result as upstream's
//! hash-order walk. Across documents, order matters only through `m_Mr`, so
//! callers must feed documents in a fixed (index-file) order.

use std::collections::BTreeMap;

use crate::error::KmmError;
use crate::model::{ArrayItem, KMixtureModel, NULL_TOKEN, SENTENCE_START};

/// Default `--maximum-occurs-allowed` (`gen_k_mixture_model.cpp:48`).
pub const DEFAULT_MAX_OCCURS: u32 = 20;
/// Default `--maximum-increase-rates-allowed` (`:49`).
pub const DEFAULT_MAX_INCREASE_RATE: f64 = 3.0;

/// Generation parameters (the two CLI knobs plus pi-gram training).
#[derive(Clone, Copy, Debug)]
pub struct GenerateParams {
    /// `--maximum-occurs-allowed`.
    pub max_occurs: u32,
    /// `--maximum-increase-rates-allowed`.
    pub max_increase_rate: f64,
    /// `!--skip-pi-gram-training` (default true).
    pub train_pi_gram: bool,
}

impl Default for GenerateParams {
    fn default() -> Self {
        Self {
            max_occurs: DEFAULT_MAX_OCCURS,
            max_increase_rate: DEFAULT_MAX_INCREASE_RATE,
            train_pi_gram: true,
        }
    }
}

/// Per-document aggregation of one segmented file.
struct Document {
    /// `token1 → token2 → count`.
    bigram: BTreeMap<u32, BTreeMap<u32, u32>>,
    /// `token → freq` (mutated by the maximum-occurs filter during fold).
    unigram: BTreeMap<u32, u32>,
}

impl KMixtureModel {
    /// Folds one document (the text of one segmented file) into the model
    /// (`main` loop body, `gen_k_mixture_model.cpp:357-408`).
    ///
    /// # Errors
    ///
    /// Returns [`KmmError::Malformed`] when a non-empty line is not a
    /// `token phrase` record.
    pub fn add_document(&mut self, text: &str, params: GenerateParams) -> Result<(), KmmError> {
        let mut document = self.read_document(text, params.train_pi_gram)?;

        // Train each token1's second words, token-ascending.
        let token1s: Vec<u32> = document.bigram.keys().copied().collect();
        for token1 in token1s {
            self.train_second_word(&mut document, token1, params);
        }

        // magic.m_N++ per document.
        self.n = self.n.wrapping_add(1);

        // post_processing_unigram: add surviving unigram freqs to headers.
        let mut total: u32 = 0;
        for (&token, &freq) in &document.unigram {
            let gram = self.grams.entry(token).or_default();
            gram.header_freq = gram.header_freq.wrapping_add(freq);
            total = total.wrapping_add(freq);
        }
        // total_freq overflow is guarded upstream (skip the add on wrap).
        if let Some(sum) = self.total_freq.checked_add(total) {
            self.total_freq = sum;
        }
        Ok(())
    }

    /// Aggregate one document's unigram and pair counts (`read_document`,
    /// `:63-142`). Records each token's phrase text for the export column.
    fn read_document(&mut self, text: &str, train_pi_gram: bool) -> Result<Document, KmmError> {
        let mut bigram: BTreeMap<u32, BTreeMap<u32, u32>> = BTreeMap::new();
        let mut unigram: BTreeMap<u32, u32> = BTreeMap::new();

        // `cur` carries across lines; `last` is the previous line's token
        // (`cur_token = last_token = 0` initially, `gen_k_mixture_model.cpp:70`).
        let mut cur = NULL_TOKEN;
        for line in text.lines() {
            let token = self.parse_line(line)?;
            let mut last = cur;
            cur = token;

            // Skip null_token in the second word.
            if cur == NULL_TOKEN {
                continue;
            }
            let entry = unigram.entry(cur).or_insert(0);
            *entry = entry.wrapping_add(1);

            // Sentence boundary.
            if last == NULL_TOKEN {
                if !train_pi_gram {
                    continue;
                }
                last = SENTENCE_START;
            }

            let pair = bigram.entry(last).or_default().entry(cur).or_insert(0);
            *pair = pair.wrapping_add(1);
        }

        Ok(Document { bigram, unigram })
    }

    /// Parse one segmented line to its token, recording its phrase text.
    /// An empty line is `null_token` (`TAGLIB_PARSE_SEGMENTED_LINE`).
    fn parse_line(&mut self, line: &str) -> Result<u32, KmmError> {
        if line.is_empty() {
            return Ok(NULL_TOKEN);
        }
        let (head, rest) = line
            .split_once([' ', '\t'])
            .ok_or_else(|| KmmError::Malformed {
                detail: format!("no separator in {line:?}"),
            })?;
        let token = head.parse::<u32>().map_err(|_| KmmError::Malformed {
            detail: format!("token field {head:?} is not an integer"),
        })?;
        if token != NULL_TOKEN {
            self.record_text(token, rest);
        }
        Ok(token)
    }

    /// Fold one token1's second words into the model (`train_second_word` +
    /// `train_single_gram`, `:219-288`). Skips the store when the header WC
    /// did not grow (all pairs over-cap), removing a newly-created empty
    /// row.
    fn train_second_word(&mut self, document: &mut Document, token1: u32, params: GenerateParams) {
        let Some(seconds) = document.bigram.get(&token1) else {
            return;
        };
        let pairs: Vec<(u32, u32)> = seconds.iter().map(|(&t2, &c)| (t2, c)).collect();

        let existed = self.grams.contains_key(&token1);
        let gram = self.grams.entry(token1).or_default();
        let before = gram.header_wc;

        for (token2, count) in pairs {
            train_word_pair(gram, &mut document.unigram, token2, count, params);
        }

        let delta = gram.header_wc.wrapping_sub(before);
        if delta == 0 {
            if !existed {
                self.grams.remove(&token1);
            }
            return;
        }
        // magic.m_WC += delta, overflow-guarded (skip the add on wrap).
        if let Some(sum) = self.wc.checked_add(delta) {
            self.wc = sum;
        }
    }
}

/// `train_word_pair` (`:144-217`): fold one `(token1→)token2` pair count
/// into `gram`, applying the maximum-occurs filter (which subtracts an
/// over-cap pair's count back out of the per-document unigram).
fn train_word_pair(
    gram: &mut crate::model::SingleGram,
    unigram: &mut BTreeMap<u32, u32>,
    token2: u32,
    count: u32,
    params: GenerateParams,
) {
    match gram.items.get(&token2).copied() {
        Some(mut item) => {
            let cap = params
                .max_occurs
                .max(ceil_mul(item.mr, params.max_increase_rate));
            if count > cap {
                subtract_unigram(unigram, token2, count);
                return;
            }
            item.wc = item.wc.wrapping_add(count);
            item.n_n_0 = item.n_n_0.wrapping_add(1);
            if count == 1 {
                item.n_1 = item.n_1.wrapping_add(1);
            }
            item.mr = item.mr.max(count);
            gram.items.insert(token2, item);
        }
        None => {
            if count > params.max_occurs {
                subtract_unigram(unigram, token2, count);
                return;
            }
            gram.items.insert(
                token2,
                ArrayItem {
                    wc: count,
                    n_n_0: 1,
                    n_1: u32::from(count == 1),
                    mr: count,
                },
            );
        }
    }
    // Reached only when the pair was not skipped: grow the array header WC.
    gram.header_wc = gram.header_wc.wrapping_add(count);
}

/// `(guint32) ceil(mr * rate)` — the increase-rate cap.
fn ceil_mul(mr: u32, rate: f64) -> u32 {
    (f64::from(mr) * rate).ceil() as u32
}

/// Subtract an over-cap pair's `count` from the per-document unigram
/// (`:158-173`): store the difference, or steal the key on zero. The
/// `guint32` subtraction wraps (the "< 0 → abort" branch is unreachable),
/// so `wrapping_sub` reproduces it without a panic.
fn subtract_unigram(unigram: &mut BTreeMap<u32, u32>, token2: u32, count: u32) {
    let Some(freq) = unigram.get(&token2).copied() else {
        return;
    };
    let next = freq.wrapping_sub(count);
    if next == 0 {
        unigram.remove(&token2);
    } else {
        unigram.insert(token2, next);
    }
}

#[cfg(test)]
mod tests {
    use super::{GenerateParams, ceil_mul};
    use crate::model::{ArrayItem, KMixtureModel};

    fn seg(lines: &[&str]) -> String {
        let mut text = String::new();
        for line in lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    }

    #[test]
    fn ceil_mul_matches_ceil_of_product() {
        assert_eq!(ceil_mul(0, 3.0), 0);
        assert_eq!(ceil_mul(1, 3.0), 3);
        assert_eq!(ceil_mul(7, 3.0), 21);
        assert_eq!(ceil_mul(2, 3.5), 7);
    }

    #[test]
    fn one_document_counts_pairs_and_documents() {
        // Sentence "甲 乙 甲 乙" with sentence_start pigram.
        let doc = seg(&["10 甲", "20 乙", "10 甲", "20 乙"]);
        let mut model = KMixtureModel::new();
        model
            .add_document(&doc, GenerateParams::default())
            .expect("count");

        assert_eq!(model.n, 1, "one document");
        // Pairs: <start>→甲 (1), 甲→乙 (2), 乙→甲 (1).
        let start = &model.grams[&1];
        assert_eq!(
            start.items[&10],
            ArrayItem {
                wc: 1,
                n_n_0: 1,
                n_1: 1,
                mr: 1
            }
        );
        let jia = &model.grams[&10];
        assert_eq!(
            jia.items[&20],
            ArrayItem {
                wc: 2,
                n_n_0: 1,
                n_1: 0,
                mr: 2
            }
        );
        let yi = &model.grams[&20];
        assert_eq!(
            yi.items[&10],
            ArrayItem {
                wc: 1,
                n_n_0: 1,
                n_1: 1,
                mr: 1
            }
        );
        // Unigram freq: 甲 2, 乙 2.
        assert_eq!(model.grams[&10].header_freq, 2);
        assert_eq!(model.grams[&20].header_freq, 2);
        // total_freq = 4; wc = Σ array-header WC = 1 + 2 + 1 = 4 (validate
        // requires wc == total_freq).
        assert_eq!(model.total_freq, 4);
        assert_eq!(model.wc, 4);
        assert_eq!(model.text(10), Some("甲"));
    }

    #[test]
    fn two_documents_accumulate_document_frequency() {
        let doc = seg(&["10 甲", "20 乙"]);
        let mut model = KMixtureModel::new();
        let params = GenerateParams::default();
        model.add_document(&doc, params).expect("doc1");
        model.add_document(&doc, params).expect("doc2");

        assert_eq!(model.n, 2);
        // 甲→乙 seen once in each of two documents.
        let jia = &model.grams[&10];
        assert_eq!(
            jia.items[&20],
            ArrayItem {
                wc: 2,
                n_n_0: 2,
                n_1: 2,
                mr: 1
            }
        );
    }

    #[test]
    fn null_token_separates_sentences() {
        // Two sentences separated by a null token; each starts a pigram.
        let doc = seg(&["10 甲", "0 ", "20 乙"]);
        let mut model = KMixtureModel::new();
        model
            .add_document(&doc, GenerateParams::default())
            .expect("count");
        // <start>→甲 and <start>→乙, no 甲→乙 across the boundary.
        assert!(model.grams[&1].items.contains_key(&10));
        assert!(model.grams[&1].items.contains_key(&20));
        assert!(
            !model
                .grams
                .get(&10)
                .map(|g| g.items.contains_key(&20))
                .unwrap_or(false)
        );
    }

    #[test]
    fn skip_pi_gram_training_drops_sentence_start() {
        let doc = seg(&["10 甲", "20 乙"]);
        let mut model = KMixtureModel::new();
        let params = GenerateParams {
            train_pi_gram: false,
            ..GenerateParams::default()
        };
        model.add_document(&doc, params).expect("count");
        // No <start> row: the first token has no predecessor pair.
        assert!(!model.grams.contains_key(&1));
        // 甲→乙 still counted.
        assert_eq!(model.grams[&10].items[&20].wc, 1);
    }

    #[test]
    fn over_cap_pair_is_dropped_and_unigram_reduced() {
        // 甲 repeated 25 times after 乙 exceeds the default cap 20: the
        // 乙→甲 pair is dropped and 甲's unigram freq loses those 25.
        let mut lines = vec!["20 乙".to_owned()];
        for _ in 0..25 {
            lines.push("10 甲".to_owned());
        }
        let doc: String = lines.iter().map(|l| format!("{l}\n")).collect();
        let mut model = KMixtureModel::new();
        model
            .add_document(&doc, GenerateParams::default())
            .expect("count");
        // 乙→甲 count would be 24 (甲 follows 乙 once, then 甲 follows 甲 24×);
        // 甲→甲 is 24 > 20 → dropped; 乙→甲 is 1 → kept.
        let yi = &model.grams[&20];
        assert_eq!(yi.items.get(&10).map(|i| i.wc), Some(1));
        // 甲→甲 dropped: 甲 has no self-row items.
        assert!(
            model
                .grams
                .get(&10)
                .map(|g| g.items.is_empty())
                .unwrap_or(true)
        );
        // 甲's unigram freq: 25 total, minus the 24 over-cap self-pair = 1.
        assert_eq!(model.grams[&10].header_freq, 1);
    }

    #[test]
    fn malformed_line_is_an_error() {
        let mut model = KMixtureModel::new();
        let err = model
            .add_document("10\n", GenerateParams::default())
            .unwrap_err();
        assert!(matches!(err, crate::error::KmmError::Malformed { .. }));
    }
}
