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

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hasher};

use compact_str::CompactString;
use oxpinyin_core::cost::{NAN_COST, NEG_INF_COST, POS_INF_COST, cost_add, cost_gt};
use oxpinyin_core::scoring::{ScoringError, expand_keys};
use oxpinyin_core::{
    ChewingKey, Completeness, Cost, Dictionary, LanguageModel, NbestSuccessors, PhraseEntry,
    PhraseIndexRanges, PhraseItemRead, PhraseToken, PronunciationRows, SyllableKey,
    TrellisPathMemo,
};
use smallvec::SmallVec;

use crate::constraint::{Cell, ConstraintStore, PhraseSpan};
use crate::error::EngineError;
use crate::session::matrix::ParsedMatrix;
use crate::session::{
    MAX_PHRASE_LENGTH, SCAN_EXPANSION_LIMIT, ScanKey, column_is_empty, training_readings,
};

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

/// A multiplicative hasher for the trellis's `u32` token keys.
///
/// These maps are only probed (`get`, `entry`), never iterated: the order of
/// a position's nodes is `node_order`, so the hash function cannot reach the
/// output. SipHash's per-lookup cost dominated the step expansion.
#[derive(Default, Clone, Copy)]
struct TokenHasher(u64);

impl Hasher for TokenHasher {
    fn write(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.write_u64(u64::from(byte));
        }
    }

    fn write_u32(&mut self, value: u32) {
        self.write_u64(u64::from(value));
    }

    fn write_u64(&mut self, value: u64) {
        self.0 = (self.0.rotate_left(5) ^ value).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

type TokenMap<V> = HashMap<u32, V, BuildHasherDefault<TokenHasher>>;

/// The trellis state over the remaining input's byte positions.
type NodeValues = SmallVec<[Value; NSTORE]>;

struct Trellis {
    /// The surface's `<nstore, nbest>`.
    shape: NbestShape,
    /// `nodes[position][token]` = the physical pinned heap slots.
    nodes: Vec<TokenMap<NodeValues>>,
    node_order: Vec<Vec<u32>>,
    /// Token → the key path its first step priced it over, as an offset
    /// and length into `text_keys`/`text_tones`; the phrase text is read
    /// through the dictionary when a result is emitted.
    texts: TokenMap<(u32, u32)>,
    text_keys: Vec<SyllableKey>,
    text_tones: Vec<u8>,
}

impl Trellis {
    /// Seeds step 0 with one zero-cost node per token — upstream's
    /// `fill_prefixes` (`phonetic_lookup.h:244-276`), which inserts
    /// every `m_prefixes` entry as an initial node at `log(1.0)`.
    /// `pinyin_guess_sentence_with_prefix` drives this with the prefix
    /// token list; the single-seed `new` stays the sentence path's law.
    fn with_seeds(bound: usize, seeds: &[u32], shape: NbestShape) -> Self {
        let mut nodes = vec![TokenMap::default(); bound + 1];
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
            texts: TokenMap::default(),
            text_keys: Vec::new(),
            text_tones: Vec::new(),
        }
    }

    /// 074a2219 phonetic_lookup_heap.h:56-81: a full heap replaces its
    /// front when the newcomer wins. At equal length the front is BEST,
    /// so reproducing the pin deliberately evicts the best stored value.
    fn insert(&mut self, position: usize, value: Value) {
        let nstore = self.shape.nstore();
        let node = match self.nodes[position].entry(value.token) {
            Entry::Occupied(node) => node.into_mut(),
            Entry::Vacant(slot) => {
                self.node_order[position].push(value.token);
                slot.insert(NodeValues::new())
            }
        };
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
    /// Fewer final-step values yield fewer tails; an empty final step
    /// never falls back to an earlier populated step (#594).
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

    /// The phrase text of `token`, read once per call through `cache`;
    /// `None` where the dictionary has no text for it.
    fn token_text<D>(
        &self,
        dictionary: &D,
        cache: &mut TokenMap<Option<CompactString>>,
        token: u32,
    ) -> Result<Option<CompactString>, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
    {
        if let Some(text) = cache.get(&token) {
            return Ok(text.clone());
        }
        let text = match self.texts.get(&token) {
            Some(&(offset, len)) => {
                let range = offset as usize..offset as usize + len as usize;
                match (
                    self.text_keys.get(range.clone()),
                    self.text_tones.get(range),
                ) {
                    (Some(keys), Some(tones)) => dictionary
                        .trellis_text(keys, tones, token)
                        .map_err(|error| {
                            EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
                        })?
                        .map(CompactString::from),
                    _ => None,
                }
            }
            None => None,
        };
        cache.insert(token, text.clone());
        Ok(text)
    }

    fn text<D>(
        &self,
        dictionary: &D,
        cache: &mut TokenMap<Option<CompactString>>,
        spans: &[(usize, u32)],
    ) -> Result<Option<CompactString>, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
    {
        let mut text = CompactString::const_new("");
        for (_, token) in spans {
            let Some(part) = self.token_text(dictionary, cache, *token)? else {
                return Ok(None);
            };
            text.push_str(&part);
        }
        Ok(Some(text))
    }
}

/// One ranged token a step priced: the key count of the path that spelled
/// it (captured during enumeration, before the path is unwound), the
/// phrase's length in characters and the entry's pronunciation possibility.
/// `path` indexes the span's [`SpanPaths`].
#[derive(Clone)]
struct SpanEntry {
    token: u32,
    path: u32,
    keys: u32,
    chars: u32,
    pronunciation: Option<(u64, u64)>,
}

/// A ranged token, priced on first use (`phonetic_lookup.h:646-684`: the
/// pin reads a phrase item only when a step prices it). `None` inside
/// the cell: the dictionary has no readable item for the token over its
/// path, so the token keeps its place in its range with no entry.
#[derive(Clone)]
struct LazySlot {
    path: u32,
    priced: OnceCell<Option<SpanEntry>>,
}

impl LazySlot {
    fn new(path: u32) -> Self {
        Self {
            path,
            priced: OnceCell::new(),
        }
    }
}

/// The key paths of one span search, flat: path `i` is
/// `keys[bounds[i].0..][..bounds[i].1]`, and likewise its tones.
/// `memos[i]` is what the dictionary keeps while it prices path `i`, gone
/// with the span search.
#[derive(Default)]
struct SpanPaths {
    keys: Vec<SyllableKey>,
    tones: Vec<u8>,
    bounds: Vec<(u32, u32)>,
    memos: Vec<TrellisPathMemo>,
}

impl SpanPaths {
    fn clear(&mut self) {
        self.keys.clear();
        self.tones.clear();
        self.bounds.clear();
        self.memos.clear();
    }

    fn push(&mut self, keys: &[SyllableKey], tones: &[u8]) -> u32 {
        let id = u32::try_from(self.bounds.len()).unwrap_or(u32::MAX);
        let offset = u32::try_from(self.keys.len()).unwrap_or(u32::MAX);
        self.keys.extend_from_slice(keys);
        self.tones.extend_from_slice(tones);
        self.bounds
            .push((offset, u32::try_from(keys.len()).unwrap_or(u32::MAX)));
        self.memos.push(TrellisPathMemo::default());
        id
    }

    fn memo(&self, id: u32) -> Option<&TrellisPathMemo> {
        self.memos.get(id as usize)
    }

    fn path(&self, id: u32) -> (&[SyllableKey], &[u8]) {
        let Some(&(offset, len)) = self.bounds.get(id as usize) else {
            return (&[], &[]);
        };
        let range = offset as usize..offset as usize + len as usize;
        (
            self.keys.get(range.clone()).unwrap_or(&[]),
            self.tones.get(range).unwrap_or(&[]),
        )
    }
}

/// A span search's ranges and the paths they were found on.
#[derive(Default)]
struct SpanRanges {
    ranges: PhraseIndexRanges<LazySlot>,
    paths: SpanPaths,
}

impl SpanRanges {
    fn clear(&mut self) {
        self.ranges.clear();
        self.paths.clear();
    }
}

/// Prices a [`LazySlot`] through the dictionary, once per slot.
struct Pricing<'a, D> {
    /// The span search's ranges.
    ranges: &'a PhraseIndexRanges<LazySlot>,
    dictionary: &'a D,
    paths: &'a SpanPaths,
}

impl<'a, D> Pricing<'a, D>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    /// `slot`'s entry for `token`; `None` when the dictionary reads no
    /// item for it. The answer is kept in the slot for the span's steps.
    fn price<'s>(
        &self,
        slot: &'s LazySlot,
        token: u32,
    ) -> Result<Option<&'s SpanEntry>, EngineError> {
        if let Some(priced) = slot.priced.get() {
            return Ok(priced.as_ref());
        }
        let (keys, tones) = self.paths.path(slot.path);
        let item = match self.paths.memo(slot.path) {
            Some(memo) => self
                .dictionary
                .trellis_item_on_path(keys, tones, token, memo),
            None => self.dictionary.trellis_item(keys, tones, token),
        }
        .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        let entry = item.map(|item| SpanEntry {
            token,
            path: slot.path,
            keys: keys.len().try_into().unwrap_or(u32::MAX),
            chars: item.chars,
            pronunciation: item.pronunciation,
        });
        Ok(slot.priced.get_or_init(|| entry).as_ref())
    }

    /// The first slot in walk order that spells `token` and prices.
    fn first_priced(&self, token: u32) -> Result<Option<&'a SpanEntry>, EngineError> {
        for (ranged, slot) in self.ranges.walk_tokens() {
            if ranged == token
                && let Some(entry) = self.price(slot, token)?
            {
                return Ok(Some(entry));
            }
        }
        Ok(None)
    }
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
    // One path scratch for every span search of the call.
    let mut scratch = GenerationPath::default();
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

        // Own rows only for this step's span walk. A later
        // step or guess reloads after training, store writes or mask_out.
        let mut costs = StepCosts::default();
        if let Some(Cell::OneStep { token, end, .. }) = cell_at(position).cloned() {
            // One span search to the fixed end, the forced token only —
            // `search_bigram2`/`search_unigram2`'s ONESTEP branches,
            // entered on any `SEARCH_OK` (`phonetic_lookup.h:775-788`) —
            // then move on: no widening, no other token starts there.
            if end > position && end <= bound {
                let mut found = SpanRanges::default();
                generation.search(position, end, dictionary, &mut scratch, &mut found)?;
                let pricing = Pricing {
                    ranges: &found.ranges,
                    dictionary,
                    paths: &found.paths,
                };
                let mut forced = PhraseIndexRanges::new();
                forced.convert(pricing.first_priced(token.value())?.map(|entry| {
                    let slot = LazySlot::new(entry.path);
                    let _ = slot.priced.set(Some(entry.clone()));
                    (entry.token, Some(slot))
                }));
                if found.ranges.iter().next().is_some() {
                    let forced = Pricing {
                        ranges: &forced,
                        ..pricing
                    };
                    expand_entries(
                        &forced,
                        &beam,
                        position..end,
                        model,
                        &mut costs,
                        &mut trellis,
                        Some(token.value()),
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
        widen_free_span(
            &env,
            constraints,
            position,
            &beam,
            &mut scratch,
            &mut costs,
            &mut trellis,
        )?;
        position += 1;
    }

    let span = bound;
    let mut rows = Vec::new();
    let mut texts = TokenMap::default();
    for tail in trellis.tails() {
        let spans = trellis.extract(&tail);
        let Some(text) = trellis.text(dictionary, &mut texts, &spans)? else {
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
                text: texts.get(token).cloned().flatten().unwrap_or_default(),
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
    /// One table search's ranges, drained into the span's arrays.
    ranges: PhraseIndexRanges<()>,
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
        path: &mut GenerationPath,
        into: &mut SpanRanges,
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
        // A failed search leaves the scratch mid-path.
        path.keys.clear();
        path.tones.clear();
        path.ranges.clear();
        let mut continued = false;
        // search_matrix appends every path's ranges, system table then user
        // table, to the per-library arrays in walk order.
        self.walk(start, end, dictionary, path, into, &mut continued)?;
        Ok(continued)
    }

    fn walk<D>(
        &self,
        start: usize,
        end: usize,
        dictionary: &D,
        path: &mut GenerationPath,
        into: &mut SpanRanges,
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
            *continued |= dictionary
                .trellis_ranges_continued(&path.keys, &path.tones, &mut path.ranges)
                .map_err(|error| {
                    EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
                })?;
            let id = into.paths.push(&path.keys, &path.tones);
            // Drains the scratch, keeping its capacity for the next path.
            into.ranges
                .append_from(&mut path.ranges, |()| LazySlot::new(id));
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
    scratch: &mut GenerationPath,
    costs: &mut StepCosts,
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
    let mut found = SpanRanges::default();
    while end <= env.bound && widening {
        if matches!(cell_at(end), Some(Cell::NoSearch { .. })) {
            // A span may not end inside a forced run.
            break;
        }
        found.clear();
        widening = env
            .generation
            .search(position, end, env.dictionary, scratch, &mut found)?;
        let pricing = Pricing {
            ranges: &found.ranges,
            dictionary: env.dictionary,
            paths: &found.paths,
        };
        expand_entries(
            &pricing,
            beam,
            position..end,
            env.model,
            costs,
            trellis,
            None,
        )?;
        end += 1;
    }
    Ok(())
}

/// The matrix a forcing is revalidated against: the pin's
/// `ForwardPhoneticConstraints::validate_constraint`
/// (`src/lookup/phonetic_lookup.cpp:120-165` at 074a2219) over the columns
/// of the current parse, with the separator zero keys the pin's
/// `fill_matrix` writes.
pub(crate) struct ConstraintSpelling<'a> {
    scan: &'a [Vec<ScanKey>],
    bound: usize,
    physical: bool,
    input: &'a [u8],
    parsed: OnceCell<ParsedMatrix>,
    /// The last item a read returned: the pin's one `PhraseItem item`.
    last_item: RefCell<Option<PronunciationRows>>,
}

impl<'a> ConstraintSpelling<'a> {
    /// `scan` is the parse's scan matrix, `bound` its parsed length and
    /// `input` the raw bytes it was built over.
    pub(crate) const fn new(
        scan: &'a [Vec<ScanKey>],
        bound: usize,
        physical: bool,
        input: &'a [u8],
    ) -> Self {
        Self {
            scan,
            bound,
            physical,
            input,
            parsed: OnceCell::new(),
            last_item: RefCell::new(None),
        }
    }

    /// Whether the forced `token` still spells over `[start, end)`:
    /// `compute_pronunciation_possibility` of the token's item over the
    /// span is not below `FLT_EPSILON` (`phonetic_lookup.cpp:161-164`).
    ///
    /// The pin searches no table. It walks the matrix columns from `start`
    /// to `end` (`phonetic_key_matrix.cpp:534-585`), one key per column
    /// step, and for each complete path as long as the phrase asks
    /// `PhraseItem::get_pronunciation_possibility` (`phrase_index.h:136-163`)
    /// over the item's stored pronunciations, summing the paths. An
    /// incomplete key compares equal to any final
    /// (`pinyin_compare_middle_and_final3`, `pinyin_phrase3.h:73-94`), so the
    /// walk never expands one and has no cap on how many completions an
    /// abbreviation stands for. An empty start or end column answers zero
    /// (`:593-596`).
    ///
    /// The drop test is the integer zero test: a path counts when some
    /// stored pronunciation matches it with a nonzero frequency
    /// (`matched > 0` over a nonzero total). It decides what the `f32`
    /// `< FLT_EPSILON` test does on the pinned data, where no per-token
    /// total reaches 2^23 (`docs/findings/upstream-divergences.md`, the
    /// `validate_constraint` entry).
    ///
    /// `text_len` is the forced phrase's character count; it only bounds
    /// the walk for a dictionary that keeps no phrase items
    /// ([`PhraseItemRead::Unsupported`]).
    ///
    /// # Errors
    ///
    /// Propagates the dictionary's backend failure.
    pub(crate) fn spells<D>(
        &self,
        dictionary: &D,
        start: usize,
        end: usize,
        token: PhraseToken,
        text_len: usize,
    ) -> Result<bool, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
    {
        let dictionary_error =
            |error: D::Error| EngineError::Scoring(ScoringError::Dictionary(error.to_string()));
        // `PhraseItem item;` is declared once before the pin's loop
        // (`phonetic_lookup.cpp:141`) and `get_phrase_item` leaves it alone
        // when it fails (`phrase_index.h:649-650`, `phrase_index.cpp:189-193`):
        // a token with no readable item is checked against the item the
        // last successful read left, and against a zero-length one only
        // when no read has succeeded yet. The read comes before the
        // overrun test (`:154`), so a forcing that is then cleared for
        // overrunning still leaves its item behind.
        let read = dictionary
            .phrase_item_pronunciations(token)
            .map_err(dictionary_error)?;
        let item_less = matches!(read, PhraseItemRead::Unsupported);
        let mut last = self.last_item.borrow_mut();
        if let PhraseItemRead::Item(rows) = read {
            *last = Some(rows);
        }
        let parsed = self
            .parsed
            .get_or_init(|| ParsedMatrix::from_scan(self.scan, self.bound, self.physical));
        let columns = &parsed.columns;
        if end >= columns.len()
            || column_is_empty(columns, &parsed.zeros, start)
            || column_is_empty(columns, &parsed.zeros, end)
        {
            return Ok(false);
        }
        if item_less {
            for reading in training_readings(columns, self.input, start, end, text_len) {
                if lookup_finds_token(dictionary, &reading, token)? {
                    return Ok(true);
                }
            }
            return Ok(false);
        }
        let Some(rows) = last.as_ref() else {
            return Ok(false);
        };
        // `phrase_length`: every stored pronunciation spells it.
        let Some(length) = rows.first().map(|(keys, _)| keys.len()) else {
            return Ok(false);
        };
        // `total_freq` is read-independent; the pin's guard returns zero
        // for an item whose frequencies sum to zero.
        let total = rows.iter().fold(0_u32, |total, (_, frequency)| {
            total.wrapping_add(*frequency)
        });
        if total == 0 {
            return Ok(false);
        }
        for reading in training_readings(columns, self.input, start, end, length) {
            let matched = rows
                .iter()
                .filter(|(stored, _)| reading_matches(&reading, stored))
                .fold(0_u32, |matched, (_, frequency)| {
                    matched.wrapping_add(*frequency)
                });
            if matched != 0 {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

/// `pinyin_compare_with_tones(query, stored, phrase_length) == 0`
/// (`src/storage/pinyin_phrase3.h:108-144` at 074a2219), key by key
/// (the three passes only reorder which difference is reported): initials
/// equal (`:68-71`); middle and final equal, or either side the
/// incomplete `(ZERO_MIDDLE, ZERO_FINAL)` pair (`:73-94`, the incomplete
/// test at `:80-86`); tones equal, or either side zero (`:96-105`).
fn reading_matches(query: &[ChewingKey], stored: &[ChewingKey]) -> bool {
    query.len() == stored.len()
        && query.iter().zip(stored).all(|(q, s)| {
            q.initial == s.initial
                && ((q.middle == s.middle && q.final_ == s.final_)
                    || (q.middle == 0 && q.final_ == 0)
                    || (s.middle == 0 && s.final_ == 0))
                && (q.tone == s.tone || q.tone == 0 || s.tone == 0)
        })
}

/// [`ConstraintSpelling::spells`] for a dictionary that keeps no phrase
/// items: the token is searched under `reading`, an incomplete key standing
/// for its bounded completions, and kept unless its possibility is the
/// zero count. Every shipping dictionary reads its items instead.
fn lookup_finds_token<D>(
    dictionary: &D,
    reading: &[ChewingKey],
    token: PhraseToken,
) -> Result<bool, EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
{
    let Some(path) = reading
        .iter()
        .map(|key| SyllableKey::from_canonical_text(key.pinyin_spelling()))
        .collect::<Option<SmallVec<[SyllableKey; 16]>>>()
    else {
        return Ok(false);
    };
    let has_incomplete = path
        .iter()
        .any(|key| key.completeness() == Completeness::Partial);
    let mut found = false;
    let mut look = |sequence: &[SyllableKey]| -> Result<(), EngineError> {
        let entries = dictionary
            .lookup(sequence)
            .map_err(|error| EngineError::Scoring(ScoringError::Dictionary(error.to_string())))?;
        found |= entries.iter().any(|entry| {
            entry.token() == token && !matches!(entry.pronunciation_possibility(), Some((0, _)))
        });
        Ok(())
    };
    if has_incomplete && !dictionary.handles_partial_keys() {
        for sequence in expand_keys(&path, SCAN_EXPANSION_LIMIT) {
            look(sequence.as_slice())?;
        }
    } else {
        look(&path)?;
    }
    Ok(found)
}

#[derive(Default)]
struct StepCosts {
    rows: TokenMap<oxpinyin_core::NbestStepRow>,
}

/// 074a2219 phonetic_lookup.h:542-642, 787-788: all bigram
/// predecessors first, each over library/range/item order, then head unigram.
///
/// `one_step` is a `CONSTRAINT_ONESTEP` cell's forced token: the bigram
/// half then reads it with `get_freq` alone (`:600-611`), never `search`.
///
/// # Errors
///
/// The model's load failures, and a lookup refusal where the pin's read
/// of a predecessor's merged row asserts or leaves the value.
fn expand_entries<D, L>(
    pricing: &Pricing<'_, D>,
    beam: &[BeamEntry],
    span: std::ops::Range<usize>,
    model: &L,
    costs: &mut StepCosts,
    trellis: &mut Trellis,
    one_step: Option<u32>,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
    D::Error: core::fmt::Display,
    L: LanguageModel<Token = PhraseToken>,
    L::Error: core::fmt::Display,
{
    let entries = pricing.ranges;
    let eligible = |entry: &SpanEntry| !matches!(entry.pronunciation, Some((0, _)));
    // Entered on `SEARCH_OK` only; the predecessors' rows are loaded and
    // merged then even when no ranged item can step.
    if one_step.is_none() && entries.iter().next().is_none() {
        return Ok(());
    }
    // search_bigram2 (phonetic_lookup.h:582-641): per predecessor, load
    // and merge its grams (both absent: no bigram step), then per library
    // and range in append order `SingleGram::search` (ngram.cpp:153-176):
    // from `lower_bound(range_begin)`, one step per yielded item with that
    // item's own frequency, duplicates included.
    let mut items: Vec<(u32, u32)> = Vec::new();
    let mut selected: Vec<(&SpanEntry, (u32, u32))> = Vec::new();
    for predecessor in beam {
        selected.clear();
        let prev = predecessor.value.token;
        let row = costs.rows.entry(prev).or_default();
        let traversal = match model
            .nbest_successors_in_row(&PhraseToken::new(prev), row)
            .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))?
        {
            None => false,
            Some(NbestSuccessors::Miss) => true,
            Some(NbestSuccessors::Row(gram)) => {
                let refuse = |fault| EngineError::lookup_gram(prev, fault);
                if let Some(fault) = gram.fault() {
                    return Err(refuse(fault));
                }
                if let Some(token) = one_step {
                    if let Some(freq) = gram.get_freq(token).map_err(refuse)? {
                        let total = gram.total().map_err(refuse)?;
                        for (ranged, slot) in entries.walk_tokens() {
                            if ranged != token {
                                continue;
                            }
                            if let Some(entry) = pricing.price(slot, ranged)?
                                && eligible(entry)
                            {
                                selected.push((entry, (freq, total)));
                            }
                        }
                    }
                } else {
                    for range in entries.iter() {
                        items.clear();
                        gram.search_into(range.first(), range.last(), &mut items)
                            .map_err(refuse)?;
                        if items.is_empty() {
                            continue;
                        }
                        let total = gram.total().map_err(refuse)?;
                        // The walk checks the range end only (ngram.cpp:167-
                        // 173): out of order, it can yield a token below the
                        // range's first. The pin steps any yielded token whose
                        // phrase spells this span (`bigram_gen_next_step`'s
                        // `get_phrase_item` and pronunciation check), so such
                        // a token takes this span's entry for it, if any.
                        for &(token, freq) in &items {
                            let own = match entries.entry(range, token) {
                                Some(slot) => pricing.price(slot, token)?,
                                None => None,
                            };
                            let found = match own {
                                Some(entry) => Some(entry),
                                None => pricing.first_priced(token)?,
                            };
                            if let Some(entry) = found
                                && eligible(entry)
                            {
                                selected.push((entry, (freq, total)));
                            }
                        }
                    }
                }
                true
            }
        };
        if traversal {
            for &(entry, item) in &selected {
                let branch = Branch::Bigram(Some(item));
                expand_one(
                    entry,
                    StepCtx::new(pricing.paths, span.clone(), model),
                    predecessor,
                    row,
                    trellis,
                    branch,
                )?;
            }
        } else {
            // Models without row traversal retain their existing scalar path.
            for (token, slot) in entries.walk_tokens() {
                let Some(entry) = pricing.price(slot, token)?.filter(|entry| eligible(entry))
                else {
                    continue;
                };
                let branch = Branch::Bigram(None);
                expand_one(
                    entry,
                    StepCtx::new(pricing.paths, span.clone(), model),
                    predecessor,
                    row,
                    trellis,
                    branch,
                )?;
            }
        }
    }
    // The pin's independent unigram pass uses only the best predecessor
    // (search_unigram2, phonetic_lookup.h:560-572).
    if let Some(predecessor) = beam.first() {
        let row = costs.rows.entry(predecessor.value.token).or_default();
        for (token, slot) in entries.walk_tokens() {
            let Some(entry) = pricing.price(slot, token)?.filter(|entry| eligible(entry)) else {
                continue;
            };
            let branch = Branch::Unigram;
            expand_one(
                entry,
                StepCtx::new(pricing.paths, span.clone(), model),
                predecessor,
                row,
                trellis,
                branch,
            )?;
        }
    }
    Ok(())
}

/// Which of the pin's two step costs one trellis step takes.
#[derive(Clone, Copy)]
enum Branch {
    /// `bigram_gen_next_step` for one merged-row item, its `(freq, total)`;
    /// `None` on the scalar path of a model without row traversal.
    Bigram(Option<(u32, u32)>),
    /// `unigram_gen_next_step`.
    Unigram,
}

/// What a trellis step reads besides its entry and predecessor.
struct StepCtx<'a, L> {
    paths: &'a SpanPaths,
    span: std::ops::Range<usize>,
    model: &'a L,
}

impl<'a, L> StepCtx<'a, L> {
    const fn new(paths: &'a SpanPaths, span: std::ops::Range<usize>, model: &'a L) -> Self {
        Self { paths, span, model }
    }
}

/// One trellis step on `branch`.
fn expand_one<L: LanguageModel<Token = PhraseToken>>(
    entry: &SpanEntry,
    ctx: StepCtx<'_, L>,
    predecessor: &BeamEntry,
    row: &mut oxpinyin_core::NbestStepRow,
    trellis: &mut Trellis,
    branch: Branch,
) -> Result<(), EngineError>
where
    L::Error: core::fmt::Display,
{
    let StepCtx { paths, span, model } = ctx;
    let prev = predecessor.value.token;
    let step = model
        .nbest_item_costs_in_row(
            &PhraseToken::new(prev),
            &PhraseToken::new(entry.token),
            match branch {
                Branch::Bigram(item) => item,
                Branch::Unigram => None,
            },
            row,
        )
        .map_err(|error| EngineError::Scoring(ScoringError::LanguageModel(error.to_string())))?;
    let cost = match branch {
        Branch::Bigram(_) => step.blended,
        Branch::Unigram => step.unigram,
    };
    let Some(mut cost) = cost else {
        return Ok(());
    };
    if let Some((matched, total)) = entry.pronunciation
        && matched > 0
        && total > 0
    {
        cost = cost_add(cost, oxpinyin_core::cost::surprisal(matched, total));
    }
    if let Entry::Vacant(slot) = trellis.texts.entry(entry.token) {
        // The first step to price the token names the path whose text
        // a result reads.
        let (keys, tones) = paths.path(entry.path);
        let offset = u32::try_from(trellis.text_keys.len()).unwrap_or(u32::MAX);
        trellis.text_keys.extend_from_slice(keys);
        trellis.text_tones.extend_from_slice(tones);
        slot.insert((offset, u32::try_from(keys.len()).unwrap_or(u32::MAX)));
    }
    let chars = entry.chars;
    trellis.insert(
        span.end,
        Value {
            token: entry.token,
            prev_token: prev,
            from: span.start,
            sub: predecessor.slot,
            length: predecessor.value.length.saturating_add(chars),
            keys: predecessor.value.keys.saturating_add(entry.keys),
            cost: cost_add(predecessor.value.cost, cost),
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{LONG_SENTENCE_PENALTY, NbestRow, NbestShape, Trellis, Value, nbest_sentences};
    use crate::session::ScanKey;
    use oxpinyin_core::graph::SegmentGraph;
    use oxpinyin_core::{Cost, LanguageModel, NbestStepCosts, OptionBits, PhraseToken};
    use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

    use crate::config::EmptyConfigSource;
    use crate::error::EngineError;
    use crate::session::build_scan_matrix;
    use crate::storage::StoragePaths;

    /// A slot whose pricing is already known, so no dictionary is read.
    fn priced_slot(token: u32) -> super::LazySlot {
        let slot = super::LazySlot::new(0);
        slot.priced
            .set(Some(super::SpanEntry {
                token,
                path: 0,
                keys: 1,
                chars: 1,
                pronunciation: None,
            }))
            .ok();
        slot
    }

    fn pricing(
        ranges: &oxpinyin_core::PhraseIndexRanges<super::LazySlot>,
    ) -> super::Pricing<'_, FixtureDictionary> {
        static DICTIONARY: std::sync::OnceLock<FixtureDictionary> = std::sync::OnceLock::new();
        // The path memos are not `Sync`, so no static can hold the paths.
        super::Pricing {
            ranges,
            dictionary: DICTIONARY.get_or_init(FixtureDictionary::default),
            paths: Box::leak(Box::default()),
        }
    }

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
    fn tails_do_not_fall_back_from_an_empty_final_step() {
        for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
            let mut trellis = Trellis::with_seeds(2, &[0], shape);
            for token in 1..=3 {
                trellis.insert(
                    1,
                    Value {
                        token,
                        ..value(1_000, 1, 0)
                    },
                );
            }
            assert_eq!(trellis.candidates(1).len(), 3);
            assert!(trellis.tails().is_empty());
        }
    }

    #[test]
    fn tails_keep_only_the_underfilled_final_step() {
        let mut trellis = Trellis::with_seeds(2, &[0], NbestShape::PINYIN);
        for token in 1..=3 {
            trellis.insert(
                1,
                Value {
                    token,
                    ..value(1_000, 1, 0)
                },
            );
        }
        for token in 4..=5 {
            trellis.insert(
                2,
                Value {
                    token,
                    ..value(2_000, 2, 0)
                },
            );
        }
        let mut tokens: Vec<_> = trellis.tails().iter().map(|tail| tail.token).collect();
        tokens.sort_unstable();
        assert_eq!(tokens, [4, 5]);
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

    #[test]
    fn predecessor_rows_are_reused_only_inside_each_step_and_guess() {
        use oxpinyin_core::{MergedGram, NbestStepRow};
        use std::cell::RefCell;
        struct Counting(RefCell<Vec<u32>>);
        impl LanguageModel for Counting {
            type Token = PhraseToken;
            type Error = EngineError;
            fn score(
                &self,
                _: &[PhraseToken],
                _: &PhraseToken,
                cost: Cost,
            ) -> Result<Cost, EngineError> {
                Ok(cost)
            }
            fn nbest_step_costs_in_row(
                &self,
                prev: &PhraseToken,
                _: &PhraseToken,
                row: &mut NbestStepRow,
            ) -> Result<NbestStepCosts, EngineError> {
                row.get_or_try_init::<EngineError>(|| {
                    self.0.borrow_mut().push(prev.value());
                    Ok(Some(MergedGram::new(2, vec![(1, 1), (2, 1)])))
                })?;
                Ok(NbestStepCosts {
                    blended: Some(1000),
                    unigram: Some(2000),
                })
            }
        }
        let dict = FixtureDictionary::parse(
            "token=1\tkeys=ni\ttext=你\tunigram=1000\ntoken=2\tkeys=ni\ttext=尼\tunigram=900\n",
        )
        .expect("authored fixture");
        let (matrix, bound) = one_column_matrix("nini");
        let model = Counting(RefCell::new(Vec::new()));
        // Passing &model exercises the blanket &L forwarding too.
        for run in 1..=2 {
            let rows = nbest_sentences(
                &matrix,
                bound,
                &dict,
                &&model,
                &[PhraseToken::new(1)],
                None,
                NbestShape::PINYIN,
            )
            .unwrap();
            assert!(!rows.is_empty());
            let loads = model.0.borrow();
            assert_eq!(loads.iter().filter(|prev| **prev == 1).count(), 2 * run);
            assert_eq!(loads.iter().filter(|prev| **prev == 2).count(), run);
        }
    }

    #[test]
    fn sparse_bigram_walk_skips_absent_tokens_and_keeps_span_order() {
        use oxpinyin_core::{MergedGram, NbestStepRow};
        use std::cell::RefCell;

        struct Sparse(RefCell<Vec<u32>>);
        impl LanguageModel for Sparse {
            type Token = PhraseToken;
            type Error = EngineError;

            fn score(
                &self,
                _: &[PhraseToken],
                _: &PhraseToken,
                cost: Cost,
            ) -> Result<Cost, EngineError> {
                Ok(cost)
            }

            fn nbest_successors_in_row<'a>(
                &self,
                _: &PhraseToken,
                row: &'a mut NbestStepRow,
            ) -> Result<Option<oxpinyin_core::NbestSuccessors<'a>>, EngineError> {
                Ok(Some(
                    row.get_or_try_init::<EngineError>(|| {
                        Ok(Some(MergedGram::new(2, vec![(1, 2), (3, 0)])))
                    })?
                    .map_or(
                        oxpinyin_core::NbestSuccessors::Miss,
                        oxpinyin_core::NbestSuccessors::Row,
                    ),
                ))
            }

            fn nbest_step_costs_in_row(
                &self,
                _: &PhraseToken,
                token: &PhraseToken,
                _: &mut NbestStepRow,
            ) -> Result<NbestStepCosts, EngineError> {
                self.0.borrow_mut().push(token.value());
                Ok(NbestStepCosts {
                    blended: Some(1000),
                    unigram: Some(2000),
                })
            }
        }

        let mut entries = oxpinyin_core::PhraseIndexRanges::new();
        entries.convert([3, 2, 1, 3].map(|token| (token, Some(priced_slot(token)))));
        let model = Sparse(RefCell::new(Vec::new()));
        let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
        let beam = trellis.beam(0);
        super::expand_entries(
            &pricing(&entries),
            &beam,
            0..1,
            &model,
            &mut super::StepCosts::default(),
            &mut trellis,
            None,
        )
        .unwrap();
        // Bigram visits present 3,1,3 in the original range/item order;
        // the zero-count record 3 still participates. The independent head
        // unigram pass then visits every candidate, including absent 2.
        assert_eq!(*model.0.borrow(), [3, 1, 3, 3, 2, 1, 3]);
    }

    /// N2 refusals 1, 2, 4 and 6 in the n-best step
    /// (`phonetic_lookup.h:594-632`): the predecessor's merge asserts on a
    /// short user value beside a system gram; with no system gram,
    /// `search` asserts on its total, a one-step constraint's `get_freq`
    /// reads past it, and a walk past the last whole item reads past a
    /// partial tail. A walk that stops before the tail steps normally.
    #[test]
    fn a_predecessor_row_the_pin_cannot_read_refuses_the_step() {
        use oxpinyin_core::{MergedGram, NbestStepRow, NbestSuccessors};

        struct Rows(Option<MergedGram>);
        impl LanguageModel for Rows {
            type Token = PhraseToken;
            type Error = EngineError;

            fn score(
                &self,
                _: &[PhraseToken],
                _: &PhraseToken,
                cost: Cost,
            ) -> Result<Cost, EngineError> {
                Ok(cost)
            }

            fn nbest_successors_in_row<'a>(
                &self,
                _: &PhraseToken,
                row: &'a mut NbestStepRow,
            ) -> Result<Option<NbestSuccessors<'a>>, EngineError> {
                Ok(Some(
                    row.get_or_try_init::<EngineError>(|| Ok(self.0.clone()))?
                        .map_or(NbestSuccessors::Miss, NbestSuccessors::Row),
                ))
            }

            fn nbest_item_costs_in_row(
                &self,
                _: &PhraseToken,
                _: &PhraseToken,
                item: Option<(u32, u32)>,
                _: &mut NbestStepRow,
            ) -> Result<NbestStepCosts, EngineError> {
                Ok(NbestStepCosts {
                    blended: item.map(|(freq, _)| Cost::from(freq)),
                    unigram: Some(2000),
                })
            }
        }

        let gram = |total: u32, items: &[(u32, u32)], tail: &[u8]| {
            let mut value = total.to_le_bytes().to_vec();
            for (token, freq) in items {
                value.extend_from_slice(&token.to_le_bytes());
                value.extend_from_slice(&freq.to_le_bytes());
            }
            value.extend_from_slice(tail);
            value
        };
        let mut entries = oxpinyin_core::PhraseIndexRanges::new();
        entries.convert([1, 2].map(|token| (token, Some(priced_slot(token)))));
        let run = |row: Option<MergedGram>, one_step: Option<u32>| {
            let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
            let beam = trellis.beam(0);
            super::expand_entries(
                &pricing(&entries),
                &beam,
                0..1,
                &Rows(row),
                &mut super::StepCosts::default(),
                &mut trellis,
                one_step,
            )
        };
        let short = EngineError::ShortLookupPredecessor { token: 11 };
        let unsafe_read = EngineError::UnsafeLookupPredecessor { token: 11 };
        let merged = MergedGram::merge(Some(gram(1, &[(1, 1)], &[])), Some(vec![0, 0]));
        assert_eq!(run(merged, None), Err(short.clone()));
        let alone = || MergedGram::merge(None, Some(vec![0, 0]));
        assert_eq!(run(alone(), None), Err(short));
        assert_eq!(run(alone(), Some(1)), Err(unsafe_read.clone()));
        let tailed = || MergedGram::merge(None, Some(gram(3, &[(1, 3)], &[9])));
        assert_eq!(run(tailed(), None), Err(unsafe_read));
        // `get_freq(1)` returns at the whole item before the tail.
        assert_eq!(run(tailed(), Some(1)), Ok(()));
        assert_eq!(run(None, None), Ok(()), "both grams absent: no bigram step");
    }

    /// `SingleGram::search` bounds its walk by the range end only
    /// (`ngram.cpp:167-173`). On the row `[1, 3, 2]` the walk over range
    /// `[3]` starts at 3 and yields 2 too, below that range; the pin steps
    /// every yielded token whose phrase spells the span, with that item's
    /// own count, so 2 steps there with 7.
    #[test]
    fn a_token_the_walk_yields_below_its_range_still_steps() {
        use oxpinyin_core::{MergedGram, NbestStepRow, NbestSuccessors};
        use std::cell::RefCell;

        /// Each bigram step's token and the item it was priced from.
        type Steps = Vec<(u32, Option<(u32, u32)>)>;
        struct Walked(RefCell<Steps>);
        impl LanguageModel for Walked {
            type Token = PhraseToken;
            type Error = EngineError;

            fn score(
                &self,
                _: &[PhraseToken],
                _: &PhraseToken,
                cost: Cost,
            ) -> Result<Cost, EngineError> {
                Ok(cost)
            }

            fn nbest_successors_in_row<'a>(
                &self,
                _: &PhraseToken,
                row: &'a mut NbestStepRow,
            ) -> Result<Option<NbestSuccessors<'a>>, EngineError> {
                Ok(Some(
                    row.get_or_try_init::<EngineError>(|| {
                        Ok(Some(MergedGram::new(15, vec![(1, 1), (3, 5), (2, 7)])))
                    })?
                    .map_or(NbestSuccessors::Miss, NbestSuccessors::Row),
                ))
            }

            fn nbest_item_costs_in_row(
                &self,
                _: &PhraseToken,
                token: &PhraseToken,
                item: Option<(u32, u32)>,
                _: &mut NbestStepRow,
            ) -> Result<NbestStepCosts, EngineError> {
                if item.is_some() {
                    self.0.borrow_mut().push((token.value(), item));
                }
                Ok(NbestStepCosts {
                    blended: Some(1000),
                    unigram: Some(2000),
                })
            }
        }

        let mut entries = oxpinyin_core::PhraseIndexRanges::new();
        entries.convert([1, 3, 2].map(|token| (token, Some(priced_slot(token)))));
        let model = Walked(RefCell::new(Vec::new()));
        let mut trellis = super::Trellis::with_seeds(1, &[11], NbestShape::PINYIN);
        let beam = trellis.beam(0);
        super::expand_entries(
            &pricing(&entries),
            &beam,
            0..1,
            &model,
            &mut super::StepCosts::default(),
            &mut trellis,
            None,
        )
        .unwrap();
        // Range [1] yields 1; range [3] yields 3 and then 2; range [2]
        // starts past 2 and yields nothing.
        assert_eq!(
            *model.0.borrow(),
            [(1, Some((1, 15))), (3, Some((5, 15))), (2, Some((7, 15)))]
        );
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
