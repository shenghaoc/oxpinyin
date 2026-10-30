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
//! adjusted float score and preselection batch position for pinyin, under
//! S1/S2 of the compatibility policy's "witnessed algorithm-precondition
//! violations (2026-10-28 UTC)" amendment. Zhuyin selection uses length,
//! raw possibility and batch position under the 2026-10-29 ruling;
//! single-slot node admission keeps the pin's raw comparison.
//! Spans widen exactly while the phrase
//! table reports a longer phrase could still start (`SEARCH_CONTINUED`),
//! the same prefix-driven widening the window scan uses.
//!
//! Scoring is the upstream per-step form
//! `log((λ·bigram + (1−λ)·unigram) · pinyin_poss)` accumulated in f32,
//! with pre-log pieces from
//! [`oxpinyin_core::LanguageModel::nbest_step_costs`]. The bigram branch
//! expands every beam value; the unigram branch expands only the beam's
//! head, as upstream's `search_bigram2` / `search_unigram2` do.
//! Pronunciation uses the first matched matrix path; see
//! `docs/findings/sentence-surface.md` §3 for the recorded divergences
//! (first-path-only pronunciation).

use std::cell::{OnceCell, RefCell};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::hash::{BuildHasherDefault, Hasher};

use oxpinyin_core::PhraseText;
use oxpinyin_core::graph::{GraphError, MAX_GRAPH_INPUT};
use oxpinyin_core::scoring::{ScoringError, expand_keys};
use oxpinyin_core::{
    ChewingKey, Completeness, Cost, Dictionary, LanguageModel, NbestSuccessors, PhraseEntry,
    PhraseIndexRanges, PhraseItemRead, PhraseToken, PronunciationRows, SyllableKey,
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

/// The sentence-start seed token (`novel_types.h`: `sentence_start = 1`).
pub const SENTENCE_START: u32 = 1;

/// One decoded sentence row, in tail order (index 0 is the 1-best).
#[derive(Clone, Debug)]
pub struct NbestRow {
    /// Concatenated phrase texts.
    pub(crate) text: PhraseText,
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
    /// Cost of a pre-frequency fallback row; zero for real-table rows.
    pub(crate) cost: Cost,
    /// Real-table accumulated log possibility; absent for fallback rows.
    pub(crate) poss: Option<f32>,
}

/// `m_last_step` of the seed: the pin's `-1` (074a2219 `phonetic_lookup.h:59`).
const SEED_STEP: u32 = u32::MAX;

/// One hypothesis in the trellis, at the pin's `trellis_value_t` field
/// widths (074a2219 phonetic_lookup.h:41-52) wherever every value fits.
#[derive(Clone, Copy, Debug)]
struct Value {
    /// Phrase token ending at this step (`m_handles[1]`, `phrase_token_t`).
    token: u32,
    /// Token of the phrase the predecessor value ended on (`m_handles[0]`).
    prev_token: u32,
    /// Position the covering phrase starts at (`m_last_step`, a `gint32`
    /// step); [`SEED_STEP`] marks the seed. A step is a byte position of a
    /// graph's input, at most `MAX_GRAPH_INPUT`.
    from: u32,
    /// Slot of the predecessor value in node `(from, prev_token)`
    /// (`m_sub_index`, a `gint32` below `nstore`).
    sub: u32,
    /// Accumulated phrase characters (`m_sentence_length`, `gint32`).
    length: u32,
    /// Accumulated keys covered (the candidate's `consumed_keys`).
    keys: u32,
    /// Accumulated log possibility, rounded to f32 after every step.
    poss: f32,
}

impl Value {
    const fn seed(token: u32) -> Self {
        Self {
            token,
            prev_token: 0,
            from: SEED_STEP,
            sub: 0,
            length: 0,
            keys: 0,
            poss: 0.0,
        }
    }
}

/// One beam member: the value plus its slot inside its own node, which is
/// what a successor records as its backtracking `sub` (upstream's
/// `number()` assigns `m_current_index` before the beam is built).
#[derive(Clone, Copy, Debug)]
struct BeamEntry {
    value: Value,
    slot: u32,
    /// Index in the complete candidate batch, before selection.
    /// Independent of `slot`, which continues to identify the predecessor.
    batch_position: usize,
}

/// S1/S2: two f32 roundings, without contraction or widening. The penalty
/// is the pin's `LONG_SENTENCE_PENALTY`, `log(1.2f)`
/// (`phonetic_lookup.h:39`), in Rust's `f32` maths (class (a)).
fn adjusted_poss(value: &Value) -> f32 {
    value.poss - value.length as f32 * 1.2_f32.ln()
}

/// Best first: +infinity, finite descending, -infinity, NaN last.
/// Signed zeros tie; NaN payload and sign never affect ranking.
fn ordered_poss_compare(left: f32, right: f32) -> Ordering {
    match (left.is_nan(), right.is_nan()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => right.partial_cmp(&left).unwrap_or(Ordering::Equal),
    }
}

/// The pinyin node uses adjusted score alone: a tying newcomer cannot evict its
/// front. The two-slot front-eviction algorithm itself remains the pin's.
fn node_loses_to(left: &Value, right: &Value, nstore: usize) -> bool {
    if nstore == 1 {
        return zhuyin_loses_to(left, right);
    }
    ordered_poss_compare(adjusted_poss(left), adjusted_poss(right)).is_gt()
}

/// 074a2219 phonetic_lookup.h:66-91 with `nstore = 1`, unchanged.
/// NaN suppresses the possibility comparison; the length clause still applies.
fn zhuyin_loses_to(left: &Value, right: &Value) -> bool {
    (left.length == right.length && left.poss < right.poss) || left.length > right.length
}

/// S1/S2 selection key: adjusted score, then earlier batch position. The
/// position is assigned before selection and never participates in insertion.
fn selection_compare(left: &BeamEntry, right: &BeamEntry, nstore: usize) -> Ordering {
    if nstore == 1 {
        return left
            .value
            .length
            .cmp(&right.value.length)
            .then_with(|| ordered_poss_compare(left.value.poss, right.value.poss))
            .then_with(|| left.batch_position.cmp(&right.batch_position));
    }
    ordered_poss_compare(adjusted_poss(&left.value), adjusted_poss(&right.value))
        .then_with(|| left.batch_position.cmp(&right.batch_position))
}

/// Select on the complete key, then order just the kept prefix. Batch
/// positions are unique, so unstable selection cannot reorder equal keys.
/// O(m + k log k) time and no allocation beyond the candidate batch.
fn select_top(mut values: Vec<BeamEntry>, count: usize, nstore: usize) -> Vec<BeamEntry> {
    let count = count.min(values.len());
    if count == 0 {
        values.clear();
        return values;
    }
    let compare = |left: &BeamEntry, right: &BeamEntry| selection_compare(left, right, nstore);
    if count < values.len() {
        values.select_nth_unstable_by(count - 1, compare);
        values.truncate(count);
    }
    values.sort_unstable_by(compare);
    values
}

/// S3: stably order the selected tails by raw possibility descending
/// (`phonetic_lookup.h:174-178,337 at 074a2219`), without integer truncation.
/// Adjusted S is selection-only; raw possibility orders the final tails.
fn tail_merge(values: &mut [Value]) {
    values.sort_by(|left, right| ordered_poss_compare(left.poss, right.poss));
}

/// A multiplicative hasher for the trellis's `u32` token keys.
///
/// These maps are only probed (`get`, `entry`), never iterated: the order of
/// a position's nodes is its [`Step`]'s node vector, so the hash function
/// cannot reach the output. SipHash's per-lookup cost dominated the step expansion.
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

/// One `(position, token)` node: the pin's `trellis_node<nstore>`
/// (074a2219 phonetic_lookup_heap.h:31-82), a fixed array whose first
/// `len` values occupy their physical backtracking slots. The rest are unused.
#[derive(Clone, Copy, Debug)]
struct Node {
    /// `m_nelem`, at most `nstore`.
    len: u32,
    /// `m_elements`, sized for the larger shape's `nstore`.
    values: [Value; NSTORE],
}

impl Node {
    const EMPTY: Self = Self {
        len: 0,
        values: [Value::seed(0); NSTORE],
    };

    /// The stored values in their physical backtracking slots.
    fn values(&self) -> &[Value] {
        self.values.get(..self.len as usize).unwrap_or_default()
    }

    /// 074a2219 phonetic_lookup_heap.h:56-81: a full two-slot node
    /// evicts its front only for a strictly better newcomer. The ordered
    /// pinyin key makes that newcomer better than the back too.
    fn keep(&mut self, value: Value, nstore: usize) {
        match self.len {
            0 => {
                self.values[0] = value;
                self.len = 1;
            }
            1 if nstore == NSTORE => {
                if node_loses_to(&self.values[0], &value, nstore) {
                    self.values[1] = self.values[0];
                    self.values[0] = value;
                } else {
                    self.values[1] = value;
                }
                self.len = 2;
            }
            _ if node_loses_to(&self.values[0], &value, nstore) => {
                self.values[0] = value;
            }
            _ => {}
        }
    }
}

/// One position's nodes, laid out as the pin's per-step pair
/// (074a2219 phonetic_lookup.h:184-186, filled by `insert_candidate` at
/// :300-330): `m_steps_index`, a token → node-index `GHashTable`, and
/// `m_steps_content`, a `GArray` of nodes in creation order.
#[derive(Clone, Default)]
struct Step {
    index: TokenMap<u32>,
    nodes: Vec<Node>,
}

impl Step {
    fn node(&self, token: u32) -> Option<&Node> {
        let index = *self.index.get(&token)?;
        self.nodes.get(index as usize)
    }

    /// The node for `token`, appended in creation order when it is new
    /// (`g_array_append_val` + `g_hash_table_insert`, :314-315).
    fn node_mut(&mut self, token: u32) -> Option<&mut Node> {
        let index = match self.index.entry(token) {
            Entry::Occupied(slot) => *slot.get(),
            Entry::Vacant(slot) => {
                // One node per distinct `u32` token: the count always fits.
                let index = u32::try_from(self.nodes.len()).ok()?;
                self.nodes.push(Node::EMPTY);
                *slot.insert(index)
            }
        };
        self.nodes.get_mut(index as usize)
    }
}

struct Trellis {
    /// The surface's `<nstore, nbest>`.
    shape: NbestShape,
    /// One [`Step`] per byte position of the remaining input.
    steps: Vec<Step>,
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
        let mut steps = vec![Step::default(); bound + 1];
        if let Some(first) = steps.first_mut() {
            for &seed_token in seeds {
                // A repeated seed replaces its node in place, keeping the
                // node's creation slot.
                if let Some(node) = first.node_mut(seed_token) {
                    *node = Node::EMPTY;
                    node.values[0] = Value::seed(seed_token);
                    node.len = 1;
                }
            }
        }
        Self {
            shape,
            steps,
            texts: TokenMap::default(),
            text_keys: Vec::new(),
            text_tones: Vec::new(),
        }
    }

    /// Preserve front eviction and physical slots before any successor
    /// records its predecessor slot (074a2219 phonetic_lookup.h:282-293,665,695).
    fn insert(&mut self, position: usize, value: Value) {
        let nstore = self.shape.nstore();
        let Some(node) = self
            .steps
            .get_mut(position)
            .and_then(|step| step.node_mut(value.token))
        else {
            return;
        };
        node.keep(value, nstore);
    }

    /// 074a2219 phonetic_lookup.h:272-297: creation order, physical slots.
    fn candidates(&self, position: usize) -> Vec<BeamEntry> {
        self.steps.get(position).map_or_else(Vec::new, |step| {
            step.nodes
                .iter()
                .flat_map(|node| {
                    node.values()
                        .iter()
                        .zip(0_u32..)
                        .map(|(&value, slot)| (value, slot))
                })
                .enumerate()
                .map(|(batch_position, (value, slot))| BeamEntry {
                    value,
                    slot,
                    batch_position,
                })
                .collect()
        })
    }

    fn beam(&mut self, position: usize) -> SmallVec<[BeamEntry; NBEAM]> {
        select_top(self.candidates(position), NBEAM, self.shape.nstore())
            .into_iter()
            .collect()
    }

    /// 074a2219 phonetic_lookup.h:329-339: only the fixed final step,
    /// complete-key selection, then S3's stable ordering by raw possibility.
    /// Fewer final-step values yield fewer tails; an empty final step
    /// never falls back to an earlier populated step (#594).
    fn tails(&mut self) -> Vec<Value> {
        let position = self.steps.len().saturating_sub(1);
        let mut values: Vec<_> = select_top(
            self.candidates(position),
            self.shape.nbest(),
            self.shape.nstore(),
        )
        .into_iter()
        .map(|entry| entry.value)
        .collect();
        tail_merge(&mut values);
        values
    }

    fn extract(&self, tail: &Value) -> Vec<(usize, u32)> {
        let mut spans = Vec::new();
        let mut current = *tail;
        while current.from != SEED_STEP {
            let from = current.from as usize;
            spans.push((from, current.token));
            let Some(next) = self
                .steps
                .get(from)
                .and_then(|step| step.node(current.prev_token))
                .and_then(|node| node.values().get(current.sub as usize))
            else {
                break;
            };
            current = *next;
        }
        spans.reverse();
        spans
    }

    /// The phrase text of `token`, read once per call through `cache`;
    /// `None` where the dictionary has no text for it.
    fn token_text<D>(
        &self,
        dictionary: &D,
        cache: &mut TokenMap<Option<PhraseText>>,
        token: u32,
    ) -> Result<Option<PhraseText>, EngineError>
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
                        })?,
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
        cache: &mut TokenMap<Option<PhraseText>>,
        spans: &[(usize, u32)],
    ) -> Result<Option<PhraseText>, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey, Entry = PhraseEntry>,
        D::Error: core::fmt::Display,
    {
        let mut text = oxpinyin_core::PhraseTextConcat::new();
        for (_, token) in spans {
            let Some(part) = self.token_text(dictionary, cache, *token)? else {
                return Ok(None);
            };
            text.push(&part);
        }
        Ok(Some(text.finish()))
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
#[derive(Default)]
struct SpanPaths {
    keys: Vec<SyllableKey>,
    tones: Vec<u8>,
    bounds: Vec<(u32, u32)>,
}

impl SpanPaths {
    fn clear(&mut self) {
        self.keys.clear();
        self.tones.clear();
        self.bounds.clear();
    }

    fn push(&mut self, keys: &[SyllableKey], tones: &[u8]) -> u32 {
        let id = u32::try_from(self.bounds.len()).unwrap_or(u32::MAX);
        let offset = u32::try_from(self.keys.len()).unwrap_or(u32::MAX);
        self.keys.extend_from_slice(keys);
        self.tones.extend_from_slice(tones);
        self.bounds
            .push((offset, u32::try_from(keys.len()).unwrap_or(u32::MAX)));
        id
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
/// `memos[i]` is what the dictionary keeps while it prices path `i` (its
/// [`Dictionary::PathMemo`]), gone with the span search.
struct SpanRanges<M> {
    ranges: PhraseIndexRanges<LazySlot>,
    paths: SpanPaths,
    memos: Vec<M>,
}

impl<M> Default for SpanRanges<M> {
    fn default() -> Self {
        Self {
            ranges: PhraseIndexRanges::default(),
            paths: SpanPaths::default(),
            memos: Vec::new(),
        }
    }
}

impl<M: Default> SpanRanges<M> {
    fn clear(&mut self) {
        self.ranges.clear();
        self.paths.clear();
        self.memos.clear();
    }

    /// Records a key path and files a fresh memo for it; the id indexes both.
    fn push_path(&mut self, keys: &[SyllableKey], tones: &[u8]) -> u32 {
        self.memos.push(M::default());
        self.paths.push(keys, tones)
    }
}

/// Prices a [`LazySlot`] through the dictionary, once per slot.
struct Pricing<'a, D: Dictionary> {
    /// The span search's ranges.
    ranges: &'a PhraseIndexRanges<LazySlot>,
    dictionary: &'a D,
    paths: &'a SpanPaths,
    /// The dictionary's memo of each path, indexed like `paths`.
    memos: &'a [D::PathMemo],
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
        let item = match self.memos.get(slot.path as usize) {
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
                    memos: &found.memos,
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
        // `trellis.text` resolved every token's text or the row was
        // skipped above; each span keeps its phrase's own text, stored words
        // and length included, for the constraint writes and training.
        let Some(phrase_spans) = spans
            .iter()
            .map(|(start, token)| {
                Some(PhraseSpan {
                    start: *start,
                    token: PhraseToken::new(*token),
                    text: texts.get(token)?.clone()?,
                })
            })
            .collect::<Option<Vec<PhraseSpan>>>()
        else {
            continue;
        };
        let tokens = phrase_spans.iter().map(|span| span.token).collect();
        rows.push(NbestRow {
            text,
            tokens,
            spans: phrase_spans,
            keys: tail.keys as usize,
            span,
            cost: 0,
            poss: Some(tail.poss),
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
    /// The table searches' value bytes: one buffer for every point read
    /// of the guess, as the pin's table reads into the chunk it reuses
    /// (`chewing_large_table2_kyotodb.cpp:170-174` at 074a2219).
    value: Vec<u8>,
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
        into: &mut SpanRanges<D::PathMemo>,
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
        into: &mut SpanRanges<D::PathMemo>,
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
                .trellis_ranges_continued(
                    &path.keys,
                    &path.tones,
                    &mut path.ranges,
                    &mut path.value,
                )
                .map_err(|error| {
                    EngineError::Scoring(ScoringError::Dictionary(error.to_string()))
                })?;
            let id = into.push_path(&path.keys, &path.tones);
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
            memos: &found.memos,
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
    /// `length` is the forced phrase's stored length
    /// ([`PhraseText::phrase_length`]); it only bounds
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
        length: usize,
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
            for reading in training_readings(columns, self.input, start, end, length) {
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
    let pronunciation = entry.pronunciation.map_or(1.0, |(matched, total)| {
        if total == 0 {
            0.0
        } else {
            matched as f32 / total as f32
        }
    });
    if pronunciation < f32::EPSILON {
        return Ok(());
    }
    let probability = match branch {
        Branch::Bigram(_) => step.blended.map(|s| s * f64::from(pronunciation)),
        Branch::Unigram => step
            .unigram
            .map(|elem| (elem * f64::from(pronunciation)) * f64::from(step.unigram_lambda)),
    };
    let Some(probability) = probability else {
        return Ok(());
    };
    let poss = (f64::from(predecessor.value.poss) + probability.ln()) as f32;
    if let Entry::Vacant(slot) = trellis.texts.entry(entry.token) {
        // The first step to price the token names the path whose text
        // a result reads.
        let (keys, tones) = paths.path(entry.path);
        let offset = u32::try_from(trellis.text_keys.len()).unwrap_or(u32::MAX);
        trellis.text_keys.extend_from_slice(keys);
        trellis.text_tones.extend_from_slice(tones);
        slot.insert((offset, u32::try_from(keys.len()).unwrap_or(u32::MAX)));
    }
    // `m_last_step` is a `gint32` step; a graph's positions all fit.
    let from = u32::try_from(span.start)
        .ok()
        .filter(|&from| from != SEED_STEP)
        .ok_or(EngineError::Graph(GraphError::InputTooLong {
            len: span.start,
            limit: MAX_GRAPH_INPUT,
        }))?;
    let chars = entry.chars;
    trellis.insert(
        span.end,
        Value {
            token: entry.token,
            prev_token: prev,
            from,
            sub: predecessor.slot,
            length: predecessor.value.length.saturating_add(chars),
            keys: predecessor.value.keys.saturating_add(entry.keys),
            poss,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{NbestRow, NbestShape, Trellis, Value, nbest_sentences};
    use crate::session::ScanKey;
    use oxpinyin_core::graph::SegmentGraph;
    use oxpinyin_core::{
        Cost, Dictionary, LanguageModel, NbestStepCosts, OptionBits, PhraseEntry, PhraseToken,
        SyllableKey,
    };
    use oxpinyin_testsupport::{FixtureDictionary, FixtureLanguageModel};

    use crate::config::EmptyConfigSource;
    use crate::error::EngineError;
    use crate::session::build_scan_matrix;
    use crate::storage::StoragePaths;

    #[derive(Debug)]
    struct TextError;

    impl core::fmt::Display for TextError {
        fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            formatter.write_str("text dictionary error")
        }
    }

    impl std::error::Error for TextError {}

    struct TextDictionary;

    impl Dictionary for TextDictionary {
        type Syllable = SyllableKey;
        type Entry = PhraseEntry;
        type Error = TextError;
        type PathMemo = ();

        fn lookup(&self, _syllables: &[Self::Syllable]) -> Result<Vec<Self::Entry>, Self::Error> {
            Ok(vec![PhraseEntry::new(PhraseToken::new(1), "你好")])
        }
    }

    #[test]
    fn an_ordinary_sentence_text_stays_inline() {
        let mut trellis = Trellis::with_seeds(0, &[], NbestShape::PINYIN);
        trellis.texts.insert(1, (0, 1));
        trellis
            .text_keys
            .push(SyllableKey::from_text("ni").expect("ni is a key"));
        trellis.text_tones.push(0);
        let mut cache = super::TokenMap::default();
        let text = trellis
            .text(&TextDictionary, &mut cache, &[(0, 1)])
            .expect("trellis text")
            .expect("present");
        assert!(text.is_inline(), "short sentence is inline");
        assert_eq!(text.as_str(), Some("你好"));
    }

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
        // The paths are leaked rather than held in a static.
        super::Pricing {
            ranges,
            dictionary: DICTIONARY.get_or_init(FixtureDictionary::default),
            paths: Box::leak(Box::default()),
            memos: &[],
        }
    }

    fn loses_to(left: &Value, right: &Value) -> bool {
        super::node_loses_to(left, right, 2)
    }

    fn value(cost: Cost, length: u32, _seq: u64) -> Value {
        scored(-(cost as f32) * std::f32::consts::LN_2 / 1000.0, length)
    }

    fn scored(poss: f32, length: u32) -> Value {
        Value {
            token: 1,
            prev_token: 0,
            from: super::SEED_STEP,
            sub: 0,
            length,
            keys: length,
            poss,
        }
    }

    #[test]
    fn complete_selection_keys_obey_ordering_laws() {
        let mut entries = Vec::new();
        for length in [0, 1, u32::MAX] {
            for poss in [
                f32::NAN,
                f32::from_bits(0xffc00001),
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::MAX,
                -f32::MAX,
                0.0,
                -0.0,
                -1.0,
            ] {
                entries.push(super::BeamEntry {
                    value: scored(poss, length),
                    slot: 1,
                    batch_position: entries.len(),
                });
            }
        }
        for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
            let less = |a, b| super::selection_compare(a, b, shape.nstore()).is_gt();
            for a in &entries {
                assert!(!less(a, a));
                for b in &entries {
                    assert_eq!(
                        less(a, b),
                        !less(b, a) && a.batch_position != b.batch_position
                    );
                    for c in &entries {
                        if less(a, b) && less(b, c) {
                            assert!(less(a, c));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn node_transitions_preserve_physical_slots() {
        let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
        let insert = |trellis: &mut Trellis, poss, identity| {
            trellis.insert(
                1,
                Value {
                    prev_token: identity,
                    ..scored(poss, 1)
                },
            );
        };
        let slots = |trellis: &Trellis| {
            trellis
                .candidates(1)
                .iter()
                .map(|entry| (entry.slot, entry.value.prev_token))
                .collect::<Vec<_>>()
        };
        assert!(slots(&trellis).is_empty());
        insert(&mut trellis, -10.0, 1);
        assert_eq!(slots(&trellis), [(0, 1)]);
        insert(&mut trellis, -20.0, 2);
        assert_eq!(slots(&trellis), [(0, 1), (1, 2)]);
        insert(&mut trellis, -15.0, 3); // Beats the back only: reject.
        assert_eq!(slots(&trellis), [(0, 1), (1, 2)]);
        insert(&mut trellis, -10.0, 4); // Ties the front: reject.
        assert_eq!(slots(&trellis), [(0, 1), (1, 2)]);
        insert(&mut trellis, -5.0, 5); // Evicts the front, keeps slot 1.
        assert_eq!(slots(&trellis), [(0, 5), (1, 2)]);

        for (poss, expected) in [(-5.0, [2, 1]), (-10.0, [1, 2]), (-20.0, [1, 2])] {
            let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
            insert(&mut trellis, -10.0, 1);
            insert(&mut trellis, poss, 2);
            assert_eq!(slots(&trellis), [(0, expected[0]), (1, expected[1])]);
        }
    }

    #[test]
    fn selection_keeps_the_complete_key_at_each_limit() {
        for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
            for limit in [0, 1, 3, super::NBEAM] {
                for size in [0, limit.saturating_sub(1), limit, limit + 1] {
                    for tied in [false, true] {
                        let entries: Vec<_> = (0..size)
                            .rev()
                            .map(|position| super::BeamEntry {
                                value: scored(if tied { -1.0 } else { -(position as f32) }, 1),
                                slot: (position % 2) as u32,
                                batch_position: position,
                            })
                            .collect();
                        let selected = super::select_top(entries, limit, shape.nstore());
                        assert_eq!(
                            selected
                                .iter()
                                .map(|entry| entry.batch_position)
                                .collect::<Vec<_>>(),
                            (0..size.min(limit)).collect::<Vec<_>>()
                        );
                        assert!(
                            selected
                                .iter()
                                .all(|entry| entry.slot as usize == entry.batch_position % 2)
                        );
                    }
                }
            }
        }
    }

    /// The standard-heap alternative uses exactly the production key. A
    /// max heap needs the reverse of the best-first selection comparator.
    #[derive(Clone, Copy, Debug)]
    struct HeapEntry<const S: usize>(super::BeamEntry);

    impl<const S: usize> Ord for HeapEntry<S> {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            super::selection_compare(&other.0, &self.0, S)
        }
    }

    impl<const S: usize> PartialOrd for HeapEntry<S> {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl<const S: usize> PartialEq for HeapEntry<S> {
        fn eq(&self, other: &Self) -> bool {
            self.cmp(other).is_eq()
        }
    }

    impl<const S: usize> Eq for HeapEntry<S> {}

    fn binary_heap_top<const S: usize>(
        values: Vec<super::BeamEntry>,
        count: usize,
    ) -> Vec<super::BeamEntry> {
        let mut heap = std::collections::BinaryHeap::from(
            values.into_iter().map(HeapEntry::<S>).collect::<Vec<_>>(),
        );
        (0..count)
            .map_while(|_| heap.pop().map(|entry| entry.0))
            .collect()
    }

    fn selection_batch(size: usize, tied: bool) -> Vec<super::BeamEntry> {
        (0..size)
            .rev()
            .map(|position| super::BeamEntry {
                value: scored(
                    if tied {
                        -1.0
                    } else {
                        [
                            f32::NAN,
                            f32::INFINITY,
                            f32::NEG_INFINITY,
                            0.0,
                            -0.0,
                            -1.0,
                            -20.0,
                        ][position % 7]
                    },
                    if tied { 1 } else { (position % 3) as u32 },
                ),
                slot: (position % 2) as u32,
                batch_position: position,
            })
            .collect()
    }

    #[test]
    fn selection_agrees_with_std_heap_on_mixed_keys() {
        for size in [0, 1, 2, 3, 31, 32, 33, 128] {
            for count in [0, 1, 3, super::NBEAM, 129] {
                for tied in [false, true] {
                    let entries = selection_batch(size, tied);
                    let ids = |entries: Vec<super::BeamEntry>| {
                        entries
                            .into_iter()
                            .map(|entry| (entry.batch_position, entry.slot))
                            .collect::<Vec<_>>()
                    };
                    assert_eq!(
                        ids(super::select_top(entries.clone(), count, 1)),
                        ids(binary_heap_top::<1>(entries.clone(), count))
                    );
                    assert_eq!(
                        ids(super::select_top(entries.clone(), count, 2)),
                        ids(binary_heap_top::<2>(entries, count))
                    );
                }
            }
        }
    }

    #[test]
    fn float_trellis_storage_shrinks() {
        assert_eq!(std::mem::size_of::<Value>(), 28);
        assert_eq!(std::mem::size_of::<super::Node>(), 60);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(std::mem::size_of::<super::BeamEntry>(), 40);
    }

    #[test]
    fn ieee_order_and_stable_raw_tails() {
        let mut tails = [
            f32::NAN,
            -0.0,
            f32::NEG_INFINITY,
            f32::INFINITY,
            0.0,
            -1.0,
            f32::from_bits(0xffc00001),
        ]
        .map(|p| scored(p, 1));
        for (index, tail) in tails.iter_mut().enumerate() {
            tail.token = index as u32;
        }
        super::tail_merge(&mut tails);
        assert_eq!(tails.map(|v| v.token), [3, 1, 4, 5, 2, 0, 6]);
        for (index, left) in tails.iter().enumerate() {
            for right in &tails[index..] {
                assert!(!super::ordered_poss_compare(left.poss, right.poss).is_gt());
            }
        }
    }

    #[test]
    fn adjusted_score_rounds_product_then_difference_and_ties() {
        let p = 1.2_f32.ln();
        for advantage in [p.next_down(), p, p.next_up()] {
            let short = scored(0.0, 0);
            let long = scored(advantage, 1);
            assert_eq!(loses_to(&short, &long), advantage > p);
            assert_eq!(loses_to(&long, &short), advantage < p);
        }
        // Distinct exact adjusted scores collapse on the f32 grid.
        let short = scored(-16777216.0, 0);
        let long = scored(-16777216.0, 1);
        assert_ne!(f64::from(short.poss), f64::from(long.poss) - f64::from(p));
        assert_eq!(super::adjusted_poss(&short), super::adjusted_poss(&long));
        assert!(!loses_to(&short, &long));
        assert!(!loses_to(&long, &short));
        for special in [f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(!loses_to(&scored(special, 0), &scored(special, u32::MAX)));
        }
    }

    #[test]
    fn float_adjusted_comparison_is_a_strict_weak_order() {
        let mut values = Vec::new();
        for length in [0, 1, 3, u32::MAX] {
            for poss in [
                f32::NAN,
                f32::from_bits(0xffc00001),
                f32::INFINITY,
                f32::NEG_INFINITY,
                f32::MAX,
                -f32::MAX,
                0.0,
                -0.0,
                -1.0,
                -1.0_f32.next_up(),
            ] {
                values.push(scored(poss, length));
            }
        }
        for a in &values {
            assert!(!loses_to(a, a));
            for b in &values {
                assert!(!(loses_to(a, b) && loses_to(b, a)));
                for c in &values {
                    if loses_to(a, b) && loses_to(b, c) {
                        assert!(loses_to(a, c));
                    }
                    if !loses_to(a, b) && !loses_to(b, a) && !loses_to(b, c) && !loses_to(c, b) {
                        assert!(!loses_to(a, c) && !loses_to(c, a));
                    }
                }
            }
        }
    }

    #[test]
    fn tails_require_the_final_step_even_when_underfilled() {
        for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
            let mut trellis = Trellis::with_seeds(2, &[], shape);
            for token in 1..=3 {
                trellis.insert(
                    1,
                    Value {
                        token,
                        ..scored(-1.0, 1)
                    },
                );
            }
            assert!(trellis.tails().is_empty());
            trellis.insert(
                2,
                Value {
                    token: 4,
                    ..scored(-2.0, 2)
                },
            );
            assert_eq!(
                trellis.tails().iter().map(|v| v.token).collect::<Vec<_>>(),
                [4]
            );
        }
    }

    #[test]
    fn full_node_preserves_front_eviction_and_rejects_ties() {
        let mut trellis = Trellis::with_seeds(1, &[], NbestShape::PINYIN);
        for poss in [-10.0, -20.0, -5.0] {
            trellis.insert(1, scored(poss, 1));
        }
        let residents = trellis.candidates(1);
        assert_eq!(
            residents.iter().map(|v| v.value.poss).collect::<Vec<_>>(),
            [-5.0, -20.0]
        );
        trellis.insert(
            1,
            Value {
                keys: 999,
                ..scored(-5.0, 1)
            },
        );
        assert!(trellis.candidates(1).iter().all(|v| v.value.keys != 999));
    }

    #[test]
    fn selection_uses_batch_ties_and_zhuyin_length_first_nan_last() {
        for shape in [NbestShape::PINYIN, NbestShape::ZHUYIN] {
            let mut trellis = Trellis::with_seeds(1, &[], shape);
            for token in 1..=40 {
                trellis.insert(
                    1,
                    Value {
                        token,
                        ..scored(-1.0, 1)
                    },
                );
            }
            assert_eq!(
                trellis
                    .beam(1)
                    .iter()
                    .map(|v| v.value.token)
                    .collect::<Vec<_>>(),
                (1..=32).collect::<Vec<_>>()
            );
        }
        let entry = |poss, length, batch_position| super::BeamEntry {
            value: scored(poss, length),
            slot: 0,
            batch_position,
        };
        assert!(
            super::selection_compare(&entry(f32::INFINITY, 2, 0), &entry(f32::NAN, 1, 1), 1)
                .is_gt()
        );
        assert!(
            super::selection_compare(&entry(f32::NAN, 1, 0), &entry(f32::NEG_INFINITY, 1, 1), 1)
                .is_gt()
        );
        assert!(super::selection_compare(&entry(-0.0, 1, 1), &entry(0.0, 1, 0), 1).is_gt());
        // Zhuyin's single-slot admission remains raw IEEE <, including NaN.
        assert!(!super::node_loses_to(
            &scored(f32::NAN, 1),
            &scored(1.0, 1),
            1
        ));
    }

    #[test]
    fn backtracking_uses_the_physical_slot_after_batch_selection() {
        let mut trellis = Trellis::with_seeds(3, &[0], NbestShape::PINYIN);
        for token in [7, 8] {
            trellis.insert(
                1,
                Value {
                    token,
                    from: 0,
                    ..value(0, 1, 0)
                },
            );
        }
        trellis.insert(
            2,
            Value {
                token: 99,
                ..value(0, 2, 0)
            },
        );
        for (cost, prev_token) in [(100, 7), (200, 8)] {
            trellis.insert(
                2,
                Value {
                    token: 11,
                    prev_token,
                    from: 1,
                    ..value(cost, 2, 0)
                },
            );
        }
        let beam = trellis.beam(2);
        let predecessor = beam
            .iter()
            .find(|entry| entry.value.token == 11 && entry.slot == 1)
            .unwrap();
        assert_eq!(predecessor.batch_position, 2);
        let paths = super::SpanPaths::default();
        let model = Fixed {
            blended: Some(0),
            unigram: Some(0),
        };
        super::expand_one(
            &super::SpanEntry {
                token: 22,
                path: 0,
                keys: 1,
                chars: 1,
                pronunciation: None,
            },
            super::StepCtx::new(&paths, 2..3, &model),
            predecessor,
            &mut oxpinyin_core::NbestStepRow::default(),
            &mut trellis,
            super::Branch::Unigram,
        )
        .unwrap();
        let tails = trellis.tails();
        assert_eq!(tails[0].sub, 1);
        assert_eq!(trellis.extract(&tails[0]), [(0, 8), (1, 11), (2, 22)]);
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
                blended: (self.blended)
                    .map(|cost: i64| (-(cost as f64) * std::f64::consts::LN_2 / 1000.0).exp()),
                unigram: (self.unigram)
                    .map(|cost: i64| (-(cost as f64) * std::f64::consts::LN_2 / 1000.0).exp()),

                unigram_lambda: 1.0,
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
                    blended: Some(0.5),
                    unigram: Some(0.25),

                    unigram_lambda: 1.0,
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
                    blended: Some(0.5),
                    unigram: Some(0.25),

                    unigram_lambda: 1.0,
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
                    blended: (item.map(|(freq, _)| Cost::from(freq)))
                        .map(|cost: i64| (-(cost as f64) * std::f64::consts::LN_2 / 1000.0).exp()),
                    unigram: Some(0.25),

                    unigram_lambda: 1.0,
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
                    blended: Some(0.5),
                    unigram: Some(0.25),

                    unigram_lambda: 1.0,
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
        assert_eq!(rows[0].text.as_str(), Some("你好"));
        assert_eq!(rows[0].span, bound);
        assert_eq!(rows[0].keys, 2);
    }

    /// libzhuyin's `PhoneticLookup<1, 1>` keeps one value per node and
    /// extracts one tail, where libpinyin's `<2, 3>` extracts up to three:
    /// two readings of `ni` give the pinyin shape two rows and the zhuyin
    /// shape exactly one. Selection ties use earlier batch position.
    #[test]
    fn the_zhuyin_shape_keeps_one_tail_with_batch_ties() {
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
        assert_eq!(zhuyin[0].text, "你好", "earlier batch position wins ties");
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
            poss: None,
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
