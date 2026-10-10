//! The n-best sentence trellis — the W14 port of upstream's
//! `PhoneticLookup<nstore, nbest>` beam search (`lookup/phonetic_lookup.h`).
//! libpinyin instantiates `<2, 3>` (`pinyin.cpp:55`) and libzhuyin `<1, 1>`
//! (`zhuyin.cpp:50`); the pair is a per-session [`NbestShape`], set by each
//! facade, never a global constant.
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
use oxpinyin_core::cost::{NAN_COST, NEG_INF_COST, POS_INF_COST, cost_add, cost_gt};
use oxpinyin_core::scoring::{ScoringError, expand_keys};
use oxpinyin_core::{
    Completeness, Cost, Dictionary, LanguageModel, NbestStepCosts, PhraseEntry, PhraseToken,
    SyllableKey,
};
use smallvec::SmallVec;

use crate::constraint::{Cell, ConstraintStore, PhraseSpan};
use crate::error::EngineError;
use crate::session::{MAX_PHRASE_LENGTH, SCAN_EXPANSION_LIMIT, ScanKey};

/// Inline capacity of a node's value list: libpinyin's `nstore`, the
/// larger of the two shapes, so neither allocates per node. The live cap
/// is [`NbestShape::nstore`].
const NSTORE: usize = 2;
/// Sentence rows extracted from the tails (`nbest`) on the pinyin surface;
/// also the fallback DP's row cap there.
pub const NBEST_ROWS: usize = 3;

/// The two template constants of upstream's `PhoneticLookup<nstore,
/// nbest>`: values kept per `(position, token)` trellis node, and sentence
/// tails extracted.
///
/// libpinyin is `<2, 3>` (`pinyin.cpp:55`), libzhuyin is
/// `<1, 1>` (`zhuyin.cpp:50`); upstream asserts `nstore <= nbest`
/// (`phonetic_lookup.h:715`), which the two named shapes satisfy and no
/// other constructor exists to violate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NbestShape {
    nstore: usize,
    nbest: usize,
}

impl NbestShape {
    /// libpinyin's `PhoneticLookup<2, 3>` — the default.
    pub const PINYIN: Self = Self {
        nstore: NSTORE,
        nbest: NBEST_ROWS,
    };
    /// libzhuyin's `PhoneticLookup<1, 1>`: one value per trellis node and
    /// one sentence tail.
    pub const ZHUYIN: Self = Self {
        nstore: 1,
        nbest: 1,
    };

    /// Values kept per `(position, token)` node.
    #[must_use]
    pub const fn nstore(self) -> usize {
        self.nstore
    }

    /// Sentence tails extracted.
    #[must_use]
    pub const fn nbest(self) -> usize {
        self.nbest
    }
}

impl Default for NbestShape {
    fn default() -> Self {
        Self::PINYIN
    }
}
/// Beam width per step (`nbeam`).
const NBEAM: usize = 32;

/// `LONG_SENTENCE_PENALTY = log(1.2)` (`phonetic_lookup.h:39`) on the core
/// scale: log2(1.2) ≈ 0.26304 bits.
const LONG_SENTENCE_PENALTY: Cost = 263;

/// The sentence-start seed token (`novel_types.h`: `sentence_start = 1`).
pub const SENTENCE_START: u32 = 1;

/// One decoded sentence row, in tail order (index 0 is the 1-best).
#[derive(Clone, Debug)]
pub struct NbestRow {
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
}

impl Value {
    const fn seed(token: u32) -> Self {
        Self {
            token,
            prev_token: 0,
            from: usize::MAX,
            sub: 0,
            length: 0,
            keys: 0,
            cost: 0,
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

/// 074a2219 phonetic_lookup.h:66-91, including the nstore gate. The
/// longer-by-one branch is result-redundant: the final longer clause wins
/// regardless. This comparator is cyclic; never pass it to a Rust sort.
///
/// The possibility comparisons are float comparisons at the pin, so a NaN
/// cost makes every one of them false ([`cost_gt`]); only the length clause
/// is left standing.
fn loses_to(left: &Value, right: &Value, nstore: usize) -> bool {
    (nstore > 1
        && ((left.length.checked_add(1) == Some(right.length)
            && cost_gt(left.cost, cost_add(right.cost, LONG_SENTENCE_PENALTY)))
            || (left.length == right.length.saturating_add(1)
                && cost_gt(cost_add(left.cost, LONG_SENTENCE_PENALTY), right.cost))))
        || (left.length == right.length && cost_gt(left.cost, right.cost))
        || left.length > right.length
}

// Literal libstdc++ stl_heap.h algorithms (__push_heap, __adjust_heap,
// __pop_heap, __make_heap), not an ordered-sort approximation. Equal and
// cyclic comparisons make each assignment and the right-child tie matter.
fn heap_push<T: Clone>(
    values: &mut [T],
    mut hole: usize,
    top: usize,
    value: T,
    less: &impl Fn(&T, &T) -> bool,
) {
    while hole > top {
        let parent = (hole - 1) / 2;
        if !less(&values[parent], &value) {
            break;
        }
        values[hole] = values[parent].clone();
        hole = parent;
    }
    values[hole] = value;
}

fn heap_adjust<T: Clone>(
    values: &mut [T],
    mut hole: usize,
    len: usize,
    value: T,
    less: &impl Fn(&T, &T) -> bool,
) {
    let top = hole;
    let mut child = hole;
    while child < (len - 1) / 2 {
        child = 2 * (child + 1);
        if less(&values[child], &values[child - 1]) {
            child -= 1;
        }
        values[hole] = values[child].clone();
        hole = child;
    }
    if len.is_multiple_of(2) && child == (len - 2) / 2 {
        child = 2 * (child + 1);
        values[hole] = values[child - 1].clone();
        hole = child - 1;
    }
    heap_push(values, hole, top, value, less);
}

fn heap_pop<T: Clone>(values: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    if values.len() > 1 {
        let last = values.len() - 1;
        let value = values[last].clone();
        values[last] = values[0].clone();
        heap_adjust(values, 0, last, value, less);
    }
}

fn heap_make<T: Clone>(values: &mut [T], less: &impl Fn(&T, &T) -> bool) {
    if values.len() < 2 {
        return;
    }
    let mut parent = (values.len() - 2) / 2;
    loop {
        let value = values[parent].clone();
        heap_adjust(values, parent, values.len(), value, less);
        if parent == 0 {
            break;
        }
        parent -= 1;
    }
}

fn heap_top<T: Clone>(mut values: Vec<T>, count: usize, less: impl Fn(&T, &T) -> bool) -> Vec<T> {
    heap_make(&mut values, &less);
    let mut result = Vec::with_capacity(count.min(values.len()));
    while !values.is_empty() && result.len() < count {
        result.push(values[0].clone());
        heap_pop(&mut values, &less);
        values.pop();
    }
    result
}

/// `m_poss` of a cost: the natural-log possibility the pin keeps in a
/// `gfloat`.
fn poss(cost: Cost) -> f32 {
    match cost {
        NAN_COST => f32::NAN,
        POS_INF_COST => f32::NEG_INFINITY,
        NEG_INF_COST => f32::INFINITY,
        _ => (-(cost as f64) * core::f64::consts::LN_2 / 1000.0) as f32,
    }
}

/// A C float converted to `gint` as x86-64 does it (`cvttss2si`): anything
/// that does not fit, NaN included, is `INT_MIN`.
fn c_float_to_gint(value: f32) -> i32 {
    if value.is_nan() || !(-2_147_483_648.0..2_147_483_648.0).contains(&value) {
        i32::MIN
    } else {
        value as i32
    }
}

/// The final tail comparison (074a2219 phonetic_lookup.h:174-178):
/// `-(lhs->m_poss - rhs->m_poss)` in `float`, truncated to `gint`. A NaN
/// difference is `INT_MIN` for both orders, so NaN sorts before everything
/// and before itself, and the comparator is not transitive: the order that
/// results is the sort algorithm's (below).
fn tail_compare(left: &Value, right: &Value) -> i32 {
    c_float_to_gint(-(poss(left.cost) - poss(right.cost)))
}

/// GLib 2.90.0 `g_ptr_array_sort` (`g_sort_array`, gqsort.c:
/// `msort_with_tmp`): split at `n / 2`, sort both halves, merge taking the
/// left element on `cmp <= 0`. Stable for a consistent comparator; for the
/// inconsistent one above its order is exactly this recursion's.
pub(crate) fn glib_merge_sort<T: Clone>(values: &mut [T], compare: &impl Fn(&T, &T) -> i32) {
    if values.len() < 2 {
        return;
    }
    let mid = values.len() / 2;
    let (left, right) = values.split_at_mut(mid);
    glib_merge_sort(left, compare);
    glib_merge_sort(right, compare);
    let left = left.to_vec();
    let right = right.to_vec();
    let (mut l, mut r) = (0, 0);
    for slot in values {
        if r == right.len() || (l < left.len() && compare(&left[l], &right[r]) <= 0) {
            *slot = left[l].clone();
            l += 1;
        } else {
            *slot = right[r].clone();
            r += 1;
        }
    }
}

fn tail_merge(values: &mut [Value]) {
    glib_merge_sort(values, &tail_compare);
}

/// The trellis state over the remaining input's byte positions.
type NodeValues = SmallVec<[Value; NSTORE]>;

struct Trellis {
    /// The surface's `<nstore, nbest>`.
    shape: NbestShape,
    /// `nodes[position][token]` = the physical pinned heap slots.
    nodes: Vec<HashMap<u32, NodeValues>>,
    node_order: Vec<Vec<u32>>,
    /// Token → phrase text, gathered from the span searches.
    texts: HashMap<u32, CompactString>,
}

impl Trellis {
    /// Seeds step 0 with one zero-cost node per token — upstream's
    /// `fill_prefixes` (`phonetic_lookup.h:244-276`), which inserts
    /// every `m_prefixes` entry as an initial node at `log(1.0)`.
    /// `pinyin_guess_sentence_with_prefix` drives this with the prefix
    /// token list; the single-seed `new` stays the sentence path's law.
    fn with_seeds(bound: usize, seeds: &[u32], shape: NbestShape) -> Self {
        let mut nodes = vec![HashMap::new(); bound + 1];
        let mut node_order = vec![Vec::new(); bound + 1];
        for &seed_token in seeds {
            let mut seed_values = NodeValues::new();
            seed_values.push(Value::seed(seed_token));
            if !nodes[0].contains_key(&seed_token) {
                node_order[0].push(seed_token);
            }
            nodes[0].insert(seed_token, seed_values);
        }
        Self {
            shape,
            nodes,
            node_order,
            texts: HashMap::new(),
        }
    }

    /// 074a2219 phonetic_lookup_heap.h:56-81: a full heap replaces its
    /// front when the newcomer wins. At equal length the front is BEST,
    /// so reproducing the pin deliberately evicts the best stored value.
    fn insert(&mut self, position: usize, value: Value) {
        let nstore = self.shape.nstore();
        if !self.nodes[position].contains_key(&value.token) {
            self.node_order[position].push(value.token);
        }
        let node = self.nodes[position].entry(value.token).or_default();
        let less = |left: &Value, right: &Value| loses_to(left, right, nstore);
        if node.len() < nstore {
            node.push(value.clone());
            let last = node.len() - 1;
            heap_push(node, last, 0, value, &less);
        } else if less(&node[0], &value) {
            heap_pop(node, &less);
            let last = node.len() - 1;
            heap_push(node, last, 0, value, &less);
        }
    }

    /// 074a2219 phonetic_lookup.h:272-297: creation order, physical slots.
    fn candidates(&self, position: usize) -> Vec<BeamEntry> {
        self.node_order[position]
            .iter()
            .filter_map(|token| self.nodes[position].get(token))
            .flat_map(|node| {
                node.iter().enumerate().map(|(slot, value)| BeamEntry {
                    value: value.clone(),
                    slot,
                })
            })
            .collect()
    }

    fn beam(&mut self, position: usize) -> SmallVec<[BeamEntry; NBEAM]> {
        heap_top(self.candidates(position), NBEAM, |left, right| {
            loses_to(&left.value, &right.value, self.shape.nstore())
        })
        .into_iter()
        .collect()
    }

    /// 074a2219 phonetic_lookup.h:329-339: only the fixed final step,
    /// heap selection, then GLib's stable merge with gint comparison.
    fn tails(&mut self) -> Vec<Value> {
        let position = self.nodes.len() - 1;
        let mut values: Vec<_> = heap_top(
            self.candidates(position),
            self.shape.nbest(),
            |left, right| loses_to(&left.value, &right.value, self.shape.nstore()),
        )
        .into_iter()
        .map(|entry| entry.value)
        .collect();
        tail_merge(&mut values);
        values
    }

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
#[cfg(test)]
pub fn nbest_sentences<D, L>(
    matrix: &[Vec<ScanKey>],
    bound: usize,
    dictionary: &D,
    model: &L,
    history: &[PhraseToken],
    constraints: Option<&ConstraintStore>,
    shape: NbestShape,
) -> Result<Vec<NbestRow>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    nbest_sentences_generation(
        GenerationInput {
            matrix,
            bound,
            physical_separators: true,
            topology: None,
        },
        dictionary,
        model,
        history,
        constraints,
        shape,
    )
}

pub(super) fn nbest_sentences_generation<D, L>(
    input: GenerationInput<'_>,
    dictionary: &D,
    model: &L,
    history: &[PhraseToken],
    constraints: Option<&ConstraintStore>,
    shape: NbestShape,
) -> Result<Vec<NbestRow>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let GenerationInput {
        matrix,
        bound,
        physical_separators,
        topology,
    } = input;
    // The sentence path's single-seed law: the constrained decode seeds
    // the virtual start; the free walk seeds the history's last token.
    let seed = match constraints {
        Some(_) => SENTENCE_START,
        None => history.last().map_or(SENTENCE_START, |token| token.value()),
    };
    nbest_sentences_with_seeds_generation(
        GenerationInput {
            matrix,
            bound,
            physical_separators,
            topology,
        },
        dictionary,
        model,
        &[PhraseToken::new(seed)],
        constraints,
        shape,
    )
}

/// The prefix-seeded variant: every seed token becomes a zero-cost
/// initial node, upstream's `fill_prefixes` over `m_prefixes =
/// [sentence_start] + _compute_prefixes(prefix)` — the exact shape
/// `pinyin_guess_sentence_with_prefix` drives.
pub(super) fn nbest_sentences_with_seeds_generation<D, L>(
    input: GenerationInput<'_>,
    dictionary: &D,
    model: &L,
    seeds: &[PhraseToken],
    constraints: Option<&ConstraintStore>,
    shape: NbestShape,
) -> Result<Vec<NbestRow>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let GenerationInput {
        matrix,
        bound,
        physical_separators,
        topology,
    } = input;
    let owned;
    let generation = if let Some(topology) = topology {
        GenerationView {
            columns: &topology.columns,
            zeros: &topology.zeros,
        }
    } else {
        owned = crate::session::matrix::ParsedMatrix::from_scan(matrix, bound, physical_separators);
        GenerationView {
            columns: &owned.columns,
            zeros: &owned.zeros,
        }
    };
    let seed_values: Vec<u32> = seeds.iter().map(|token| token.value()).collect();
    let mut trellis = Trellis::with_seeds(bound, &seed_values, shape);
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

        if let Some(Cell::OneStep { token, end, .. }) = cell_at(position).cloned() {
            // One span search to the fixed end, the forced token only —
            // `search_bigram2`/`search_unigram2`'s ONESTEP branches —
            // then move on: no widening, no other token starts there.
            if end > position && end <= bound {
                let mut entries: Vec<SpanEntry> = Vec::new();
                generation.search(position, end, dictionary, &mut entries)?;
                if let Some(entry) = entries.iter().find(|entry| entry.token == token.value()) {
                    expand_entries(
                        core::slice::from_ref(entry),
                        &beam,
                        position,
                        end,
                        model,
                        &mut costs,
                        &mut trellis,
                    )?;
                }
            }
            position += 1;
            continue;
        }

        let env = NbestEnv {
            generation: &generation,
            bound,
            dictionary,
            model,
        };
        widen_free_span(&env, constraints, position, &beam, &mut costs, &mut trellis)?;
        position += 1;
    }

    let span = bound;
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

pub(super) struct GenerationInput<'a> {
    pub(super) matrix: &'a [Vec<ScanKey>],
    pub(super) bound: usize,
    pub(super) physical_separators: bool,
    pub(super) topology: Option<&'a crate::session::matrix::ParsedMatrix>,
}

#[derive(Default)]
struct GenerationPath {
    keys: SmallVec<[SyllableKey; 16]>,
    tones: SmallVec<[u8; 16]>,
}

/// Additional trellis-only view of the unchanged ordinary scan matrix.
/// 074a2219 phonetic_key_matrix.cpp:350-455 represents each physical
/// separator by a zero-key hop, including every doubled/trailing byte.
/// Exact-scheme formatting separators are not physical zero keys.
struct GenerationView<'a> {
    columns: &'a [Vec<ScanKey>],
    zeros: &'a [bool],
}

impl GenerationView<'_> {
    fn search<D>(
        &self,
        start: usize,
        end: usize,
        dictionary: &D,
        into: &mut Vec<SpanEntry>,
    ) -> Result<bool, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
    {
        let Some(column) = self.columns.get(start) else {
            return Ok(false);
        };
        if column.is_empty() && !self.zeros.get(start).copied().unwrap_or(false) {
            return Ok(false);
        }
        // search_matrix's empty-end-column gate is independent of paths.
        if end + 1 < self.columns.len() && self.columns[end].is_empty() && !self.zeros[end] {
            return Ok(true);
        }
        let mut path = GenerationPath::default();
        let mut continued = false;
        self.walk(start, end, dictionary, &mut path, into, &mut continued)?;
        // search_matrix files each path's ranges into per-library arrays.
        // This sort takes only integer library ids, never the trellis comparator.
        into.sort_by_key(|entry| entry.token >> 24);
        Ok(continued)
    }

    fn walk<D>(
        &self,
        start: usize,
        end: usize,
        dictionary: &D,
        path: &mut GenerationPath,
        into: &mut Vec<SpanEntry>,
        continued: &mut bool,
    ) -> Result<(), EngineError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
    {
        if start > end {
            return Ok(());
        }
        if start == end {
            if path.keys.len() > MAX_PHRASE_LENGTH {
                return Ok(());
            }
            if path.keys.is_empty() {
                *continued = true;
                return Ok(());
            }
            let entries = dictionary
                .trellis_records(&path.keys, &path.tones)
                .map_err(|error| {
                    EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
                })?;
            *continued |= dictionary
                .phrase_prefix_exists(&path.keys)
                .map_err(|error| {
                    EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
                })?;
            for entry in entries {
                let pronunciation = entry.pronunciation_possibility();
                into.push(SpanEntry {
                    token: entry.token().value(),
                    text: entry.into_text(),
                    keys: path.keys.len().try_into().unwrap_or(u32::MAX),
                    pronunciation,
                });
            }
            return Ok(());
        }
        if self.zeros.get(start).copied().unwrap_or(false) {
            return self.walk(start + 1, end, dictionary, path, into, continued);
        }
        let Some(column) = self.columns.get(start) else {
            return Ok(());
        };
        for key in column {
            if key.to > end {
                *continued = true;
                continue;
            }
            if key.to <= start {
                continue;
            }
            path.keys.push(key.key);
            path.tones.push(key.tone);
            if path.keys.len() <= MAX_PHRASE_LENGTH {
                self.walk(key.to, end, dictionary, path, into, continued)?;
            }
            path.keys.pop();
            path.tones.pop();
        }
        Ok(())
    }
}

/// The shared, immutable environment of one `nbest_sentences` walk: the
/// scan matrix and its bound, the dictionary, and the language model.
struct NbestEnv<'a, D, L> {
    /// The scan matrix columns.
    generation: &'a GenerationView<'a>,
    /// The walk's one-past-end column.
    bound: usize,
    /// The lexicon dictionary.
    dictionary: &'a D,
    /// The language model.
    model: &'a L,
}

/// The free widening walk preserves all pronunciation records and ranges;
/// no first-token reduction is permitted on the trellis generation path.
///
/// # Errors
///
/// Propagates the dictionary lookup's backend failure.
fn widen_free_span<D, L>(
    env: &NbestEnv<'_, D, L>,
    constraints: Option<&ConstraintStore>,
    position: usize,
    beam: &[BeamEntry],
    costs: &mut HashMap<(u32, u32), NbestStepCosts>,
    trellis: &mut Trellis,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let cell_at = |position: usize| constraints.and_then(|store| store.cell(position));
    let mut end = position + 1;
    let mut widening = true;
    while end <= env.bound && widening {
        if matches!(cell_at(end), Some(Cell::NoSearch { .. })) {
            // A span may not end inside a forced run.
            break;
        }
        let mut entries = Vec::new();
        widening = env
            .generation
            .search(position, end, env.dictionary, &mut entries)?;
        expand_entries(&entries, beam, position, end, env.model, costs, trellis)?;
        end += 1;
    }
    Ok(())
}

/// Whether the forced `token` still spells over `[start, end)` with a
/// kept pronunciation possibility under the current matrix: an entry
/// spelling the token keeps it when its possibility is `None` (no
/// counts — read as possibility 1) or `Some` with a nonzero matched
/// count; only `Some((0, _))` rejects. The engine-side counterpart of
/// `validate_constraint`'s `compute_pronunciation_possibility` drop
/// test (below-`FLT_EPSILON` upstream; the zero-guard from the §3
/// matched/total work). Recorded equivalent on
/// model20 data (`docs/findings/upstream-divergences.md`, the
/// `validate_constraint` entry): the below-ε boundary is unreachable
/// there, so the zero test decides both sides.
///
/// # Errors
///
/// Propagates the dictionary lookup's backend failure.
pub fn span_finds_token<D>(
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

/// 074a2219 phonetic_lookup.h:542-642, 787-788: all bigram
/// predecessors first, each over library/range/item order, then head unigram.
fn expand_entries<L>(
    entries: &[SpanEntry],
    beam: &[BeamEntry],
    start: usize,
    end: usize,
    model: &L,
    costs: &mut HashMap<(u32, u32), NbestStepCosts>,
    trellis: &mut Trellis,
) -> Result<(), EngineError>
where
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    for unigram in [false, true] {
        for predecessor in beam.iter().take(if unigram { 1 } else { beam.len() }) {
            for entry in entries {
                let pronunciation = match entry.pronunciation {
                    Some((matched, total)) if matched > 0 && total > 0 => {
                        Some(oxpinyin_core::cost::surprisal(matched, total))
                    }
                    Some((0, _)) => continue,
                    _ => None,
                };
                let key = (predecessor.value.token, entry.token);
                let step = if let Some(step) = costs.get(&key) {
                    *step
                } else {
                    let step = model
                        .nbest_step_costs(&PhraseToken::new(key.0), &PhraseToken::new(key.1))
                        .map_err(|error| {
                            EngineError::Scoring(ScoringError::LanguageModel(error.to_string()))
                        })?;
                    costs.insert(key, step);
                    step
                };
                let Some(mut cost) = (if unigram { step.unigram } else { step.blended }) else {
                    continue;
                };
                if let Some(pronunciation) = pronunciation {
                    cost = cost_add(cost, pronunciation);
                }
                trellis
                    .texts
                    .entry(entry.token)
                    .or_insert_with(|| entry.text.clone());
                let chars = entry.text.chars().count().try_into().unwrap_or(u32::MAX);
                trellis.insert(
                    end,
                    Value {
                        token: entry.token,
                        prev_token: predecessor.value.token,
                        from: start,
                        sub: predecessor.slot,
                        length: predecessor.value.length.saturating_add(chars),
                        keys: predecessor.value.keys.saturating_add(entry.keys),
                        cost: cost_add(predecessor.value.cost, cost),
                    },
                );
            }
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
    use super::{LONG_SENTENCE_PENALTY, NbestRow, NbestShape, Value, nbest_sentences};
    use crate::session::ScanKey;
    use oxpinyin_core::graph::SegmentGraph;
    use oxpinyin_core::{Cost, LanguageModel, NbestStepCosts, OptionBits, PhraseToken};
    use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

    use crate::config::EmptyConfigSource;
    use crate::error::EngineError;
    use crate::session::build_scan_matrix;
    use crate::storage::StoragePaths;

    fn loses_to(left: &Value, right: &Value) -> bool {
        super::loses_to(left, right, 2)
    }

    fn value(cost: Cost, length: u32, _seq: u64) -> Value {
        Value {
            token: 1,
            prev_token: 0,
            from: usize::MAX,
            sub: 0,
            length,
            keys: length,
            cost,
        }
    }

    /// A NaN cost makes every possibility comparison false, so only the
    /// length clause of `trellis_value_less_than` is left (`:66-91`).
    #[test]
    fn a_nan_cost_loses_only_by_length() {
        use oxpinyin_core::cost::{NAN_COST, NEG_INF_COST, POS_INF_COST};
        let nan = value(NAN_COST, 3, 0);
        for other in [
            value(10, 3, 0),
            value(NAN_COST, 3, 0),
            value(10, 2, 0),
            value(10, 4, 0),
        ] {
            assert!(!loses_to(&nan, &other) || nan.length > other.length);
            assert!(!loses_to(&other, &nan) || other.length > nan.length);
        }
        assert!(loses_to(&value(NAN_COST, 4, 0), &value(10, 3, 0)));
        // Infinities order as floats do: a possibility of +inf (cost -inf)
        // beats any finite one, 0 (cost +inf) loses to it.
        assert!(loses_to(&value(0, 3, 0), &value(NEG_INF_COST, 3, 0)));
        assert!(loses_to(&value(POS_INF_COST, 3, 0), &value(0, 3, 0)));
        assert!(!loses_to(
            &value(POS_INF_COST, 3, 0),
            &value(POS_INF_COST, 3, 0)
        ));
    }

    /// GLib's merge sort over the pin's tail comparator with NaN keys. The
    /// order for `[-9.25, nan, -1.5, nan]` is glib 2.90's (measured; a plain
    /// insertion sort over the same comparator leaves `[0, 1, 2, 3]`), and
    /// `INT_MIN` (x86-64) and `0` (arm64) for the NaN comparison never give
    /// different orders: a left element wins every `<= 0` merge step either
    /// way, so the architecture's float-to-int conversion is not observable.
    #[test]
    fn the_tail_sort_is_glibs_merge_and_blind_to_the_nan_conversion() {
        use super::{c_float_to_gint, glib_merge_sort};
        let sorted = |poss: &[f32], nan_as_zero: bool| {
            let mut items: Vec<(f32, usize)> = poss.iter().copied().zip(0..).collect();
            glib_merge_sort(&mut items, &|a: &(f32, usize), b: &(f32, usize)| {
                let difference = -(a.0 - b.0);
                if nan_as_zero && difference.is_nan() {
                    0
                } else {
                    c_float_to_gint(difference)
                }
            });
            items.into_iter().map(|item| item.1).collect::<Vec<_>>()
        };
        assert_eq!(
            sorted(&[-9.25, f32::NAN, -1.5, f32::NAN], false),
            [2, 0, 1, 3]
        );
        let values = [f32::NAN, -1.5, -9.25, -20.5];
        for count in 2..=6_u32 {
            for code in 0..4_usize.pow(count) {
                let poss: Vec<f32> = (0..count)
                    .map(|slot| values[(code >> (2 * slot)) & 3])
                    .collect();
                assert_eq!(sorted(&poss, false), sorted(&poss, true), "{poss:?}");
            }
        }
    }

    #[test]
    fn equal_length_compares_pure_cost() {
        assert!(loses_to(&value(20, 3, 0), &value(10, 3, 1)));
        assert!(!loses_to(&value(10, 3, 0), &value(20, 3, 1)));
        // Equal cost and length are comparator ties.
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
        // The final longer clause loses regardless of cost; it does not
        // depend on the shorter-by-one penalty clause.
        assert!(loses_to(
            &value(1, 3, 0),
            &value(LONG_SENTENCE_PENALTY, 2, 1)
        ));
        assert!(loses_to(
            &value(0, 3, 0),
            &value(LONG_SENTENCE_PENALTY, 2, 1)
        ));
    }

    #[test]
    fn the_comparator_cycle_requires_heap_selection() {
        let a = value(70_848, 4, 0);
        let b = value(69_469, 5, 0);
        let c = value(68_730, 6, 0);
        assert!(loses_to(&a, &b));
        assert!(loses_to(&b, &c));
        assert!(loses_to(&c, &a));
        assert!(!super::loses_to(&a, &b, 1), "nstore gate");
    }

    #[test]
    fn a_full_store_evicts_its_best_cost_and_preserves_slots() {
        let mut trellis = super::Trellis::with_seeds(1, &[0], NbestShape::PINYIN);
        for cost in [10, 20, 5] {
            trellis.insert(1, value(cost, 1, 0));
        }
        let costs: Vec<_> = trellis.nodes[1][&1].iter().map(|item| item.cost).collect();
        assert_eq!(costs, [5, 20], "pin discards 10, despite it beating 20");
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
        let matrix = build_scan_matrix(&graph, OptionBits::default(), true);
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
        let rows = nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::PINYIN)
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
        let rows: Vec<NbestRow> =
            nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::PINYIN)
                .expect("the trellis cannot fail here");
        assert!(!rows.is_empty());
        assert_eq!(rows[0].text.as_str(), "你好");
        assert_eq!(rows[0].span, bound);
        assert_eq!(rows[0].keys, 2);
    }

    /// libzhuyin's `PhoneticLookup<1, 1>` keeps one value per node and
    /// extracts one tail, where libpinyin's `<2, 3>` extracts up to three:
    /// two readings of `ni` give the pinyin shape two rows and the zhuyin
    /// shape exactly one; equal-cost heap ties can select different heads.
    #[test]
    fn the_zhuyin_shape_has_its_own_single_tail_heap_order() {
        const VOCAB: &str = "token=1\tkeys=ni\ttext=你\tunigram=1000\n\
                             token=2\tkeys=ni\ttext=尼\tunigram=900\n\
                             token=3\tkeys=hao\ttext=好\tunigram=900\n";
        let dict = FixtureDictionary::parse(VOCAB).expect("authored fixture");
        let model = Fixed {
            blended: Some(1_000),
            unigram: Some(2_000),
        };
        let (matrix, bound) = one_column_matrix("nihao");
        let pinyin = nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::PINYIN)
            .expect("the trellis cannot fail here");
        let zhuyin = nbest_sentences(&matrix, bound, &dict, &model, &[], None, NbestShape::ZHUYIN)
            .expect("the trellis cannot fail here");
        assert!(
            pinyin.len() >= 2,
            "libpinyin's shape keeps both readings: {pinyin:?}"
        );
        assert_eq!(
            zhuyin.len(),
            1,
            "libzhuyin's shape extracts one tail: {zhuyin:?}"
        );
        assert_eq!(pinyin[0].text, "你好");
        assert_eq!(zhuyin[0].text, "尼好", "one-store heap tie order");
        assert_eq!(NbestShape::default(), NbestShape::PINYIN);
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
