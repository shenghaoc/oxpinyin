//! The n-best sentence trellis — the W14 port of upstream's
//! `PhoneticLookup<2, 3>` beam search (`lookup/phonetic_lookup.h`).
//!
//! One trellis per `guess_sentence` call. Steps are byte positions of the
//! remaining input (upstream uses key-matrix columns; the two coincide
//! wherever no apostrophe rides between keys). Nodes at a step are keyed by
//! the phrase token ending there and keep the best [`NSTORE`] values; each
//! step's beam keeps the best [`NBEAM`] values across nodes, ordered by the
//! ported `trellis_value_less_than` — "worse-than" — comparator with the
//! `log(1.2)` long-sentence penalty. Spans widen exactly while the phrase
//! table reports a longer phrase could still start (`SEARCH_CONTINUED`),
//! the same prefix-driven widening the window scan uses.
//!
//! Scoring is the upstream per-step form
//! `log((λ·bigram + (1−λ)·unigram) · pinyin_poss)` expressed on the core
//! fixed-point surprisal scale through
//! [`oxpinyin_core::LanguageModel::nbest_step_costs`]. The bigram branch
//! expands every beam value; the unigram branch expands only the beam's
//! head, as upstream's `search_bigram2` / `search_unigram2` do.
//! `pinyin_poss` is 1 for every span match — see
//! `docs/findings/sentence-surface.md` §3 for the recorded divergences
//! (polyphone discounting, float accumulation).

use std::collections::HashMap;

use compact_str::CompactString;
use oxpinyin_core::scoring::{ScoringError, expand_keys};
use oxpinyin_core::{
    Completeness, Cost, Dictionary, LanguageModel, NbestStepCosts, PhraseEntry, PhraseToken,
    SyllableKey,
};
use smallvec::SmallVec;

use crate::constraint::{Cell, ConstraintStore, PhraseSpan};
use crate::error::EngineError;
use crate::session::{MAX_PHRASE_LENGTH, SCAN_EXPANSION_LIMIT, ScanKey};

/// Values per `(position, token)` node (`nstore`).
const NSTORE: usize = 2;
/// Sentence rows extracted from the tails (`nbest`); also the fallback
/// DP's row cap.
pub(crate) const NBEST_ROWS: usize = 3;
/// Beam width per step (`nbeam`).
const NBEAM: usize = 32;

/// `LONG_SENTENCE_PENALTY = log(1.2)` (`phonetic_lookup.h:39`) on the core
/// scale: log2(1.2) ≈ 0.26304 bits.
const LONG_SENTENCE_PENALTY: Cost = 263;

/// The sentence-start seed token (`novel_types.h`: `sentence_start = 1`).
pub(crate) const SENTENCE_START: u32 = 1;

/// One decoded sentence row, in tail order (index 0 is the 1-best).
#[derive(Clone, Debug)]
pub(crate) struct NbestRow {
    /// Concatenated phrase texts.
    pub(crate) text: CompactString,
    /// The phrase tokens in order — the selection record a chosen row
    /// contributes (upstream keeps the whole `MatchResult` on the
    /// instance; the engine records tokens instead).
    pub(crate) tokens: Vec<PhraseToken>,
    /// The row's phrases at their walk positions — the `MatchResult`
    /// shape `diff_result` constraints and the constraint-aware train
    /// walk read. Walk-local coordinates; `Session::guess_sentence`
    /// absolutizes them when it stores the rows.
    pub(crate) spans: Vec<PhraseSpan>,
    /// Keys the sentence covers.
    pub(crate) keys: usize,
    /// Bytes of the input the sentence spans.
    pub(crate) span: usize,
    /// Accumulated trellis surprisal (lower is better).
    pub(crate) cost: Cost,
}

/// One hypothesis in the trellis.
#[derive(Clone, Debug)]
struct Value {
    /// Phrase token ending at this step (`m_handles[1]`).
    token: u32,
    /// Token of the phrase the predecessor value ended on (`m_handles[0]`).
    prev_token: u32,
    /// Position the covering phrase starts at (`m_last_step`); `usize::MAX`
    /// marks the seed.
    from: usize,
    /// Slot of the predecessor value in node `(from, prev_token)`
    /// (`m_sub_index`).
    sub: usize,
    /// Accumulated phrase characters (`m_sentence_length`).
    length: u32,
    /// Accumulated keys covered (the candidate's `consumed_keys`).
    keys: u32,
    /// Accumulated surprisal (`m_poss`, negated: lower is better).
    cost: Cost,
    /// Insertion sequence, the deterministic stand-in for upstream's heap
    /// order when the comparator ties.
    seq: u64,
}

impl Value {
    fn seed(token: u32) -> Self {
        Self {
            token,
            prev_token: 0,
            from: usize::MAX,
            sub: 0,
            length: 0,
            keys: 0,
            cost: 0,
            seq: 0,
        }
    }
}

/// One beam member: the value plus its slot inside its own node, which is
/// what a successor records as its backtracking `sub` (upstream's
/// `number()` assigns `m_current_index` before the beam is built).
#[derive(Clone, Debug)]
struct BeamEntry {
    value: Value,
    slot: usize,
}

/// `trellis_value_less_than`: whether `left` loses to `right`.
///
/// Port of `phonetic_lookup.h:66-87` in surprisal space (poss = −cost): a
/// sentence one key shorter is preferred unless the longer one's cost is
/// more than [`LONG_SENTENCE_PENALTY`] lower; any longer sentence otherwise
/// loses; at equal length the higher cost loses.
///
/// Deliberately **not** a strict weak order — upstream's comparator is
/// not either — which is why the sort closures below fall back to `seq`
/// on a tie: that keeps `sort_by` well-defined on the transitive data
/// the pins were measured under. The residual contract caveat and the
/// measured rejection of a heap-shaped selection are recorded in
/// `docs/findings/sentence-surface.md` §3.
fn loses_to(left: &Value, right: &Value) -> bool {
    match left.length.cmp(&right.length) {
        core::cmp::Ordering::Less if right.length - left.length == 1 => {
            // The shorter keeps its place unless beaten by more than the
            // penalty.
            left.cost > right.cost.saturating_add(LONG_SENTENCE_PENALTY)
        }
        core::cmp::Ordering::Greater if left.length - right.length == 1 => {
            // The longer must be better by at least the penalty.
            left.cost > right.cost.saturating_sub(LONG_SENTENCE_PENALTY)
        }
        core::cmp::Ordering::Equal => left.cost > right.cost,
        // A gap of two or more: the longer loses outright.
        core::cmp::Ordering::Greater => true,
        core::cmp::Ordering::Less => false,
    }
}

/// The trellis state over the remaining input's byte positions.
type NodeValues = SmallVec<[Value; NSTORE]>;

struct Trellis {
    /// `nodes[position][token]` = up to [`NSTORE`] values, best-first.
    nodes: Vec<HashMap<u32, NodeValues>>,
    /// Token → phrase text, gathered from the span searches.
    texts: HashMap<u32, CompactString>,
    /// Next insertion sequence.
    seq: u64,
}

impl Trellis {
    fn new(bound: usize, seed_token: u32) -> Self {
        let mut nodes = vec![HashMap::new(); bound + 1];
        let mut seed_values = NodeValues::new();
        seed_values.push(Value::seed(seed_token));
        nodes[0].insert(seed_token, seed_values);
        Self {
            nodes,
            texts: HashMap::new(),
            seq: 1,
        }
    }

    /// `trellis_node::eval_item`: keep the best [`NSTORE`] values.
    fn insert(&mut self, position: usize, value: Value) {
        let node = self.nodes[position].entry(value.token).or_default();
        if node.len() < NSTORE {
            // Insert best-first: before the first value this one beats.
            let slot = node
                .iter()
                .position(|stored| loses_to(stored, &value))
                .unwrap_or(node.len());
            node.insert(slot, value);
            return;
        }
        // Full: replace the worst when the newcomer beats it (upstream
        // replaces the min-heap front; on a comparator tie the stored value
        // wins).
        if let Some(worst) = node.last()
            && loses_to(worst, &value)
        {
            let last = node.len() - 1;
            node[last] = value;
            let mut index = last;
            while index > 0 && loses_to(&node[index], &node[index - 1]) {
                node.swap(index, index - 1);
                index -= 1;
            }
        }
    }

    /// The step's beam: every value at `position` with its node slot,
    /// best-first, capped at [`NBEAM`] (`get_candidates` +
    /// `get_top_results`).
    fn beam(&mut self, position: usize) -> SmallVec<[BeamEntry; NBEAM]> {
        // Token-sorted node iteration: HashMap iteration order must not
        // reach the sort input, and the `(rank, seq)` ordering below makes
        // the output independent of it regardless.
        let mut tokens: Vec<_> = self.nodes[position].keys().copied().collect();
        tokens.sort_unstable();
        let mut entries: SmallVec<[BeamEntry; NBEAM]> = tokens
            .into_iter()
            .filter_map(|token| self.nodes[position].get(&token))
            .flat_map(|node| {
                node.iter().enumerate().map(|(slot, value)| BeamEntry {
                    value: value.clone(),
                    slot,
                })
            })
            .collect();
        entries.sort_by(|left, right| {
            if loses_to(&right.value, &left.value) {
                core::cmp::Ordering::Less
            } else if loses_to(&left.value, &right.value) {
                core::cmp::Ordering::Greater
            } else {
                left.value.seq.cmp(&right.value.seq)
            }
        });
        entries.truncate(NBEAM);
        entries
    }

    /// The tails: the top [`NBEST_ROWS`] values at the furthest position that
    /// carries any, ordered by cost ascending (upstream selects with the
    /// comparator in `get_top_results`, then sorts the tails by raw poss
    /// with `trellis_value_compare`).
    fn tails(&mut self) -> Vec<Value> {
        let Some(position) = self
            .nodes
            .iter()
            .rposition(|node| !node.is_empty())
            .filter(|position| *position > 0)
        else {
            return Vec::new();
        };
        let mut tokens: Vec<_> = self.nodes[position].keys().copied().collect();
        tokens.sort_unstable();
        let mut values: Vec<Value> = tokens
            .into_iter()
            .filter_map(|token| self.nodes[position].get(&token))
            .flat_map(|node| node.iter().cloned())
            .collect();
        values.sort_by(|left, right| {
            if loses_to(right, left) {
                core::cmp::Ordering::Less
            } else if loses_to(left, right) {
                core::cmp::Ordering::Greater
            } else {
                left.seq.cmp(&right.seq)
            }
        });
        values.truncate(NBEST_ROWS);
        values.sort_by_key(|value| (value.cost, value.seq));
        values
    }

    /// Backtracks one tail into its phrase spans, forward order
    /// (`extract_result`).
    fn extract(&self, tail: &Value) -> Vec<(usize, u32)> {
        let mut spans = Vec::new();
        let mut current = tail.clone();
        while current.from != usize::MAX {
            spans.push((current.from, current.token));
            let Some(node) = self.nodes[current.from].get(&current.prev_token) else {
                break;
            };
            let Some(next) = node.get(current.sub) else {
                break;
            };
            current = next.clone();
        }
        spans.reverse();
        spans
    }

    fn text(&self, spans: &[(usize, u32)]) -> Option<CompactString> {
        let mut text = CompactString::const_new("");
        for (_, token) in spans {
            text.push_str(self.texts.get(token)?);
        }
        Some(text)
    }
}

/// One `(token, text)` pair a span search found, with the key count of the
/// path that spelled it (captured during enumeration, before the path is
/// unwound) and the entry's pronunciation possibility.
struct SpanEntry {
    token: u32,
    text: CompactString,
    keys: u32,
    pronunciation: Option<(u64, u64)>,
}

/// Computes the sentence rows for the input covered by `matrix`.
///
/// `matrix` is the scan matrix ([`crate::session`]'s key columns), `bound`
/// the parsed byte bound, `history` the selected tokens (the seed bigram
/// context of the unconstrained walk; empty means `sentence_start`).
///
/// `constraints` switches in the §3 constrained walk
/// (`get_nbest_match`'s gates, `phonetic_lookup.h:771-842`): the seed is
/// `sentence_start` exactly as upstream's `fill_prefixes` plants it (the
/// forced prefix is the context), `NoSearch` positions are skipped
/// entirely, a `OneStep` position runs exactly one span search to its
/// fixed end expanding only the forced token, and free spans break before
/// ending inside a forced run. `None` — or a store with no forcings — is
/// today's walk, bit for bit.
pub(crate) fn nbest_sentences<D, L>(
    matrix: &[Vec<ScanKey>],
    bound: usize,
    dictionary: &D,
    model: &L,
    history: &[PhraseToken],
    constraints: Option<&ConstraintStore>,
) -> Result<Vec<NbestRow>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let seed = match constraints {
        Some(_) => SENTENCE_START,
        None => history.last().map_or(SENTENCE_START, |token| token.value()),
    };
    let mut trellis = Trellis::new(bound, seed);
    // Memoised step costs: the beam revisits (prev, token) pairs.
    let mut costs: HashMap<(u32, u32), NbestStepCosts> = HashMap::new();
    let cell_at = |position: usize| constraints.and_then(|store| store.cell(position));

    let mut position = 0_usize;
    while position < bound {
        if matches!(cell_at(position), Some(Cell::NoSearch { .. })) {
            // The interior of a forced run: nothing may start here.
            position += 1;
            continue;
        }
        let beam = trellis.beam(position);
        if beam.is_empty() {
            position += 1;
            continue;
        }
        let head_seq = beam.first().map(|entry| entry.value.seq);

        if let Some(Cell::OneStep { token, end, .. }) = cell_at(position).cloned() {
            // One span search to the fixed end, the forced token only —
            // `search_bigram2`/`search_unigram2`'s ONESTEP branches —
            // then move on: no widening, no other token starts there.
            if end > position && end <= bound {
                let mut path: SmallVec<[SyllableKey; 16]> = SmallVec::new();
                let mut entries: Vec<SpanEntry> = Vec::new();
                span_entries(matrix, position, end, &mut path, dictionary, &mut entries)?;
                if let Some(entry) = entries.iter().find(|entry| entry.token == token.value()) {
                    expand_entry(
                        Expansion {
                            entry,
                            beam: &beam,
                            head_seq,
                            start: position,
                            end,
                        },
                        model,
                        &mut costs,
                        &mut trellis,
                    )?;
                }
            }
            position += 1;
            continue;
        }

        let mut end = position + 1;
        let mut widening = true;
        while end <= bound && widening {
            if matches!(cell_at(end), Some(Cell::NoSearch { .. })) {
                // A span may not end inside a forced run.
                break;
            }
            widening = false;
            let mut path: SmallVec<[SyllableKey; 16]> = SmallVec::new();
            let mut entries: Vec<SpanEntry> = Vec::new();
            span_entries(matrix, position, end, &mut path, dictionary, &mut entries)?;
            widen_probe(matrix, position, end, &mut path, dictionary, &mut widening)?;

            // First path per token wins: equal-cost duplicates from other
            // paths of the same span would only fill node slots.
            let mut first_of_token: SmallVec<[u32; 32]> = SmallVec::new();
            for entry in entries {
                if !first_of_token.contains(&entry.token) {
                    first_of_token.push(entry.token);
                } else {
                    continue;
                }
                expand_entry(
                    Expansion {
                        entry: &entry,
                        beam: &beam,
                        head_seq,
                        start: position,
                        end,
                    },
                    model,
                    &mut costs,
                    &mut trellis,
                )?;
            }
            end += 1;
        }
        position += 1;
    }

    let span = trellis
        .nodes
        .iter()
        .rposition(|node| !node.is_empty())
        .unwrap_or(0);
    let mut rows = Vec::new();
    for tail in trellis.tails() {
        let spans = trellis.extract(&tail);
        let Some(text) = trellis.text(&spans) else {
            continue;
        };
        if text.is_empty() {
            continue;
        }
        let phrase_spans: Vec<PhraseSpan> = spans
            .iter()
            .map(|(start, token)| PhraseSpan {
                start: *start,
                token: PhraseToken::new(*token),
                text: trellis.texts.get(token).cloned().unwrap_or_default(),
            })
            .collect();
        let tokens = phrase_spans.iter().map(|span| span.token).collect();
        rows.push(NbestRow {
            text,
            tokens,
            spans: phrase_spans,
            keys: tail.keys as usize,
            span,
            cost: tail.cost,
        });
    }
    Ok(rows)
}

/// Whether the forced `token` still spells over `[start, end)` under the
/// current matrix — the engine-side counterpart of
/// `validate_constraint`'s `compute_pronunciation_possibility` drop test
/// (below-`FLT_EPSILON` upstream; "the span search no longer yields the
/// token" here, the recorded §3 possibility divergence's shape).
///
/// # Errors
///
/// Propagates the dictionary lookup's backend failure.
pub(crate) fn span_finds_token<D>(
    matrix: &[Vec<ScanKey>],
    start: usize,
    end: usize,
    token: PhraseToken,
    dictionary: &D,
) -> Result<bool, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    let mut path: SmallVec<[SyllableKey; 16]> = SmallVec::new();
    let mut entries: Vec<SpanEntry> = Vec::new();
    span_entries(matrix, start, end, &mut path, dictionary, &mut entries)?;
    Ok(entries
        .iter()
        .any(|entry| entry.token == token.value() && !matches!(entry.pronunciation, Some((0, _)))))
}

/// One span expansion over the whole beam: the entry to push, the beam it
/// rides, and the span it covers.
struct Expansion<'a> {
    entry: &'a SpanEntry,
    beam: &'a [BeamEntry],
    head_seq: Option<u64>,
    start: usize,
    end: usize,
}

/// Expands one span entry over the whole beam — the shared body of the
/// free widening loop and the forced one-step expansion. The unigram
/// branch rides only the beam's head (`search_unigram2` uses
/// `topresults[0]`); the bigram branch every beam value.
fn expand_entry<L>(
    expansion: Expansion<'_>,
    model: &L,
    costs: &mut HashMap<(u32, u32), NbestStepCosts>,
    trellis: &mut Trellis,
) -> Result<(), EngineError>
where
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let Expansion {
        entry,
        beam,
        head_seq,
        start: position,
        end,
    } = expansion;
    trellis
        .texts
        .entry(entry.token)
        .or_insert_with(|| entry.text.clone());
    let chars: u32 = trellis.texts[&entry.token]
        .chars()
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let keys = entry.keys;
    // `pinyin_poss` as a step-cost term: matched/total over the item's
    // pronunciations, `None` (poss 1) without counts. A matched share of
    // 0 is upstream's below-ε skip of the step.
    let pronunciation = match entry.pronunciation {
        Some((matched, total)) if matched > 0 && total > 0 => {
            Some(oxpinyin_core::cost::surprisal(matched, total))
        }
        Some((0, _)) => return Ok(()),
        _ => None,
    };
    for predecessor in beam {
        let key = (predecessor.value.token, entry.token);
        let step = match costs.get(&key) {
            Some(step) => *step,
            None => {
                let step = model
                    .nbest_step_costs(
                        &PhraseToken::new(predecessor.value.token),
                        &PhraseToken::new(entry.token),
                    )
                    .map_err(|error| {
                        EngineError::Scoring(ScoringError::LanguageModel(error.to_string()))
                    })?;
                costs.insert(key, step);
                step
            }
        };
        let push = |mut cost: Cost, trellis: &mut Trellis| {
            if let Some(pron) = pronunciation {
                cost = cost.saturating_add(pron);
            }
            trellis.insert(
                end,
                Value {
                    token: entry.token,
                    prev_token: predecessor.value.token,
                    from: position,
                    sub: predecessor.slot,
                    length: predecessor.value.length.saturating_add(chars),
                    keys: predecessor.value.keys.saturating_add(keys),
                    cost: predecessor.value.cost.saturating_add(cost),
                    seq: trellis.seq,
                },
            );
            trellis.seq += 1;
        };
        if Some(predecessor.value.seq) == head_seq
            && let Some(step_cost) = step.unigram
        {
            push(step_cost, trellis);
        }
        if let Some(step_cost) = step.blended {
            push(step_cost, trellis);
        }
    }
    Ok(())
}

/// Enumerates every key-path from `start` to `end` and collects the
/// dictionary entries it spells (`search_matrix`).
fn span_entries<D>(
    matrix: &[Vec<ScanKey>],
    start: usize,
    end: usize,
    path: &mut SmallVec<[SyllableKey; 16]>,
    dictionary: &D,
    into: &mut Vec<SpanEntry>,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    let Some(column) = matrix.get(start) else {
        return Ok(());
    };
    for scan_key in column.iter().copied() {
        if scan_key.to > end {
            continue;
        }
        path.push(scan_key.key);
        if scan_key.to == end {
            extend_and_lookup(path, path.len(), dictionary, into)?;
        } else if path.len() < MAX_PHRASE_LENGTH {
            span_entries(matrix, scan_key.to, end, path, dictionary, into)?;
        }
        path.pop();
    }
    Ok(())
}

/// Whether any longer phrase could still start at the span being widened
/// (`SEARCH_CONTINUED`).
fn widen_probe<D>(
    matrix: &[Vec<ScanKey>],
    start: usize,
    end: usize,
    path: &mut SmallVec<[SyllableKey; 16]>,
    dictionary: &D,
    continued: &mut bool,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    let Some(column) = matrix.get(start) else {
        return Ok(());
    };
    for scan_key in column.iter().copied() {
        if scan_key.to > end {
            // A key overhanging the window: the phrase could continue.
            *continued = true;
            continue;
        }
        path.push(scan_key.key);
        if scan_key.to == end {
            let can_extend = dictionary.phrase_prefix_exists(path).map_err(|error| {
                EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
            })?;
            *continued |= can_extend;
        } else if path.len() < MAX_PHRASE_LENGTH {
            widen_probe(matrix, scan_key.to, end, path, dictionary, continued)?;
        }
        path.pop();
    }
    Ok(())
}

/// Incomplete-key expansion then one dictionary lookup per sequence.
fn extend_and_lookup<D>(
    path: &[SyllableKey],
    path_len: usize,
    dictionary: &D,
    into: &mut Vec<SpanEntry>,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    let has_incomplete = path
        .iter()
        .any(|key| key.completeness() == Completeness::Partial);
    if has_incomplete {
        for sequence in expand_keys(path, SCAN_EXPANSION_LIMIT) {
            push_span_entries(sequence.as_slice(), path_len, dictionary, into)?;
        }
    } else {
        push_span_entries(path, path_len, dictionary, into)?;
    }
    Ok(())
}

fn push_span_entries<D>(
    sequence: &[SyllableKey],
    path_len: usize,
    dictionary: &D,
    into: &mut Vec<SpanEntry>,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    let entries = dictionary
        .lookup(sequence)
        .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
    for entry in entries {
        let pronunciation = entry.pronunciation_possibility();
        into.push(SpanEntry {
            token: entry.token().value(),
            text: entry.into_text(),
            keys: path_len.try_into().unwrap_or(u32::MAX),
            pronunciation,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{LONG_SENTENCE_PENALTY, NbestRow, Value, loses_to, nbest_sentences};
    use crate::session::ScanKey;
    use oxpinyin_core::fixture::{FixtureDictionary, FixtureLanguageModel};
    use oxpinyin_core::graph::SegmentGraph;
    use oxpinyin_core::{Cost, LanguageModel, NbestStepCosts, OptionBits, PhraseToken};

    use crate::config::EmptyConfigSource;
    use crate::error::EngineError;
    use crate::session::build_scan_matrix;
    use crate::storage::StoragePaths;

    fn value(cost: Cost, length: u32, seq: u64) -> Value {
        Value {
            token: 1,
            prev_token: 0,
            from: usize::MAX,
            sub: 0,
            length,
            keys: length,
            cost,
            seq,
        }
    }

    #[test]
    fn equal_length_compares_pure_cost() {
        assert!(loses_to(&value(20, 3, 0), &value(10, 3, 1)));
        assert!(!loses_to(&value(10, 3, 0), &value(20, 3, 1)));
        // Comparator ties (equal cost and length) keep the earlier value.
        assert!(!loses_to(&value(10, 3, 0), &value(10, 3, 1)));
        assert!(!loses_to(&value(10, 3, 1), &value(10, 3, 0)));
    }

    #[test]
    fn a_gap_of_two_loses_for_the_longer_one() {
        assert!(loses_to(&value(0, 5, 0), &value(100, 3, 1)));
        assert!(!loses_to(&value(100, 3, 0), &value(0, 5, 1)));
    }

    #[test]
    fn one_shorter_is_preferred_within_the_penalty() {
        // The shorter (2) keeps its place unless beaten by more than P.
        assert!(!loses_to(
            &value(10 + LONG_SENTENCE_PENALTY, 2, 0),
            &value(10, 3, 1)
        ));
        assert!(loses_to(
            &value(11 + LONG_SENTENCE_PENALTY, 2, 0),
            &value(10, 3, 1)
        ));
        // The longer (3) must be better by at least P: 0 ≤ 263 − 263 keeps
        // it, 1 does not.
        assert!(loses_to(
            &value(1, 3, 0),
            &value(LONG_SENTENCE_PENALTY, 2, 1)
        ));
        assert!(!loses_to(
            &value(0, 3, 0),
            &value(LONG_SENTENCE_PENALTY, 2, 1)
        ));
    }

    /// A model answering fixed step costs, so the tests measure the trellis
    /// and not the blend.
    struct Fixed {
        blended: Option<Cost>,
        unigram: Option<Cost>,
    }

    impl LanguageModel for Fixed {
        type Error = EngineError;
        type Token = PhraseToken;

        fn score(
            &self,
            _history: &[PhraseToken],
            _token: &PhraseToken,
            edge_cost: Cost,
        ) -> Result<Cost, EngineError> {
            Ok(edge_cost)
        }

        fn nbest_step_costs(
            &self,
            _prev: &PhraseToken,
            _token: &PhraseToken,
        ) -> Result<NbestStepCosts, EngineError> {
            Ok(NbestStepCosts {
                blended: self.blended,
                unigram: self.unigram,
            })
        }
    }

    fn one_column_matrix(text: &str) -> (Vec<Vec<ScanKey>>, usize) {
        let graph = SegmentGraph::build(text.as_bytes()).expect("short input builds");
        let matrix = build_scan_matrix(&graph, OptionBits::default());
        let bound = graph.consumed();
        (matrix, bound)
    }

    #[test]
    fn a_model_without_step_costs_yields_no_rows() {
        let (matrix, bound) = one_column_matrix("nihao");
        let dict = FixtureDictionary::default();
        let model = Fixed {
            blended: None,
            unigram: None,
        };
        let rows = nbest_sentences(&matrix, bound, &dict, &model, &[], None)
            .expect("the trellis cannot fail here");
        assert!(rows.is_empty());
    }

    /// The mini vocabulary's `ni`+`hao` chain with equal step costs decodes
    /// the composition span.
    #[test]
    fn the_trellis_decodes_the_composition_span() {
        const VOCAB: &str =
            "token=1\tkeys=ni\ttext=你\tunigram=1000\ntoken=2\tkeys=hao\ttext=好\tunigram=900\n";
        let dict = FixtureDictionary::parse(VOCAB).expect("authored fixture");
        let model = Fixed {
            blended: Some(1_000),
            unigram: Some(2_000),
        };
        let (matrix, bound) = one_column_matrix("nihao");
        let rows: Vec<NbestRow> = nbest_sentences(&matrix, bound, &dict, &model, &[], None)
            .expect("the trellis cannot fail here");
        assert!(!rows.is_empty());
        assert_eq!(rows[0].text.as_str(), "你好");
        assert_eq!(rows[0].span, bound);
        assert_eq!(rows[0].keys, 2);
    }

    #[test]
    fn nbest_row_reports_its_span() {
        let row = NbestRow {
            text: "你好".into(),
            tokens: vec![PhraseToken::new(7)],
            spans: Vec::new(),
            keys: 2,
            span: 5,
            cost: 3,
        };
        assert_eq!(row.span, 5);
        assert_eq!(row.tokens, [PhraseToken::new(7)]);
    }

    /// Session-level smoke: the public types compose.
    #[test]
    fn session_types_compose() {
        let session = crate::Session::<FixtureDictionary, FixtureLanguageModel>::new(
            &EmptyConfigSource,
            StoragePaths::new("user"),
            FixtureDictionary::default(),
            FixtureLanguageModel::default(),
        )
        .expect("the fixtures open");
        assert!(!session.is_composing());
    }
}
