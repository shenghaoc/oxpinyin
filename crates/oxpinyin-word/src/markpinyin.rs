//! `markpinyin.py` — assign pinyin and frequency to each recognized word.
//!
//! An *atomic* word's pinyins come from the dictionary list (`oldwords.txt`,
//! `word pinyin freq`); a *merged* word's pinyins are the cross-product of
//! its prefix and postfix pinyins joined by `'`, with the frequency
//! redistributed by the merge and component frequencies
//! (`markMergedWord`, `markpinyin.py:96-118`). Every pinyin list is then
//! rescaled so its frequencies sum to [`DEFAULT_PINYIN_TOTAL`], truncated to
//! integers, and pinyins below [`MINIMUM_PINYIN_FREQUENCY`] dropped
//! (`mergePinyin`, `:66-87`). The result is `recognized.txt`
//! (`word\tpinyin\tfreq`).

use std::collections::BTreeMap;

use crate::config::{DEFAULT_PINYIN_TOTAL, MINIMUM_PINYIN_FREQUENCY};
use crate::error::WordError;
use crate::partial::PartialWord;

/// Recursion depth guard: merged components are strictly shorter than the
/// merged word, so a well-formed input recurses at most a word's length;
/// this bound only stops a malformed cyclic list from overflowing the stack.
const MAX_DEPTH: usize = 512;

/// A `(pinyin, frequency)` mark.
pub type Mark = (String, u64);

/// The word→pinyin marker: atomic pinyins from `oldwords.txt` and merged
/// decompositions from `partialword.txt`.
#[derive(Clone, Debug, Default)]
pub struct Marker {
    /// `word → [(pinyin, freq)]` (`oldwords.txt`).
    atomic: BTreeMap<String, Vec<(String, u64)>>,
    /// `word → [(prefix, postfix, freq)]` (`partialword.txt`).
    merged: BTreeMap<String, Vec<(String, String, u64)>>,
}

impl Marker {
    /// A marker seeded from the partial-word records.
    #[must_use]
    pub fn new(partials: &[PartialWord]) -> Self {
        let mut merged: BTreeMap<String, Vec<(String, String, u64)>> = BTreeMap::new();
        for partial in partials {
            merged.entry(partial.merged.clone()).or_default().push((
                partial.prefix.clone(),
                partial.postfix.clone(),
                partial.freq,
            ));
        }
        Self {
            atomic: BTreeMap::new(),
            merged,
        }
    }

    /// Loads atomic `word pinyin freq` records (`load_atomic_words`,
    /// `markpinyin.py:28-44`).
    ///
    /// # Errors
    ///
    /// Returns [`WordError::Malformed`] on a line that is not three fields
    /// or whose frequency is not an integer.
    pub fn load_atomic(&mut self, text: &str) -> Result<(), WordError> {
        for line in text.lines() {
            if line.is_empty() {
                continue;
            }
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() != 3 {
                return Err(WordError::Malformed {
                    detail: format!("expected `word pinyin freq`: {line:?}"),
                });
            }
            let freq = fields[2].parse::<u64>().map_err(|_| WordError::Malformed {
                detail: format!("freq field {:?} is not an integer", fields[2]),
            })?;
            self.atomic
                .entry(fields[0].to_owned())
                .or_default()
                .push((fields[1].to_owned(), freq));
        }
        Ok(())
    }

    /// Marks one recognized word, returning `(pinyin, freq)` records
    /// (`markPinyin`, `markpinyin.py:121-129`).
    ///
    /// # Errors
    ///
    /// Returns [`WordError::MissingPinyin`] when a component word is neither
    /// atomic nor merged, or the decomposition recurses too deep.
    pub fn mark(&self, word: &str) -> Result<Vec<Mark>, WordError> {
        self.mark_inner(word, 0)
    }

    fn mark_inner(&self, word: &str, depth: usize) -> Result<Vec<Mark>, WordError> {
        if depth > MAX_DEPTH {
            return Err(WordError::MissingPinyin {
                phrase: word.to_owned(),
            });
        }
        if let Some(pinyins) = self.atomic.get(word) {
            let floats: Vec<(String, f64)> = pinyins
                .iter()
                .map(|(pinyin, freq)| (pinyin.clone(), *freq as f64))
                .collect();
            return Ok(merge_pinyin(&floats));
        }
        if let Some(decompositions) = self.merged.get(word) {
            return self.mark_merged(decompositions, depth);
        }
        Err(WordError::MissingPinyin {
            phrase: word.to_owned(),
        })
    }

    /// `markMergedWord` (`markpinyin.py:96-118`).
    fn mark_merged(
        &self,
        decompositions: &[(String, String, u64)],
        depth: usize,
    ) -> Result<Vec<Mark>, WordError> {
        let merged_sum: u64 = decompositions.iter().map(|(_, _, freq)| *freq).sum();
        let merged_sum = merged_sum as f64;

        let mut results: Vec<(String, f64)> = Vec::new();
        for (prefix, postfix, freq) in decompositions {
            let prefix_list = self.mark_inner(prefix, depth + 1)?;
            let postfix_list = self.mark_inner(postfix, depth + 1)?;
            let prefix_sum: u64 = prefix_list.iter().map(|(_, f)| *f).sum();
            let postfix_sum: u64 = postfix_list.iter().map(|(_, f)| *f).sum();
            let prefix_sum = prefix_sum as f64;
            let postfix_sum = postfix_sum as f64;

            for (prefix_pinyin, prefix_freq) in &prefix_list {
                for (postfix_pinyin, postfix_freq) in &postfix_list {
                    let merged_pinyin = format!("{prefix_pinyin}'{postfix_pinyin}");
                    let merged_freq = DEFAULT_PINYIN_TOTAL
                        * (*freq as f64)
                        * (*prefix_freq as f64)
                        * (*postfix_freq as f64)
                        / merged_sum
                        / prefix_sum
                        / postfix_sum;
                    results.push((merged_pinyin, merged_freq));
                }
            }
        }
        Ok(merge_pinyin(&results))
    }
}

/// Sum same-pinyin frequencies (in first-encounter order), rescale to
/// [`DEFAULT_PINYIN_TOTAL`], truncate to integers, and drop pinyins below
/// [`MINIMUM_PINYIN_FREQUENCY`] (`mergePinyin`, `markpinyin.py:66-87`).
fn merge_pinyin(pinyin_list: &[(String, f64)]) -> Vec<Mark> {
    // Insertion-ordered accumulation, matching the Python dict.
    let mut summed: Vec<(String, f64)> = Vec::new();
    for (pinyin, freq) in pinyin_list {
        if let Some(entry) = summed.iter_mut().find(|(p, _)| p == pinyin) {
            entry.1 += *freq;
        } else {
            summed.push((pinyin.clone(), *freq));
        }
    }
    let total: f64 = summed.iter().map(|(_, freq)| *freq).sum();
    let mut results = Vec::new();
    for (pinyin, freq) in summed {
        // int(default * freq / total) — truncation toward zero.
        let scaled = (DEFAULT_PINYIN_TOTAL * freq / total) as u64;
        if scaled < MINIMUM_PINYIN_FREQUENCY {
            continue;
        }
        results.push((pinyin, scaled));
    }
    results
}

/// Renders the `recognized.txt` lines for the given new words
/// (`markPinyins`, `markpinyin.py:132-163`).
///
/// # Errors
///
/// Propagates [`WordError::MissingPinyin`] for an unmarkable word.
pub fn render_recognized(marker: &Marker, new_words: &[String]) -> Result<String, WordError> {
    let mut out = String::new();
    for word in new_words {
        for (pinyin, freq) in marker.mark(word)? {
            out.push_str(word);
            out.push('\t');
            out.push_str(&pinyin);
            out.push('\t');
            out.push_str(&freq.to_string());
            out.push('\n');
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{Marker, merge_pinyin, render_recognized};
    use crate::partial::PartialWord;

    #[test]
    fn merge_pinyin_rescales_to_total_and_drops_small() {
        // Two pinyins 3:1 → scaled 75:25.
        let scaled = merge_pinyin(&[("a".to_owned(), 3.0), ("b".to_owned(), 1.0)]);
        assert_eq!(scaled, vec![("a".to_owned(), 75), ("b".to_owned(), 25)]);
    }

    #[test]
    fn merge_pinyin_drops_below_minimum() {
        // 50:50:1 of a tiny share → the third rounds under 3 and is dropped.
        let scaled = merge_pinyin(&[
            ("a".to_owned(), 50.0),
            ("b".to_owned(), 50.0),
            ("c".to_owned(), 1.0),
        ]);
        assert!(scaled.iter().all(|(p, _)| p != "c"), "tiny share dropped");
    }

    #[test]
    fn atomic_word_marks_from_the_dictionary() {
        let mut marker = Marker::new(&[]);
        marker
            .load_atomic("中国\tzhong'guo\t80\n中国\tzhong'guo2\t20\n")
            .expect("load");
        let marks = marker.mark("中国").expect("mark");
        // Same pinyin? No — distinct pinyins zhong'guo (80) and zhong'guo2
        // (20) → rescaled 80:20.
        assert_eq!(
            marks,
            vec![("zhong'guo".to_owned(), 80), ("zhong'guo2".to_owned(), 20)]
        );
    }

    #[test]
    fn merged_word_combines_component_pinyins() {
        // 中国 = 中 + 国, each atomic with one pinyin.
        let partials = vec![PartialWord {
            merged: "中国".to_owned(),
            prefix: "中".to_owned(),
            postfix: "国".to_owned(),
            freq: 10,
        }];
        let mut marker = Marker::new(&partials);
        marker
            .load_atomic("中\tzhong\t100\n国\tguo\t100\n")
            .expect("load");
        let marks = marker.mark("中国").expect("mark");
        assert_eq!(marks, vec![("zhong'guo".to_owned(), 100)]);
    }

    #[test]
    fn render_recognized_emits_word_pinyin_freq() {
        let mut marker = Marker::new(&[]);
        marker.load_atomic("中\tzhong\t100\n").expect("load");
        let text = render_recognized(&marker, &["中".to_owned()]).expect("render");
        assert_eq!(text, "中\tzhong\t100\n");
    }

    #[test]
    fn unknown_word_is_an_error_not_a_panic() {
        let marker = Marker::new(&[]);
        assert!(marker.mark("鬼").is_err());
    }
}
