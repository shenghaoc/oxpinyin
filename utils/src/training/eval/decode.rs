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
//! It deliberately does not reuse `oxpinyin::engine`'s sentence decode:
//! an earlier decode ranked through typing heuristics (segmentation
//! penalties, phrase bonuses, integer costs), which are not the pin's
//! probabilities, and the live `eval_correction_rate` gate showed the
//! difference (0.52 vs 0.88 on a 25-sentence corpus).

use oxpinyin::data::PhraseLibraries;
use oxpinyin::vocab::{MAX_PHRASE_LENGTH, NULL_TOKEN, SENTENCE_START};
use rustc_hash::FxHashMap;

use oxpinyin::model::PhraseList;
use oxpinyin::parser::ChewingKey;
use oxpinyin::vocab::{PhraseEntry, PhraseToken};

use super::model::EvalLanguageModel;

/// Beam width per step (`phonetic_lookup.h:37`, `nbeam`).
const NBEAM: usize = 32;

/// `get_possible_pinyin` and `convert_to_utf8` over the loaded phrase
/// libraries (`eval_correction_rate.cpp:36-63`, `:113-116`).
pub struct LibraryPhraseSource<'a> {
    /// The loaded phrase libraries used for text and pronunciation.
    pub libraries: &'a PhraseLibraries,
}

impl LibraryPhraseSource<'_> {
    fn best_keys(&self, token: PhraseToken) -> Option<Vec<ChewingKey>> {
        // All stored readings compete, including partial and unspellable
        // keys. Strict improvement keeps the first tie and index 0 at zero
        // (`eval_correction_rate.cpp:47-60`). Convert only the winner.
        let item = self.libraries.library(token)?.item(token)?;
        let mut best = item.pronunciation(0)?;
        for reading in item.pronunciations().skip(1) {
            if reading.freq > best.freq {
                best = reading;
            }
        }
        Some(
            best.keys
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| ChewingKey::from_packed(u16::from_le_bytes(*pair)))
                .collect(),
        )
    }

    fn text(&self, token: PhraseToken) -> Option<String> {
        self.libraries
            .phrase_text(token)
            .and_then(|text| text.as_str().map(ToOwned::to_owned))
    }
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

/// Evaluates the correction rate over `sentences`, decoding each with the
/// dictionary and the evaluation model.
///
/// Returns `None` when a token has no pronunciation
/// (`get_possible_pinyin` would abort) or no text, when a key chain has no
/// phrase cover (upstream asserts exactly one best match), or when a decode
/// backend fails.
pub fn correction_rate<D>(
    dictionary: &D,
    model: &EvalLanguageModel,
    phrases: &LibraryPhraseSource<'_>,
    sentences: &[Vec<PhraseToken>],
) -> Option<EvalReport>
where
    D: PhraseList<Syllable = ChewingKey>,
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
    Some(EvalReport {
        tested,
        passed,
        rate,
        mismatches,
    })
}

/// Each token's best-pronunciation keys, concatenated (`get_possible_pinyin`).
fn sentence_keys(
    phrases: &LibraryPhraseSource<'_>,
    sentence: &[PhraseToken],
) -> Option<Vec<ChewingKey>> {
    let mut keys = Vec::new();
    for &token in sentence {
        keys.extend(phrases.best_keys(token)?);
    }
    Some(keys)
}

/// The text of a token sequence (`convert_to_utf8`). A token without text
/// is an error, never silently dropped: an incomplete expected sentence
/// would make the comparison — and the reported rate — wrong.
fn sentence_text(phrases: &LibraryPhraseSource<'_>, sentence: &[PhraseToken]) -> Option<String> {
    let mut text = String::new();
    for &token in sentence {
        let phrase = phrases.text(token)?;
        text.push_str(&phrase);
    }
    Some(text)
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
    index: FxHashMap<u32, usize>,
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
struct ScoreCtx<'a, 'tables> {
    model: &'a EvalLanguageModel<'tables>,
    bigram_lambda: f32,
    unigram_lambda: f32,
}

/// `PhoneticLookup<1, 1>::get_nbest_match` over a chain of stored
/// keys, one per cell, prefixed by `sentence_start`, then
/// `extract_result`: the best token sequence in order.
fn get_nbest_match<D>(
    dictionary: &D,
    model: &EvalLanguageModel,
    keys: &[ChewingKey],
) -> Option<Vec<PhraseToken>>
where
    D: PhraseList<Syllable = ChewingKey>,
{
    if keys.is_empty() {
        return Some(Vec::new());
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
        let mut phrase_keys = 0;
        for m in i + 1..nstep {
            // search_matrix skips zero cells and limits the collected keys
            // (`phonetic_key_matrix.cpp:362-369,389-393`).
            phrase_keys += usize::from(keys[m - 1] != ChewingKey::default());
            if phrase_keys > MAX_PHRASE_LENGTH {
                break;
            }
            let span = &keys[i..m];
            let entries = priced_entries(dictionary, span)?;
            if !entries.is_empty() {
                search_bigram2(&mut steps, i, m, &topresults, &entries, &ctx);
                search_unigram2(&mut steps, i, m, &topresults[0], &entries, &ctx);
            }
            // SEARCH_CONTINUED: stop once no stored phrase extends the span.
            let continued = dictionary.phrase_prefix_exists(span).ok()?;
            if !continued {
                break;
            }
        }
    }

    extract_result(&steps)
}

/// `search_matrix` for one key span: the phrases the span spells exactly, in
/// token order, each priced with its pronunciation possibility.
fn priced_entries<D>(dictionary: &D, span: &[ChewingKey]) -> Option<Vec<PhraseEntry>>
where
    D: PhraseList<Syllable = ChewingKey>,
{
    let mut entries = dictionary.lookup(span).ok()?;
    // Library-then-token order (`prepare_ranges` walk): token ascending, so
    // ties resolve as the pin's do.
    entries.sort_by_key(|entry| entry.token());
    Some(entries)
}

/// The token path ending at the last step's best node (`get_tails` and `extract_result`).
fn extract_result(steps: &[Step]) -> Option<Vec<PhraseToken>> {
    let nstep = steps.len();
    // get_tails: the best node of the last step (nbest = 1), then
    // extract_result backtracks it. No tail means no phrase cover: upstream
    // `assert(1 == results.size())`s, so it is a data error here, not a
    // failed correction.
    let tail = *steps.last()?.top_results(1).first()?;
    let mut result = vec![NULL_TOKEN; nstep];
    let mut cursor = tail;
    loop {
        let index = cursor.last_step;
        let Ok(index) = usize::try_from(index) else {
            break;
        };
        result[index] = cursor.token;
        cursor = *steps.get(index)?.get(cursor.prev)?;
    }
    Some(
        result
            .into_iter()
            .filter(|&token| token != NULL_TOKEN)
            .collect(),
    )
}

/// `search_unigram2` (`phonetic_lookup.h:540-576`): expand from the top
/// node only, over every phrase of the span.
fn search_unigram2(
    steps: &mut [Step],
    start: usize,
    end: usize,
    max: &Node,
    entries: &[PhraseEntry],
    ctx: &ScoreCtx<'_, '_>,
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
    ctx: &ScoreCtx<'_, '_>,
) {
    for value in topresults {
        // merge_single_gram fails without a system row: skip the node.
        let Some(row) = ctx.model.bigram_row(value.token) else {
            continue;
        };
        for entry in entries {
            let token = entry.token();
            let first = row.records.partition_point(|&(next, _)| next < token);
            // SingleGram::search visits every matching stored record,
            // including duplicates (`ngram.cpp:153-173`).
            for &(_, count) in row.records[first..]
                .iter()
                .take_while(|&&(next, _)| next == token)
            {
                let bigram_poss = count as f32 / row.total as f32;
                bigram_gen_next_step(steps, start, end, value, entry, bigram_poss, ctx);
            }
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
    ctx: &ScoreCtx<'_, '_>,
) {
    let token = entry.token();
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
    ctx: &ScoreCtx<'_, '_>,
) {
    let token = entry.token();
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
mod tests;
