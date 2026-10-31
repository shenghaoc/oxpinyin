//! `PhraseLookup::get_best_match` (`src/lookup/phrase_lookup.cpp:119-434`
//! at 074a2219), the n-gram Viterbi `ngseg` runs on a segmentable run.
//!
//! The pin's score is a `gfloat` (`lookup.h:36-53`): every assignment to
//! `m_poss` rounds to `f32`, while `log` and the unigram possibility are
//! `double` expressions. The arithmetic below keeps those types, so ties
//! and comparisons see the pin's values, not a wider Rust computation.

use std::collections::HashMap;

use oxpinyin_data::{BigramRow, BigramTable};

use crate::segment::phrase::PhraseSearch;
use crate::segment::{NULL_TOKEN, SENTENCE_START};

/// One `lookup_value_t` (`lookup.h:36-53`).
#[derive(Clone, Copy, Debug)]
struct Node {
    prev: u32,
    token: u32,
    poss: f32,
    last_step: i32,
}

/// One column: `LookupStepContent` plus the `LookupStepIndex` map.
#[derive(Clone, Debug, Default)]
struct Step {
    content: Vec<Node>,
    index: HashMap<u32, usize>,
}

impl Step {
    fn get(&self, token: u32) -> Option<&Node> {
        self.index.get(&token).map(|&at| &self.content[at])
    }
}

/// The segmentation result at each character start: a token where a phrase
/// starts, [`NULL_TOKEN`] elsewhere (`final_step` fills the array with
/// nulls before it backtraces).
pub(crate) fn get_best_match(
    search: &PhraseSearch<'_>,
    bigram: Option<&BigramTable>,
    lambda: f32,
    sentence: &[u32],
) -> Vec<u32> {
    let nstep = sentence.len() + 1;
    let mut steps = vec![Step::default(); nstep];
    // `populate_prefixes` (`:36-53`): `sentence_start` at step 0 with
    // `log(1.f)`.
    steps[0].content.push(Node {
        prev: NULL_TOKEN,
        token: SENTENCE_START,
        poss: 0.0,
        last_step: -1,
    });
    steps[0].index.insert(SENTENCE_START, 0);

    let context = Context {
        search,
        bigram,
        bigram_lambda: lambda,
        // `unigram_lambda(1. - lambda)`: the `double` difference rounded
        // back to the pin's `gfloat` member.
        unigram_lambda: (1.0_f64 - f64::from(lambda)) as f32,
    };

    for start in 0..nstep - 1 {
        if steps[start].content.is_empty() {
            continue;
        }
        for end in start + 1..nstep {
            let found = search.search(&sentence[start..end]);
            if found.ok() {
                // `search_bigram2(i, tokens), search_unigram2(i, tokens);`
                search_bigram2(&mut steps, start, &found.tokens, &context);
                if let Some(max) = max_node(&steps[start]) {
                    search_unigram2(&mut steps, start, &found.tokens, max, &context);
                }
            }
            if !found.continued {
                break;
            }
        }
    }

    final_step(&steps)
}

/// The scoring context keeps the helpers under clippy's argument cap.
struct Context<'a, 'b> {
    search: &'a PhraseSearch<'b>,
    bigram: Option<&'a BigramTable>,
    bigram_lambda: f32,
    unigram_lambda: f32,
}

/// `search_unigram2` (`:217-253`): expand from the maximum node only; ties
/// keep the first (`>`).
fn search_unigram2(
    steps: &mut [Step],
    nstep: usize,
    tokens: &[u32],
    max: Node,
    context: &Context<'_, '_>,
) {
    for &token in tokens {
        unigram_gen_next_step(steps, nstep, max, token, context);
    }
}

/// `search_bigram2` (`:255-305`).
fn search_bigram2(steps: &mut [Step], nstep: usize, tokens: &[u32], context: &Context<'_, '_>) {
    let count = steps[nstep].content.len();
    for at in 0..count {
        let current = steps[nstep].content[at];
        let Some(row) = load_row(context.bigram, current.token) else {
            continue;
        };
        for &token in tokens {
            // `SingleGram::get_freq`: a miss has no bigram edge; a stored
            // zero still has one.
            let Some(freq) = row
                .records
                .iter()
                .find_map(|(next, freq)| (*next == token).then_some(*freq))
            else {
                continue;
            };
            // `freq / (gfloat) total_freq`.
            let bigram_poss = freq as f32 / row.total as f32;
            bigram_gen_next_step(steps, nstep, current, token, bigram_poss, context);
        }
    }
}

/// `Bigram::load` (the system gram only; the pin's user bigram is empty),
/// with a backend failure or miss treated as the pin's `false`.
fn load_row(bigram: Option<&BigramTable>, token: u32) -> Option<BigramRow> {
    bigram?.load_successors(token).ok().flatten()
}

/// `unigram_gen_next_step` (`:307-325`).
fn unigram_gen_next_step(
    steps: &mut [Step],
    nstep: usize,
    current: Node,
    token: u32,
    context: &Context<'_, '_>,
) {
    let Some(item) = context.search.item(token) else {
        return;
    };
    let total = context.search_total();
    let elem_poss = f64::from(item.unigram) / total;
    if elem_poss < f64::EPSILON {
        return;
    }
    let poss =
        (f64::from(current.poss) + (elem_poss * f64::from(context.unigram_lambda)).ln()) as f32;
    let next = Node {
        prev: current.token,
        token,
        poss,
        last_step: i32::try_from(nstep).unwrap_or(i32::MAX),
    };
    save_next_step(steps, nstep + item.length, next);
}

/// `bigram_gen_next_step` (`:327-346`).
fn bigram_gen_next_step(
    steps: &mut [Step],
    nstep: usize,
    current: Node,
    token: u32,
    bigram_poss: f32,
    context: &Context<'_, '_>,
) {
    let Some(item) = context.search.item(token) else {
        return;
    };
    let unigram_poss = f64::from(item.unigram) / context.search_total();
    if bigram_poss < f32::EPSILON && unigram_poss < f64::EPSILON {
        return;
    }
    // `bigram_lambda * bigram_poss` is one `gfloat` multiply; the sum with
    // the `gdouble` unigram term is `double`.
    let mixed = f64::from(context.bigram_lambda * bigram_poss)
        + f64::from(context.unigram_lambda) * unigram_poss;
    let poss = (f64::from(current.poss) + mixed.ln()) as f32;
    let next = Node {
        prev: current.token,
        token,
        poss,
        last_step: i32::try_from(nstep).unwrap_or(i32::MAX),
    };
    save_next_step(steps, nstep + item.length, next);
}

/// `save_next_step` (`:348-379`): merge by arriving token; an equal score
/// keeps the original (`orig < next`).
fn save_next_step(steps: &mut [Step], next_pos: usize, next: Node) {
    let Some(step) = steps.get_mut(next_pos) else {
        return;
    };
    if let Some(&at) = step.index.get(&next.token) {
        if step.content[at].poss < next.poss {
            step.content[at] = next;
        }
        return;
    }
    step.index.insert(next.token, step.content.len());
    step.content.push(next);
}

/// `final_step` (`:382-434`).
fn final_step(steps: &[Step]) -> Vec<u32> {
    let mut result = vec![NULL_TOKEN; steps.len().saturating_sub(1)];
    let Some(last) = steps.last() else {
        return result;
    };
    let Some(mut max) = max_node(last) else {
        return result;
    };
    loop {
        if max.last_step < 0 {
            break;
        }
        let Some(step_pos) = usize::try_from(max.last_step).ok() else {
            break;
        };
        let Some(slot) = result.get_mut(step_pos) else {
            break;
        };
        *slot = max.token;
        // `m_handles[0]` is looked up in `steps[last_step]`, not the last
        // column (`:417-429`).
        let Some(next) = steps.get(step_pos).and_then(|step| step.get(max.prev)) else {
            return result;
        };
        max = *next;
    }
    result
}

fn max_node(step: &Step) -> Option<Node> {
    let mut best: Option<&Node> = None;
    for node in &step.content {
        if best.is_none_or(|seen| node.poss > seen.poss) {
            best = Some(node);
        }
    }
    best.copied()
}

impl Context<'_, '_> {
    /// `FacadePhraseIndex::get_phrase_index_total_freq()` as the `gdouble`
    /// denominator.
    fn search_total(&self) -> f64 {
        f64::from(self.search.unigram_total())
    }
}
