//! `pinyin_phrase_segment`'s decoder — the port of `PhraseLookup::
//! get_best_match` (`src/lookup/phrase_lookup.cpp:121-157` at the pin).
//!
//! A span DP over the sentence's characters: step 0 holds one virtual
//! `sentence_start` node at zero cost (`populate_prefixes`,
//! `phrase_lookup.cpp:36-53` — `log(1.0)`, exactly representable in the
//! fixed-point scale); every span `[i, j)` within
//! [`MAX_PHRASE_LENGTH`] whose text is a stored phrase relaxes the node
//! at step `j` for each candidate token. Two cost branches per
//! candidate, straight from the pin: the bigram branch rides **every**
//! node of the from-step with the blended law
//! (`bigram_gen_next_step`, `phrase_lookup.cpp:327-344`), and the
//! unigram branch rides only the from-step's **best** node
//! (`search_unigram2` picks the step's maximum,
//! `phrase_lookup.cpp:217-241`) with the unigram law. Relaxation keeps
//! the best cost per (step, token) (`save_next_step`,
//! `:348-377`); the final step picks the best node of the last step and
//! backtracks, writing each token at its span's start position in a
//! character-length array (`final_step`, `:382-428` — "no need to
//! reverse the result").
//!
//! The failed-match shape is behavior: the result array is resized and
//! null-filled **before** the empty-last-step check that returns
//! `false`, so a failed match leaves a fully sized, all-null array —
//! `pinyin_get_n_phrase` afterwards reports the character count, not
//! zero. Costs ride the model's fixed-point step costs, the same
//! ported blend the sentence trellis uses; the recorded float-log
//! divergence class applies to near-tie segmentation choices
//! (`docs/findings/upstream-divergences.md`).

use std::collections::HashMap;

use oxpinyin_core::scoring::ScoringError;

use crate::error::EngineError;
use crate::nbest::SENTENCE_START;
use oxpinyin_core::{
    Dictionary, LanguageModel, NbestStepCosts, NbestStepRow, NbestSuccessors, PhraseEntry,
    PhraseToken, SyllableKey,
};

/// The span cap: upstream `MAX_PHRASE_LENGTH` (16, `oxpinyin-user`'s
/// ported value).
use crate::session::MAX_PHRASE_LENGTH;

/// One DP node: the best known way to cover `chars[..token_span_end]`
/// ending with `token` over `[last_step, token_span_end)`.
#[derive(Clone, Debug)]
struct Node {
    /// The node's token (the `m_handles[1]` role).
    token: PhraseToken,
    /// The predecessor node's token (`m_handles[0]`).
    prev: PhraseToken,
    /// `m_last_step` — the span's start position; [`SENTINEL_START`]
    /// marks the virtual start node.
    last_step: usize,
    /// Accumulated cost (upstream `m_poss`, on the fixed-point
    /// surprisal scale — lower is better).
    cost: i64,
}

/// The virtual start's `m_last_step` (upstream leaves it at the
/// zero-value sentinel; `usize::MAX` plays that role here).
const START_SENTINEL: usize = usize::MAX;

/// One DP step: insertion-ordered nodes plus the token → index map
/// (upstream's `steps_content` array + `steps_index` hash — the array
/// order is what makes the best-node scan deterministic on ties).
#[derive(Clone, Default)]
struct Step {
    content: Vec<Node>,
    index: HashMap<u32, usize>,
}

impl Step {
    fn relax(&mut self, node: Node) {
        if let Some(&position) = self.index.get(&node.token.value()) {
            let existing = &mut self.content[position];
            if node.cost < existing.cost {
                *existing = node;
            }
        } else {
            self.index.insert(node.token.value(), self.content.len());
            self.content.push(node);
        }
    }

    /// The step's best node — upstream's max-`m_poss` scan, first wins
    /// on ties (array order).
    fn best(&self) -> Option<&Node> {
        self.content.iter().min_by(|a, b| {
            a.cost.cmp(&b.cost).then_with(|| {
                let a_order = self.index.get(&a.token.value());
                let b_order = self.index.get(&b.token.value());
                a_order.cmp(&b_order)
            })
        })
    }
}

/// Segments `sentence` into its best dictionary phrase path.
///
/// One candidate's two step costs after `prev`: the bigram branch from
/// `prev`'s merged row read as the pin reads it — `get_freq`, then
/// `get_total_freq` on a hit (`phrase_lookup.cpp:288-292`) — and the
/// unigram branch. A model without row traversal keeps its pair costs.
///
/// # Errors
///
/// The model's failures, and a lookup refusal where the pin's merge or
/// read of the row asserts or leaves the value.
fn step_costs<L>(
    model: &L,
    prev: PhraseToken,
    token: PhraseToken,
    row: &mut NbestStepRow,
) -> Result<NbestStepCosts, EngineError>
where
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let lm = |error: L::Error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string()));
    let item = match model.nbest_successors_in_row(&prev, row).map_err(lm)? {
        None => return model.nbest_step_costs(&prev, &token).map_err(lm),
        Some(NbestSuccessors::Miss) => None,
        Some(NbestSuccessors::Row(gram)) => {
            let refuse = |fault| EngineError::lookup_gram(prev.value(), fault);
            if let Some(fault) = gram.fault() {
                return Err(refuse(fault));
            }
            match gram.get_freq(token.value()).map_err(refuse)? {
                Some(freq) => Some((freq, gram.total().map_err(refuse)?)),
                None => None,
            }
        }
    };
    model
        .nbest_item_costs_in_row(&prev, &token, item, row)
        .map_err(lm)
}

/// Returns `(matched, result)` where `result` is the character-length
/// array with each phrase's token at its span's start position and
/// `null_token` between phrases — `m_phrase_result`'s exact shape,
/// including the failed-match case (`false`, fully sized, all nulls).
///
/// # Errors
///
/// Propagates the model's step-cost failures.
pub fn phrase_segment<D, L>(
    dictionary: &D,
    model: &L,
    sentence: &str,
) -> Result<(bool, Vec<PhraseToken>), EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let chars: Vec<char> = sentence.chars().collect();
    let n = chars.len();
    let null = PhraseToken::new(0);
    let mut result = vec![null; n];

    let mut steps: Vec<Step> = vec![Step::default(); n + 1];
    steps[0].relax(Node {
        token: PhraseToken::new(SENTENCE_START),
        prev: null,
        last_step: START_SENTINEL,
        cost: 0,
    });

    for i in 0..n {
        if steps[i].content.is_empty() {
            continue;
        }
        // The unigram branch's from-node: the step's best, evaluated
        // once per step (stable during the step's own span walk —
        // relaxations write to later steps only).
        let uni_from = steps[i].best().cloned();
        for (prev, node) in steps[i]
            .content
            .iter()
            .map(|node| (node.token, node.clone()))
            .collect::<Vec<_>>()
        {
            // The predecessor's merged row, loaded at its first phrase:
            // `search_bigram2` loads and merges it on every `SEARCH_OK`
            // (`phrase_lookup.cpp:268-276`), and nothing writes the grams
            // within the call, so one load serves the step.
            let mut row = NbestStepRow::default();
            let upper = n.min(i + MAX_PHRASE_LENGTH);
            for end in (i + 1)..=upper {
                let text: String = chars[i..end].iter().collect();
                for token in dictionary.tokens_for_text(&text) {
                    let costs = step_costs(model, prev, token, &mut row)?;
                    // Bigram branch: every node of the from-step.
                    if let Some(blended) = costs.blended {
                        steps[end].relax(Node {
                            token,
                            prev,
                            last_step: i,
                            cost: node.cost + blended,
                        });
                    }
                    // Unigram branch: the from-step's best node only.
                    if let Some(from) = &uni_from
                        && from.token == prev
                        && let Some(unigram) = costs.unigram
                    {
                        steps[end].relax(Node {
                            token,
                            prev,
                            last_step: i,
                            cost: from.cost + unigram,
                        });
                    }
                }
            }
        }
    }

    // final_step: the array is already sized and null-filled; find the
    // last step's best node and backtrack.
    let Some(best) = steps[n].best() else {
        return Ok((false, result));
    };
    let mut cursor = best.clone();
    loop {
        if cursor.last_step == START_SENTINEL {
            break;
        }
        result[cursor.last_step] = cursor.token;
        let Some(node) = steps[cursor.last_step]
            .index
            .get(&cursor.prev.value())
            .map(|&position| steps[cursor.last_step].content[position].clone())
        else {
            // A broken chain — upstream returns `false` mid-backtrack
            // with whatever the walk wrote so far.
            return Ok((false, result));
        };
        cursor = node;
    }
    Ok((true, result))
}

#[cfg(test)]
mod tests {
    use super::phrase_segment;

    use oxpinyin_core::{
        Dictionary, LanguageModel, NbestStepCosts, PhraseEntry, PhraseToken, SyllableKey,
    };

    /// A tiny dictionary: the text → tokens reverse map the span DP
    /// probes, plus a model whose step costs come from per-token
    /// unigrams (the fixture shape: no bigrams, unigram possibility
    /// = count / total).
    struct Dict {
        entries: Vec<(String, PhraseToken)>,
    }

    impl Dict {
        fn parse(rows: &[(&str, u32)]) -> Self {
            Self {
                entries: rows
                    .iter()
                    .map(|(text, token)| ((*text).to_owned(), PhraseToken::new(*token)))
                    .collect(),
            }
        }
    }

    impl Dictionary for Dict {
        type Syllable = SyllableKey;
        type Entry = PhraseEntry;
        type Error = core::convert::Infallible;

        fn lookup(&self, _syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
            Ok(Vec::new())
        }

        fn tokens_for_text(&self, text: &str) -> Vec<PhraseToken> {
            self.entries
                .iter()
                .filter(|(stored, _)| stored == text)
                .map(|(_, token)| *token)
                .collect()
        }
    }

    struct Model {
        unigrams: Vec<(u32, u64)>,
        total: u64,
    }

    impl Model {
        /// The unigram branch's cost depends on the token alone
        /// (`elem_poss = unigram_freq / total`, upstream
        /// `unigram_gen_next_step:309-314`).
        fn unigram(&self, token: &PhraseToken) -> Option<NbestStepCosts> {
            let &(_, count) = self.unigrams.iter().find(|(t, _)| *t == token.value())?;
            debug_assert!(self.total > 0);
            // The test only needs a monotone scale of the same shape:
            // surprisal of the token's unigram possibility.
            let cost = (1000.0 * ((self.total as f64) / (count as f64)).log2()) as i64;
            Some(NbestStepCosts {
                blended: None,
                unigram: Some(cost),
            })
        }
    }

    impl LanguageModel for Model {
        type Token = PhraseToken;
        type Error = core::convert::Infallible;

        fn score(
            &self,
            _history: &[Self::Token],
            _token: &Self::Token,
            _edge_cost: i64,
        ) -> Result<i64, Self::Error> {
            Ok(0)
        }

        fn nbest_step_costs(
            &self,
            _prev: &Self::Token,
            token: &Self::Token,
        ) -> Result<NbestStepCosts, Self::Error> {
            Ok(self.unigram(token).unwrap_or(NbestStepCosts {
                blended: None,
                unigram: None,
            }))
        }
    }

    #[test]
    fn empty_sentence_matches_with_an_empty_result() {
        // nstep = 1 with nothing to match: the pin's final_step answers
        // `true` with a zero-length result.
        let dictionary = Dict::parse(&[]);
        let model = Model {
            unigrams: Vec::new(),
            total: 1,
        };
        let (matched, result) = phrase_segment(&dictionary, &model, "").expect("infallible model");
        assert!(matched);
        assert!(result.is_empty());
    }

    #[test]
    fn failed_match_leaves_a_sized_all_null_array() {
        // No dictionary phrase covers anything: the last step is empty,
        // the array stays character-length and null-filled, and the
        // retval is `false` — the failed-match shape
        // (`pinyin_get_n_phrase` then reports the character count).
        let dictionary = Dict::parse(&[]);
        let model = Model {
            unigrams: vec![(1, 1)],
            total: 1,
        };
        let (matched, result) =
            phrase_segment(&dictionary, &model, "abcd").expect("infallible model");
        assert!(!matched);
        assert_eq!(result.len(), 4);
        assert!(result.iter().all(|token| token.value() == 0));
    }

    #[test]
    fn phrase_tokens_sit_at_span_starts_with_nulls_between() {
        // 你好世界 covers the whole sentence: one token at position 0.
        let dictionary = Dict::parse(&[("你好世界", 7)]);
        let model = Model {
            unigrams: vec![(7, 5)],
            total: 5,
        };
        let (matched, result) =
            phrase_segment(&dictionary, &model, "你好世界").expect("infallible model");
        assert!(matched);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].value(), 7);
        assert!(result[1..].iter().all(|token| token.value() == 0));
    }

    #[test]
    fn two_phrases_write_tokens_at_their_span_starts() {
        // 你好 + 世界 as separate stored phrases: tokens at positions 0
        // and 2, nulls between.
        let dictionary = Dict::parse(&[("你好", 3), ("世界", 4)]);
        let model = Model {
            unigrams: vec![(3, 9), (4, 1)],
            total: 10,
        };
        let (matched, result) =
            phrase_segment(&dictionary, &model, "你好世界").expect("infallible model");
        assert!(matched);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0].value(), 3);
        assert_eq!(result[1].value(), 0);
        assert_eq!(result[2].value(), 4);
        assert_eq!(result[3].value(), 0);
    }

    /// A model whose every predecessor's merged row is `row`, pricing the
    /// bigram branch only for an item read from it.
    struct Rows {
        row: oxpinyin_core::MergedGram,
    }

    impl LanguageModel for Rows {
        type Token = PhraseToken;
        type Error = core::convert::Infallible;

        fn score(&self, _: &[Self::Token], _: &Self::Token, _: i64) -> Result<i64, Self::Error> {
            Ok(0)
        }

        fn nbest_successors_in_row<'a>(
            &self,
            _: &Self::Token,
            row: &'a mut oxpinyin_core::NbestStepRow,
        ) -> Result<Option<oxpinyin_core::NbestSuccessors<'a>>, Self::Error> {
            Ok(Some(
                row.get_or_try_init::<Self::Error>(|| Ok(Some(self.row.clone())))?
                    .map_or(
                        oxpinyin_core::NbestSuccessors::Miss,
                        oxpinyin_core::NbestSuccessors::Row,
                    ),
            ))
        }

        fn nbest_item_costs_in_row(
            &self,
            _: &Self::Token,
            _: &Self::Token,
            item: Option<(u32, u32)>,
            _: &mut oxpinyin_core::NbestStepRow,
        ) -> Result<NbestStepCosts, Self::Error> {
            Ok(NbestStepCosts {
                blended: item.map(|(freq, _)| i64::from(freq)),
                unigram: None,
            })
        }
    }

    /// N2 refusals 1 and 4 on `phrase_segment`'s merge and `get_freq`
    /// (`phrase_lookup.cpp:272-290`): the sentence-start predecessor's row
    /// refuses where the pin asserts or reads past it, and a whole row
    /// segments.
    #[test]
    fn a_predecessor_row_the_pin_cannot_read_refuses_the_segment() {
        use crate::error::EngineError;
        use crate::nbest::SENTENCE_START;
        use oxpinyin_core::MergedGram;
        let dictionary = Dict::parse(&[("你", 3)]);
        let system = || {
            let mut value = 1_u32.to_le_bytes().to_vec();
            value.extend_from_slice(&3_u32.to_le_bytes());
            value.extend_from_slice(&1_u32.to_le_bytes());
            value
        };
        let short_merge = Rows {
            row: MergedGram::merge(Some(system()), Some(vec![0, 0])).unwrap(),
        };
        assert_eq!(
            phrase_segment(&dictionary, &short_merge, "你").unwrap_err(),
            EngineError::ShortLookupPredecessor {
                token: SENTENCE_START
            }
        );
        let short_alone = Rows {
            row: MergedGram::merge(None, Some(vec![0, 0])).unwrap(),
        };
        assert_eq!(
            phrase_segment(&dictionary, &short_alone, "你").unwrap_err(),
            EngineError::UnsafeLookupPredecessor {
                token: SENTENCE_START
            }
        );
        let whole = Rows {
            row: MergedGram::merge(Some(system()), None).unwrap(),
        };
        let (matched, result) = phrase_segment(&dictionary, &whole, "你").unwrap();
        assert!(matched);
        assert_eq!(result[0].value(), 3);
    }
}
