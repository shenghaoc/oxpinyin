//! `PhraseLookup::get_best_match` (`src/lookup/phrase_lookup.cpp:119-157`).
//!
//! Character-domain Viterbi over the phrase lexicon. One node per
//! `(position, arriving token)`, because the next bigram depends on which
//! token arrived; steps score `log(λ P_bi + (1-λ) P_uni)` with the
//! `table.conf` λ. Why `session.rs::collect_sentence` is not reused is
//! recorded in `docs/findings/segmenter-port.md`.

use std::collections::HashMap;

use crate::error::SegmentError;
use crate::lexicon::PhraseLexicon;
use crate::model::{NULL_TOKEN, SENTENCE_START, SegmentModel};

/// One `lookup_value_t` (`src/lookup/lookup.h:36-53`).
#[derive(Clone, Copy, Debug)]
struct Node {
    prev: u32,
    token: u32,
    poss: f64,
    last_step: i32,
}

/// One DP column: insertion-ordered content plus a token → index map,
/// matching `LookupStepContent` + `LookupStepIndex`.
#[derive(Clone, Debug, Default)]
struct Step {
    content: Vec<Node>,
    index: HashMap<u32, usize>,
}

impl Step {
    const fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    fn get(&self, token: u32) -> Option<&Node> {
        self.index.get(&token).map(|&idx| &self.content[idx])
    }

    fn insert(&mut self, node: Node) {
        self.index.insert(node.token, self.content.len());
        self.content.push(node);
    }
}

/// Best-path tokens at their start offsets, then converted like
/// `convert_to_utf8(..., "\n", true)` (`src/lookup/lookup.cpp:27-69`).
pub fn get_best_match(
    lexicon: &PhraseLexicon,
    model: &SegmentModel,
    lambda: f32,
    chars: &[char],
) -> Result<Option<Vec<(u32, String)>>, SegmentError> {
    if chars.is_empty() {
        return Ok(None);
    }

    let nstep = chars.len() + 1;
    let mut steps = vec![Step::default(); nstep];
    // populate_prefixes: sentence_start at position 0 with log(1) = 0
    // (`phrase_lookup.cpp:36-53`).
    steps[0].insert(Node {
        prev: NULL_TOKEN,
        token: SENTENCE_START,
        poss: 0.0,
        last_step: -1,
    });

    let ctx = ScoreCtx {
        lexicon,
        model,
        bigram_lambda: lambda,
        unigram_lambda: (1.0_f64 - f64::from(lambda)) as f32,
    };

    let mut span = String::new();
    for i in 0..nstep - 1 {
        if steps[i].is_empty() {
            continue;
        }
        for end in i + 1..nstep {
            span.clear();
            span.extend(chars[i..end].iter());
            let (ok, continued, tokens) = lexicon.search(&span);
            if ok {
                search_bigram2(&mut steps, i, tokens, &ctx)?;
                search_unigram2(&mut steps, i, tokens, &ctx)?;
            }
            if !continued {
                break;
            }
        }
    }

    Ok(final_step(&steps, lexicon))
}

/// Shared scoring context so the C++-shaped helpers stay under clippy's
/// argument cap without changing the formula.
struct ScoreCtx<'a> {
    lexicon: &'a PhraseLexicon,
    model: &'a SegmentModel,
    bigram_lambda: f32,
    unigram_lambda: f32,
}

/// `search_unigram2` (`phrase_lookup.cpp:217-253`): expand from the
/// maximum node only. Ties keep the first-inserted node (`>` not `>=`).
fn search_unigram2(
    steps: &mut [Step],
    nstep: usize,
    tokens: &[u32],
    ctx: &ScoreCtx<'_>,
) -> Result<(), SegmentError> {
    let Some(max_value) = max_node(&steps[nstep]) else {
        return Ok(());
    };
    for &token in tokens {
        unigram_gen_next_step(steps, nstep, max_value, token, ctx);
    }
    Ok(())
}

/// `search_bigram2` (`phrase_lookup.cpp:255-305`): expand from every node
/// that has a system-bigram row.
fn search_bigram2(
    steps: &mut [Step],
    nstep: usize,
    tokens: &[u32],
    ctx: &ScoreCtx<'_>,
) -> Result<(), SegmentError> {
    let count = steps[nstep].content.len();
    for idx in 0..count {
        let cur = steps[nstep].content[idx];
        let Some(row) = ctx.model.successors(cur.token)? else {
            continue;
        };
        if row.total == 0 {
            continue;
        }
        for &token in tokens {
            let Some(freq) = row
                .records
                .iter()
                .find_map(|(next, freq)| (*next == token).then_some(*freq))
            else {
                // get_freq miss: no bigram path for this token.
                continue;
            };
            let bigram_poss = freq as f32 / row.total as f32;
            bigram_gen_next_step(steps, nstep, cur, token, bigram_poss, ctx);
        }
    }
    Ok(())
}

/// `unigram_gen_next_step` (`phrase_lookup.cpp:307-325`).
fn unigram_gen_next_step(
    steps: &mut [Step],
    nstep: usize,
    cur: Node,
    token: u32,
    ctx: &ScoreCtx<'_>,
) {
    let Some(phrase_length) = phrase_length(ctx.lexicon, token) else {
        return;
    };
    let total = ctx.model.unigram_total();
    if total == 0 {
        return;
    }
    let elem_poss = ctx.model.unigram(token) as f64 / total as f64;
    if elem_poss < f64::EPSILON {
        return;
    }
    let next = Node {
        prev: cur.token,
        token,
        poss: cur.poss + (elem_poss * f64::from(ctx.unigram_lambda)).ln(),
        last_step: i32_from_step(nstep),
    };
    save_next_step(steps, nstep.saturating_add(phrase_length), next);
}

/// `bigram_gen_next_step` (`phrase_lookup.cpp:327-346`).
fn bigram_gen_next_step(
    steps: &mut [Step],
    nstep: usize,
    cur: Node,
    token: u32,
    bigram_poss: f32,
    ctx: &ScoreCtx<'_>,
) {
    let Some(phrase_length) = phrase_length(ctx.lexicon, token) else {
        return;
    };
    let total = ctx.model.unigram_total();
    let unigram_poss = if total == 0 {
        0.0
    } else {
        ctx.model.unigram(token) as f64 / total as f64
    };
    if bigram_poss < f32::EPSILON && unigram_poss < f64::EPSILON {
        return;
    }
    let mixed = f64::from(ctx.bigram_lambda) * f64::from(bigram_poss)
        + f64::from(ctx.unigram_lambda) * unigram_poss;
    if mixed <= 0.0 {
        return;
    }
    let next = Node {
        prev: cur.token,
        token,
        poss: cur.poss + mixed.ln(),
        last_step: i32_from_step(nstep),
    };
    save_next_step(steps, nstep.saturating_add(phrase_length), next);
}

/// `save_next_step` (`phrase_lookup.cpp:348-379`). Equal scores keep the
/// first-inserted node (`orig.m_poss < next.m_poss`).
fn save_next_step(steps: &mut [Step], next_pos: usize, next: Node) {
    let Some(step) = steps.get_mut(next_pos) else {
        return;
    };
    if let Some(&idx) = step.index.get(&next.token) {
        let orig = &mut step.content[idx];
        if orig.poss < next.poss {
            orig.prev = next.prev;
            orig.poss = next.poss;
            orig.last_step = next.last_step;
        }
        return;
    }
    step.insert(next);
}

/// `final_step` (`phrase_lookup.cpp:382-434`) plus `convert_to_utf8`.
fn final_step(steps: &[Step], lexicon: &PhraseLexicon) -> Option<Vec<(u32, String)>> {
    let last = steps.last()?;
    let mut max_value = max_node(last)?;

    let mut starts = vec![NULL_TOKEN; steps.len().saturating_sub(1)];
    loop {
        let cur_step = max_value.last_step;
        if cur_step < 0 {
            break;
        }
        let idx = usize::try_from(cur_step).ok()?;
        let slot = starts.get_mut(idx)?;
        *slot = max_value.token;
        // Look up handles[0] in steps[cur_step_pos], not the last column
        // (`phrase_lookup.cpp:417-429`).
        max_value = *steps.get(idx)?.get(max_value.prev)?;
    }

    let mut phrases = Vec::new();
    for token in starts {
        if token == NULL_TOKEN {
            continue;
        }
        let text = lexicon.text(token)?.to_owned();
        phrases.push((token, text));
    }
    if phrases.is_empty() {
        None
    } else {
        Some(phrases)
    }
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

fn phrase_length(lexicon: &PhraseLexicon, token: u32) -> Option<usize> {
    let length = lexicon.text(token)?.chars().count();
    (length > 0).then_some(length)
}

fn i32_from_step(step: usize) -> i32 {
    i32::try_from(step).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::get_best_match;
    use crate::lexicon::PhraseLexicon;
    use crate::model::{PINNED_LAMBDA, SegmentModel};

    fn toy() -> (PhraseLexicon, SegmentModel) {
        let lexicon = PhraseLexicon::from_pairs(vec![
            (10, "中".to_owned()),
            (11, "国".to_owned()),
            (12, "中国".to_owned()),
        ]);
        let mut unigrams = HashMap::new();
        unigrams.insert(10, 10);
        unigrams.insert(11, 10);
        unigrams.insert(12, 100);
        let model = SegmentModel::memory(unigrams, HashMap::new(), &lexicon);
        (lexicon, model)
    }

    #[test]
    fn a_higher_unigram_phrase_beats_the_singletons() {
        let (lexicon, model) = toy();
        let chars: Vec<char> = "中国".chars().collect();
        let path = get_best_match(&lexicon, &model, PINNED_LAMBDA, &chars)
            .expect("model cannot fail")
            .expect("path exists");
        assert_eq!(path, vec![(12, "中国".to_owned())]);
    }

    #[test]
    fn missing_long_phrase_falls_back_to_characters() {
        let lexicon = PhraseLexicon::from_pairs(vec![(10, "中".to_owned()), (11, "国".to_owned())]);
        let mut unigrams = HashMap::new();
        unigrams.insert(10, 10);
        unigrams.insert(11, 10);
        let model = SegmentModel::memory(unigrams, HashMap::new(), &lexicon);
        let chars: Vec<char> = "中国".chars().collect();
        let path = get_best_match(&lexicon, &model, PINNED_LAMBDA, &chars)
            .expect("model cannot fail")
            .expect("path exists");
        assert_eq!(path, vec![(10, "中".to_owned()), (11, "国".to_owned())]);
    }
}
