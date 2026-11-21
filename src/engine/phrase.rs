//! `pinyin_phrase_segment`'s decoder — the port of `PhraseLookup::
//! get_best_match` (`src/lookup/phrase_lookup.cpp:121-157` at the pin).
//!
//! A span DP over the sentence's characters: step 0 holds one virtual
//! `sentence_start` node at +0.0f32 (`populate_prefixes`,
//! `phrase_lookup.cpp:36-53` — `log(1.0)`); every span `[i, j)` within
//! [`MAX_PHRASE_LENGTH`] whose text is a stored phrase relaxes the node
//! at step `j` for each candidate token. Two probability branches per
//! candidate, straight from the pin: the bigram branch rides **every**
//! node of the from-step with the blended law
//! (`bigram_gen_next_step`, `phrase_lookup.cpp:327-344`), and the
//! unigram branch rides only the from-step's **best** node
//! (`search_unigram2` picks the step's maximum,
//! `phrase_lookup.cpp:217-241`) with the unigram law. Relaxation keeps
//! the best possibility per (step, token) (`save_next_step`,
//! `:348-377`); the final step picks the best node of the last step and
//! backtracks, writing each token at its span's start position in a
//! character-length array (`final_step`, `:382-428` — "no need to
//! reverse the result").
//!
//! The failed-match shape is behavior: the result array is resized and
//! null-filled **before** the empty-last-step check that returns
//! `false`, so a failed match leaves a fully sized, all-null array —
//! `pinyin_get_n_phrase` afterwards reports the character count, not
//! zero. Scores accumulate as `(f64::from(poss) + probability.ln()) as
//! f32`, with pre-log branches supplied by the language model.

use rustc_hash::FxHashMap;

use crate::engine::error::EngineError;
use crate::{
    PhraseToken, SyllableKey,
    model::{Dictionary, LanguageModel, NbestStepCosts, NbestStepRow, NbestSuccessors},
};

/// The span cap: upstream `MAX_PHRASE_LENGTH` (16, `oxpinyin`'s
/// ported value).
use crate::{MAX_PHRASE_LENGTH, SENTENCE_START};

/// One DP node: the best known way to cover `chars[..token_span_end]`
/// ending with `token` over `[last_step, token_span_end)`.
#[derive(Clone, Debug)]
struct Node {
    /// The node's token (the `m_handles[1]` role).
    token: PhraseToken,
    /// The predecessor node's token (`m_handles[0]`).
    prev: PhraseToken,
    /// `m_last_step` — the span's start position; [`START_SENTINEL`]
    /// marks the virtual start node.
    last_step: usize,
    /// Accumulated f32 log possibility (upstream `m_poss`).
    poss: f32,
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
    index: FxHashMap<u32, usize>,
}

impl Step {
    fn relax(&mut self, node: Node) {
        if let Some(&position) = self.index.get(&node.token) {
            let existing = &mut self.content[position];
            if existing.poss < node.poss {
                *existing = node;
            }
        } else {
            self.index.insert(node.token, self.content.len());
            self.content.push(node);
        }
    }

    /// The step's best node — upstream's max-`m_poss` scan, first wins
    /// on ties (array order).
    fn best(&self) -> Option<&Node> {
        let mut nodes = self.content.iter();
        let mut best = nodes.next()?;
        for node in nodes {
            if node.poss > best.poss {
                best = node;
            }
        }
        Some(best)
    }
}

/// Segments `sentence` into its best dictionary phrase path.
///
/// One candidate's two step costs after `prev`: the bigram branch from
/// `prev`'s merged row read as the pin reads it — `get_freq`, then
/// `get_total_freq` on a hit (`phrase_lookup.cpp:288-292`) — and the
/// unigram branch. A missing row supplies no bigram item.
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
    L: LanguageModel,
{
    let lm = |error: L::Error| EngineError::row_read(model, &error);
    let item = match model.nbest_successors_in_row(&prev, row).map_err(lm)? {
        None | Some(NbestSuccessors::Miss) => None,
        Some(NbestSuccessors::Row(gram)) => {
            let refuse = EngineError::lookup_gram;
            if let Some(fault) = gram.fault() {
                return Err(refuse(fault));
            }
            match gram.get_freq(token).map_err(refuse)? {
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
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    let words: Vec<u32> = sentence.chars().map(u32::from).collect();
    phrase_segment_ucs4(dictionary, model, &words)
}

/// [`phrase_segment`] over raw UCS-4 words, including surrogate or
/// above-Unicode values. The phrase table compares the stored raw keys;
/// no `char` or `str` is built from a non-scalar word.
///
/// # Errors
///
/// Propagates the model's step-cost failures.
pub(crate) fn phrase_segment_ucs4<D, L>(
    dictionary: &D,
    model: &L,
    sentence: &[u32],
) -> Result<(bool, Vec<PhraseToken>), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    segment(
        model,
        sentence.len(),
        dictionary.phrase_span_searches_are_observable(),
        |start, end| {
            let words = &sentence[start..end];
            (
                dictionary.tokens_for_ucs4(words),
                dictionary.phrase_span_continues(words),
            )
        },
    )
}

fn segment<L, F>(
    model: &L,
    n: usize,
    observable_searches: bool,
    mut tokens_for: F,
) -> Result<(bool, Vec<PhraseToken>), EngineError>
where
    L: LanguageModel,
    F: FnMut(usize, usize) -> (Vec<PhraseToken>, bool),
{
    let null = 0;
    let result = vec![null; n];

    let mut steps: Vec<Step> = vec![Step::default(); n + 1];
    steps[0].relax(Node {
        token: SENTENCE_START,
        prev: null,
        last_step: START_SENTINEL,
        poss: 0.0,
    });

    for i in 0..n {
        if steps[i].content.is_empty() && !observable_searches {
            continue;
        }
        // The unigram branch's from-node: the step's best, evaluated
        // once per step (stable during the step's own span walk —
        // relaxations write to later steps only).
        let uni_from = steps[i].best().cloned();
        let (previous, later) = steps.split_at_mut(i + 1);
        let predecessors = &previous[i].content;
        let upper = n.min(i + MAX_PHRASE_LENGTH);
        // Spans precede predecessor rows, and all bigram branches precede
        // the span's unigram branch (`phrase_lookup.cpp:137-149,
        // 255-302`). A later span's bad gram must not hide an earlier
        // span's refusal.
        for (offset, step) in later.iter_mut().take(upper - i).enumerate() {
            let end = i + 1 + offset;
            let (tokens, continued) = tokens_for(i, end);
            if tokens.is_empty() {
                if !continued {
                    break;
                }
                continue;
            }
            relax_span(model, step, predecessors, uni_from.as_ref(), &tokens, i)?;
            if !continued {
                break;
            }
        }
    }

    Ok(backtrack(&steps, n, result))
}

/// Relax one span's phrases `tokens` into the span's end step: the bigram
/// branch from every node of the from-step, per node in token order, and
/// then the unigram branch from the from-step's best node `uni_from`.
fn relax_span<L>(
    model: &L,
    step: &mut Step,
    predecessors: &[Node],
    uni_from: Option<&Node>,
    tokens: &[PhraseToken],
    from_step: usize,
) -> Result<(), EngineError>
where
    L: LanguageModel,
{
    let mut unigrams = Vec::new();
    for node in predecessors {
        // The predecessor's merged row, loaded at its first phrase
        // in this span: `search_bigram2` loads and merges it on
        // every `SEARCH_OK` (`phrase_lookup.cpp:268-276`), and
        // nothing writes the grams within the call, so one load
        // serves the span.
        let mut row = NbestStepRow::default();
        for &token in tokens {
            let costs = step_costs(model, node.token, token, &mut row)?;
            // Bigram branch: every node of the from-step.
            if let Some(blended) = costs.blended {
                step.relax(Node {
                    token,
                    prev: node.token,
                    last_step: from_step,
                    poss: (f64::from(node.poss) + blended.ln()) as f32,
                });
            }
            // Unigram branch: the from-step's best node only.
            if let Some(from) = uni_from
                && from.token == node.token
                && let Some(unigram) = costs.unigram
            {
                unigrams.push((token, unigram * f64::from(costs.unigram_lambda)));
            }
        }
    }
    for (token, probability) in unigrams {
        if let Some(from) = uni_from {
            step.relax(Node {
                token,
                prev: from.token,
                last_step: from_step,
                poss: (f64::from(from.poss) + probability.ln()) as f32,
            });
        }
    }
    Ok(())
}

/// `final_step`: backtrack from the last step's best node into `result`,
/// which is sized and null-filled. `false` when the last step holds no node
/// or the chain breaks.
fn backtrack(steps: &[Step], n: usize, mut result: Vec<PhraseToken>) -> (bool, Vec<PhraseToken>) {
    // final_step: the array is already sized and null-filled; find the
    // last step's best node and backtrack.
    let Some(best) = steps[n].best() else {
        return (false, result);
    };
    let mut cursor = best.clone();
    loop {
        if cursor.last_step == START_SENTINEL {
            break;
        }
        result[cursor.last_step] = cursor.token;
        let Some(node) = steps[cursor.last_step]
            .index
            .get(&cursor.prev)
            .map(|&position| steps[cursor.last_step].content[position].clone())
        else {
            // A broken chain — upstream returns `false` mid-backtrack
            // with whatever the walk wrote so far.
            return (false, result);
        };
        cursor = node;
    }
    (true, result)
}

#[cfg(test)]
mod tests;
