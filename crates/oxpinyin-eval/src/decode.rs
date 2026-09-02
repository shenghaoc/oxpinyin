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
//! The decode is the pin's own `PhoneticLookup<1, 1>` beam Viterbi
//! (`src/lookup/phonetic_lookup.h:515-820`), ported term for term: one
//! node per `(key position, arriving token)`, the top [`NBEAM`] nodes of
//! a step expanded (`get_top_results`), every phrase spelled by an exact
//! key span scored `log((λ·P_bi + (1 − λ)·P_uni) · P_pron)` after a bigram
//! row (`bigram_gen_next_step`) and `log(P_uni · P_pron · (1 − λ))` from
//! the best node otherwise (`unigram_gen_next_step`), at the pin's float
//! widths (`gfloat m_poss`). `P_pron` is the phrase's pronunciation
//! possibility for the span's keys (`PhraseItem::get_pronunciation_
//! possibility`, `phrase_index.h:136-164`); it is what keeps a rare
//! reading (红 read `gong`) from outranking the common phrase.
//!
//! It deliberately does not reuse `oxpinyin_engine`'s sentence decode:
//! that path ranks through `Scorer`'s typing heuristics (segmentation
//! penalties, phrase bonuses, integer costs), which are not the pin's
//! probabilities, and the live `eval_correction_rate` gate showed the
//! difference (0.52 vs 0.88 on a 25-sentence corpus).

use std::collections::HashMap;

use oxpinyin_core::{Dictionary, PhraseEntry, PhraseToken, SyllableKey};

use crate::error::EvalError;
use crate::model::EvalLanguageModel;

/// `sentence_start` (`novel_types.h:122`).
pub const SENTENCE_START: u32 = 1;
/// `null_token` (`novel_types.h:121`).
pub const NULL_TOKEN: u32 = 0;
/// Longest phrase the decoder considers, in keys (`MAX_PHRASE_LENGTH`).
const MAX_PHRASE_KEYS: usize = 16;
/// Beam width per step (`phonetic_lookup.h:37`, `nbeam`).
const NBEAM: usize = 32;

/// Supplies each token's best pronunciation and its text — the
/// `get_possible_pinyin` and `convert_to_utf8` inputs — and the phrase
/// lexicon the evaluation model is floored over.
pub trait PhraseSource {
    /// The highest-frequency pronunciation of `token` as syllable keys, or
    /// `None` when the token has no pronunciation.
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>>;

    /// The phrase text of `token`, or `None` when it is unknown.
    fn text(&self, token: PhraseToken) -> Option<String>;

    /// Every token of the loaded phrase index — the items `gen_unigram`
    /// floors (`utils/training/gen_unigram.cpp:45-68`) and
    /// `get_phrase_index_total_freq` sums over.
    fn lexicon_tokens(&self) -> Vec<PhraseToken>;
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
/// dictionary and the evaluation model.
///
/// # Errors
///
/// Returns [`EvalError`] when a token has no pronunciation
/// (`get_possible_pinyin` would abort) or no text, when a key chain has no
/// phrase cover (upstream asserts exactly one best match), or when a decode
/// backend fails.
pub fn correction_rate<D, P>(
    dictionary: &D,
    model: &EvalLanguageModel,
    phrases: &P,
    sentences: &[Vec<PhraseToken>],
) -> Result<EvalReport, EvalError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    P: PhraseSource,
{
    let mut tested = 0;
    let mut passed = 0;
    let mut mismatches = Vec::new();

    for sentence in sentences {
        if sentence.is_empty() {
            continue;
        }
        let keys = sentence_keys(phrases, sentence)?;
        let expected = sentence_text(phrases, sentence)?;
        let guessed = get_nbest_match(dictionary, model, &keys)?;
        let decoded = sentence_text(phrases, &guessed)?;

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

/// The text of a token sequence (`convert_to_utf8`). A token without text
/// is an error, never silently dropped: an incomplete expected sentence
/// would make the comparison — and the reported rate — wrong.
fn sentence_text<P: PhraseSource>(
    phrases: &P,
    sentence: &[PhraseToken],
) -> Result<String, EvalError> {
    let mut text = String::new();
    for &token in sentence {
        let phrase = phrases.text(token).ok_or(EvalError::NoText {
            token: token.value(),
        })?;
        text.push_str(&phrase);
    }
    Ok(text)
}

/// One `trellis_value_t` (`phonetic_lookup.h:44-63`): `m_handles[0]` is
/// `prev`, `m_handles[1]` is `token`, `m_poss` is a `gfloat`.
#[derive(Clone, Copy, Debug)]
struct Node {
    prev: u32,
    token: u32,
    poss: f32,
    last_step: i32,
}

/// One trellis step: insertion-ordered nodes plus a token → index map
/// (`LookupStepContent` + `LookupStepIndex`), one value per token
/// (`trellis_node<1>`).
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

    /// `get_top_results<1>(num, …)`: the `num` best nodes by `m_poss`,
    /// descending. Every node of a step covers the same key count, so the
    /// heap comparator reduces to `m_poss`; ties keep insertion order.
    fn top_results(&self, num: usize) -> Vec<Node> {
        let mut nodes = self.content.clone();
        nodes.sort_by(|lhs, rhs| rhs.poss.total_cmp(&lhs.poss));
        nodes.truncate(num);
        nodes
    }
}

/// Scoring context shared by the C++-shaped helpers.
struct ScoreCtx<'a> {
    model: &'a EvalLanguageModel,
    bigram_lambda: f32,
    unigram_lambda: f32,
}

/// `PhoneticLookup<1, 1>::get_nbest_match` over a clean chain of complete
/// keys, one per cell, prefixed by `sentence_start`, then
/// `extract_result`: the best token sequence in order.
fn get_nbest_match<D>(
    dictionary: &D,
    model: &EvalLanguageModel,
    keys: &[SyllableKey],
) -> Result<Vec<PhraseToken>, EvalError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let nstep = keys.len() + 1;
    let mut steps = vec![Step::default(); nstep];
    // fill_prefixes: sentence_start at step 0 with log(1.f) = 0.
    steps[0].insert(Node {
        prev: NULL_TOKEN,
        token: SENTENCE_START,
        poss: 0.0,
        last_step: -1,
    });

    let bigram_lambda = model.lambda_f32();
    let ctx = ScoreCtx {
        model,
        bigram_lambda,
        // `unigram_lambda(1. - lambda)`: a double subtraction stored as gfloat.
        unigram_lambda: (1.0_f64 - f64::from(bigram_lambda)) as f32,
    };

    for i in 0..nstep - 1 {
        if steps[i].is_empty() {
            continue;
        }
        let topresults = steps[i].top_results(NBEAM);
        let last = (i + MAX_PHRASE_KEYS).min(nstep - 1);
        for m in i + 1..=last {
            let span = &keys[i..m];
            // search_matrix: the phrases spelled exactly by this key span.
            let mut entries = dictionary
                .lookup(span)
                .map_err(|error| EvalError::Backend {
                    detail: error.to_string(),
                })?;
            if !entries.is_empty() {
                // Library-then-token order (`prepare_ranges` walk): token
                // ascending, so ties resolve as the pin's do.
                entries.sort_by_key(|entry| entry.token().value());
                search_bigram2(&mut steps, i, m, &topresults, &entries, &ctx);
                search_unigram2(&mut steps, i, m, &topresults[0], &entries, &ctx);
            }
            // SEARCH_CONTINUED: stop once no stored phrase extends the span.
            let continued =
                dictionary
                    .phrase_prefix_exists(span)
                    .map_err(|error| EvalError::Backend {
                        detail: error.to_string(),
                    })?;
            if !continued {
                break;
            }
        }
    }

    // get_tails: the best node of the last step (nbest = 1), then
    // extract_result backtracks it. No tail means no phrase cover: upstream
    // `assert(1 == results.size())`s, so it is a data error here, not a
    // failed correction.
    let tail = steps[nstep - 1]
        .top_results(1)
        .first()
        .copied()
        .ok_or(EvalError::Undecodable { keys: keys.len() })?;
    let mut result = vec![NULL_TOKEN; nstep];
    let mut cursor = tail;
    loop {
        let index = cursor.last_step;
        let Ok(index) = usize::try_from(index) else {
            break;
        };
        result[index] = cursor.token;
        let Some(previous) = steps.get(index).and_then(|step| step.get(cursor.prev)) else {
            return Err(EvalError::Undecodable { keys: keys.len() });
        };
        cursor = *previous;
    }
    Ok(result
        .into_iter()
        .filter(|&token| token != NULL_TOKEN)
        .map(PhraseToken::new)
        .collect())
}

/// `search_unigram2` (`phonetic_lookup.h:540-576`): expand from the top
/// node only, over every phrase of the span.
fn search_unigram2(
    steps: &mut [Step],
    start: usize,
    end: usize,
    max: &Node,
    entries: &[PhraseEntry],
    ctx: &ScoreCtx<'_>,
) {
    for entry in entries {
        unigram_gen_next_step(steps, start, end, max, entry, ctx);
    }
}

/// `search_bigram2` (`phonetic_lookup.h:578-641`): expand from every beam
/// node whose token has a bigram row, over the phrases of the span that
/// row records.
fn search_bigram2(
    steps: &mut [Step],
    start: usize,
    end: usize,
    topresults: &[Node],
    entries: &[PhraseEntry],
    ctx: &ScoreCtx<'_>,
) {
    for value in topresults {
        // merge_single_gram fails without a system row: skip the node.
        if !ctx.model.has_bigram_row(value.token) {
            continue;
        }
        for entry in entries {
            let Some(bigram_poss) = ctx.model.bigram_poss(value.token, entry.token().value())
            else {
                continue;
            };
            bigram_gen_next_step(steps, start, end, value, entry, bigram_poss, ctx);
        }
    }
}

/// `compute_pronunciation_possibility` over one exact key per cell: the
/// item's `get_pronunciation_possibility` for the span's keys,
/// `matched / (gfloat) total`. A fixture entry that carries no
/// pronunciation data counts as possibility 1.
fn pronunciation_possibility(entry: &PhraseEntry) -> f32 {
    match entry.pronunciation_possibility() {
        Some((_, 0)) => 0.0,
        Some((matched, total)) => matched as f32 / total as f32,
        None => 1.0,
    }
}

/// `unigram_gen_next_step` (`phonetic_lookup.h:643-668`).
fn unigram_gen_next_step(
    steps: &mut [Step],
    start: usize,
    end: usize,
    cur: &Node,
    entry: &PhraseEntry,
    ctx: &ScoreCtx<'_>,
) {
    let token = entry.token().value();
    let elem_poss = ctx.model.unigram_poss(token);
    if elem_poss < f64::EPSILON {
        return;
    }
    let pinyin_poss = pronunciation_possibility(entry);
    if pinyin_poss < f32::EPSILON {
        return;
    }
    // `cur->m_poss + log(elem_poss * pinyin_poss * unigram_lambda)`: the
    // product and the log are doubles, the sum is stored as gfloat.
    let poss = (f64::from(cur.poss)
        + (elem_poss * f64::from(pinyin_poss) * f64::from(ctx.unigram_lambda)).ln())
        as f32;
    save_next_step(
        steps,
        end,
        Node {
            prev: cur.token,
            token,
            poss,
            last_step: i32_from_step(start),
        },
    );
}

/// `bigram_gen_next_step` (`phonetic_lookup.h:670-697`).
fn bigram_gen_next_step(
    steps: &mut [Step],
    start: usize,
    end: usize,
    cur: &Node,
    entry: &PhraseEntry,
    bigram_poss: f32,
    ctx: &ScoreCtx<'_>,
) {
    let token = entry.token().value();
    let unigram_poss = ctx.model.unigram_poss(token);
    if bigram_poss < f32::EPSILON && unigram_poss < f64::EPSILON {
        return;
    }
    let pinyin_poss = pronunciation_possibility(entry);
    if pinyin_poss < f32::EPSILON {
        return;
    }
    // `(bigram_lambda * bigram_poss + unigram_lambda * unigram_poss) *
    // pinyin_poss`: a float product plus a double product, times the
    // float possibility, all in double; log; stored as gfloat.
    let mixed =
        f64::from(ctx.bigram_lambda * bigram_poss) + f64::from(ctx.unigram_lambda) * unigram_poss;
    let scaled = mixed * f64::from(pinyin_poss);
    if scaled <= 0.0 {
        return;
    }
    let poss = (f64::from(cur.poss) + scaled.ln()) as f32;
    save_next_step(
        steps,
        end,
        Node {
            prev: cur.token,
            token,
            poss,
            last_step: i32_from_step(start),
        },
    );
}

/// `save_next_step` → `insert_candidate` → `trellis_node<1>::eval_item`
/// (`phonetic_lookup_heap.h:108-122`): a node per token per step; a later
/// candidate replaces it only when strictly better (`m_poss <`), so equal
/// scores keep the first-inserted path.
fn save_next_step(steps: &mut [Step], index: usize, candidate: Node) {
    let Some(step) = steps.get_mut(index) else {
        return;
    };
    if let Some(&idx) = step.index.get(&candidate.token) {
        let existing = &mut step.content[idx];
        if existing.poss < candidate.poss {
            *existing = candidate;
        }
        return;
    }
    step.insert(candidate);
}

fn i32_from_step(step: usize) -> i32 {
    i32::try_from(step).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use super::{PhraseSource, correction_rate, parse_eval_corpus};
    use crate::model::EvalLanguageModel;
    use oxpinyin_core::{PhraseToken, SyllableKey};
    use oxpinyin_counter::Counts;
    use oxpinyin_data::Lambda;
    use std::collections::BTreeMap;

    // The full decode is exercised by the crate's integration tests (the
    // hand-computable homophone fixture) and the live pin gate; here we pin
    // the corpus parser and the error paths.

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

    /// A minimal map-backed PhraseSource for the error tests: keys and text
    /// per token, either absent.
    struct MapSource {
        keys: BTreeMap<u32, Vec<SyllableKey>>,
        text: BTreeMap<u32, String>,
    }
    impl PhraseSource for MapSource {
        fn best_keys(&self, token: PhraseToken) -> Option<Vec<SyllableKey>> {
            self.keys.get(&token.value()).cloned()
        }
        fn text(&self, token: PhraseToken) -> Option<String> {
            self.text.get(&token.value()).cloned()
        }
        fn lexicon_tokens(&self) -> Vec<PhraseToken> {
            self.keys
                .keys()
                .map(|&token| PhraseToken::new(token))
                .collect()
        }
    }

    fn fixture() -> (oxpinyin_testsupport::FixtureDictionary, EvalLanguageModel) {
        // The real fixture dictionary; an evaluation model that gives every
        // fixture token a unigram count.
        let vocab = include_str!("../../../fixtures/w4/mini-vocab.txt");
        let dict = oxpinyin_testsupport::FixtureDictionary::parse(vocab).expect("vocab");
        let mut counts = Counts::default();
        counts.unigrams.insert(1, 10);
        let model = EvalLanguageModel::from_counts(&counts, Lambda::PINNED);
        (dict, model)
    }

    #[test]
    fn a_token_without_pronunciation_is_an_error() {
        let (dict, model) = fixture();
        let source = MapSource {
            keys: BTreeMap::new(),
            text: BTreeMap::new(),
        };
        let sentences = vec![vec![PhraseToken::new(1)]];
        let err = correction_rate(&dict, &model, &source, &sentences).unwrap_err();
        assert!(matches!(
            err,
            crate::error::EvalError::NoPronunciation { token: 1 }
        ));
    }

    #[test]
    fn a_token_without_text_is_an_error_not_a_shorter_expected_sentence() {
        let (dict, model) = fixture();
        let mut keys = BTreeMap::new();
        keys.insert(1, vec![SyllableKey::from_text("ni").expect("key")]);
        let source = MapSource {
            keys,
            text: BTreeMap::new(),
        };
        let sentences = vec![vec![PhraseToken::new(1)]];
        let err = correction_rate(&dict, &model, &source, &sentences).unwrap_err();
        assert!(
            matches!(err, crate::error::EvalError::NoText { token: 1 }),
            "{err}"
        );
    }

    #[test]
    fn a_key_chain_without_a_phrase_cover_is_an_error_not_a_mismatch() {
        let (dict, model) = fixture();
        let mut keys = BTreeMap::new();
        // `guo` is not in the fixture vocabulary, so no phrase covers the
        // chain.
        keys.insert(1, vec![SyllableKey::from_text("guo").expect("key")]);
        let mut text = BTreeMap::new();
        text.insert(1, "国".to_owned());
        let source = MapSource { keys, text };
        let sentences = vec![vec![PhraseToken::new(1)]];
        let err = correction_rate(&dict, &model, &source, &sentences).unwrap_err();
        assert!(
            matches!(err, crate::error::EvalError::Undecodable { keys: 1 }),
            "{err}"
        );
    }
}
