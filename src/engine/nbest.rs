//! Beam search for n-best sentence rows. Each call builds a trellis over byte positions.
//! Nodes retain phrase hypotheses using the session's [`NbestShape`]. The search widens a
//! span while the phrase table reports a continuation.
//!
//! Each step accumulates the log of interpolated bigram and unigram probability times
//! pronunciation probability. The bigram branch expands each beam value. The unigram
//! branch expands its head.

// PhoneticLookup<nstore, nbest>, lookup/phonetic_lookup.h; pinyin.cpp:55 and
// zhuyin.cpp:50. search_bigram2 and search_unigram2 use the two expansion rules.
// Pronunciation is summed before the epsilon check (phonetic_key_matrix.cpp:534–600,
// phonetic_lookup.h:643–697). The compatibility-policy selection rules apply to pinyin
// adjusted scores and Zhuyin length, raw possibility and batch position.

use crate::sentence_facts::{SentenceFacts, TokenMap};
use std::cell::{OnceCell, RefCell};
use std::cmp::Ordering;
use std::collections::hash_map::Entry;

use crate::{
    model::{
        Dictionary, LanguageModel, MergedGram, NbestSuccessors, PhraseIndexRanges, PhraseItemRead,
        TokenPhrase, TrellisItem,
    },
    parser::ChewingKey,
    vocab::{PhraseText, PhraseToken, SyllableKey},
};
use smallvec::SmallVec;

use crate::engine::constraint::{Cell, ConstraintStore, PhraseSpan};
use crate::engine::error::EngineError;
use crate::engine::session::matrix::{ParsedMatrix, ScanMatrix};
use crate::engine::session::{ScanKey, column_is_empty};
use crate::vocab::{MAX_PHRASE_LENGTH, SENTENCE_START};

/// Inline capacity of a node's value list: libpinyin's `nstore`, the
/// larger of the two shapes, so neither allocates per node. The live cap
/// is [`NbestShape::nstore`].
const NSTORE: usize = 2;
/// Sentence rows extracted from the tails (`nbest`) on the pinyin surface.
pub const NBEST_ROWS: usize = 3;

// PhoneticLookup<nstore, nbest> asserts nstore <= nbest, phonetic_lookup.h:715. The
// instantiations are pinyin.cpp:55 and zhuyin.cpp:50.
/// Sets the number of values retained at each trellis node and the number of sentence
/// tails extracted. [`Self::PINYIN`] and [`Self::ZHUYIN`] provide the two engine
/// configurations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct NbestShape {
    nstore: usize,
    nbest: usize,
}

impl NbestShape {
    /// Pinyin configuration with two values per trellis node and three sentence tails.
    pub const PINYIN: Self = Self {
        nstore: NSTORE,
        nbest: NBEST_ROWS,
    };
    /// Zhuyin configuration with one value per trellis node and one sentence tail.
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

/// One decoded sentence row, in tail order (index 0 is the 1-best).
#[derive(Clone, Debug)]
pub struct NbestRow {
    /// Concatenated phrase texts. `None` represents a NULL string when no
    /// phrase converts.
    pub text: Option<PhraseText>,
    /// The phrase tokens in order — the selection record a chosen row
    /// contributes (upstream keeps the whole `MatchResult` on the
    /// instance; the engine records tokens instead).
    pub(crate) tokens: Vec<PhraseToken>,
    /// The row's phrases at their walk positions — the `MatchResult`
    /// shape `diff_result` constraints and the constraint-aware train
    /// walk read. Walk-local coordinates; `Session::guess_sentence`
    /// absolutizes them when it stores the rows.
    pub spans: Vec<PhraseSpan>,
    /// Keys the sentence covers.
    pub(crate) keys: usize,
    /// Bytes of the input the sentence spans.
    pub span: usize,
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
    /// graph's input, at most `MAX_GRAPH_POSITION`.
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
/// (`phonetic_lookup.h:39`), in Rust's `f32` maths.
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
/// (074a2219 phonetic_lookup.h:184-186, filled by `insert_candidate` at :300-330): `m_steps_index`, a token → node-index `GHashTable`, and
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
    /// Seeds step 0 with one zero-cost node per token occurrence — upstream's
    /// `fill_prefixes` (`phonetic_lookup.h:244-276`), which inserts
    /// every `m_prefixes` entry as an initial node at `log(1.0)`.
    /// `pinyin_guess_sentence_with_prefix` drives this with the prefix
    /// token list; the single-seed `new` stays the sentence path's law.
    fn with_seeds(bound: usize, seeds: &[u32], shape: NbestShape) -> Self {
        let mut steps = vec![Step::default(); bound + 1];
        if let Some(first) = steps.first_mut() {
            for &seed_token in seeds {
                // The C prefix path supplies at most 1 + 16 * 4 seeds.
                let Ok(index) = u32::try_from(first.nodes.len()) else {
                    break;
                };
                let mut node = Node::EMPTY;
                node.values[0] = Value::seed(seed_token);
                node.len = 1;
                first.nodes.push(node);
                // Every occurrence occupies a beam slot. Backtracking
                // resolves the last node, as fill_prefixes' index does.
                first.index.insert(seed_token, index);
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

    /// The stored length and text of `token`, read once per call through
    /// `cache`; `None` where the dictionary has no item for it.
    fn token_text<'cache, D>(
        &self,
        dictionary: &D,
        cache: &'cache mut TokenMap<Option<TokenPhrase>>,
        token: u32,
        facts: &RefCell<SentenceFacts>,
    ) -> Result<Option<&'cache TokenPhrase>, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey>,
    {
        let slot = match cache.entry(token) {
            std::collections::hash_map::Entry::Occupied(slot) => {
                return Ok(slot.into_mut().as_ref());
            }
            std::collections::hash_map::Entry::Vacant(slot) => slot,
        };
        let text = match self.texts.get(&token) {
            Some(&(offset, len)) => {
                let range = offset as usize..offset as usize + len as usize;
                match (
                    self.text_keys.get(range.clone()),
                    self.text_tones.get(range),
                ) {
                    (Some(keys), Some(tones)) => dictionary
                        .trellis_text_with_facts(keys, tones, token, &mut facts.borrow_mut())
                        .map_err(|_| EngineError::Failure)?,
                    _ => None,
                }
            }
            None => None,
        };
        Ok(slot.insert(text).as_ref())
    }

    /// The row's string as `convert_to_utf8` builds it
    /// (`lookup/lookup.cpp:39-68` at 074a2219): each phrase converted on its
    /// own and cut at its first zero, a phrase whose conversion is NULL left
    /// out (`g_strdup(NULL)` and `g_strconcat` stop at it, `:62-65`).
    /// When all items are present and no phrase converts, `None` is the
    /// pin's NULL string. The sentence getters return true and write NULL
    /// (`pinyin.cpp:1464-1482`, `zhuyin.cpp:983-999`). Only the candidate
    /// dedup's `strcmp` on NULL is undefined.
    /// A missing item also stops this read; the span walk rejects that row.
    fn text<D>(
        &self,
        dictionary: &D,
        cache: &mut TokenMap<Option<TokenPhrase>>,
        spans: &[(usize, u32)],
        facts: &RefCell<SentenceFacts>,
    ) -> Result<Option<PhraseText>, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey>,
    {
        let mut text = crate::ucs4::PhraseTextConcat::new();
        let mut converted = false;
        for (_, token) in spans {
            let Some(part) = self.token_text(dictionary, cache, *token, facts)? else {
                return Ok(None);
            };
            if let Some(part) = &part.text {
                text.push(part);
                converted = true;
            }
        }
        Ok(converted.then(|| text.finish()))
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
    pronunciation: f32,
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
    /// Each token's pronunciation sum over the span, from its first priced
    /// slot (see [`Pricing::sums`]).
    sums: RefCell<TokenMap<TokenSum>>,
}

impl<M> Default for SpanRanges<M> {
    fn default() -> Self {
        Self {
            ranges: PhraseIndexRanges::default(),
            paths: SpanPaths::default(),
            memos: Vec::new(),
            sums: RefCell::default(),
        }
    }
}

impl<M: Default> SpanRanges<M> {
    fn clear(&mut self) {
        self.ranges.clear();
        self.paths.clear();
        self.memos.clear();
        self.sums.get_mut().clear();
    }

    /// Records a key path and files a fresh memo for it; the id indexes both.
    fn push_path(&mut self, keys: &[SyllableKey], tones: &[u8]) -> u32 {
        self.memos.push(M::default());
        self.paths.push(keys, tones)
    }
}

/// A token's pronunciation sum over a span, kept with the phrase length the
/// walk filtered paths by.
#[derive(Clone, Copy)]
struct TokenSum {
    chars: u32,
    pronunciation: f32,
}

/// Prices a [`LazySlot`] through the dictionary, once per slot.
struct Pricing<'a, D: Dictionary> {
    /// The span search's ranges.
    ranges: &'a PhraseIndexRanges<LazySlot>,
    dictionary: &'a D,
    paths: &'a SpanPaths,
    /// The dictionary's memo of each path, indexed like `paths`.
    memos: &'a [D::PathMemo],
    facts: &'a RefCell<SentenceFacts>,
    generation: &'a GenerationView<'a>,
    span: std::ops::Range<usize>,
    /// The pin computes the pronunciation sum on each predecessor step
    /// that reaches pricing (`lookup/phonetic_lookup.h:655,683`). The sum
    /// depends only on the token's item and the span's paths. Slots for the
    /// same token and phrase length in the span share the cached sum.
    sums: &'a RefCell<TokenMap<TokenSum>>,
}

impl<'a, D> Pricing<'a, D>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    /// What the dictionary prices of `token` on path `id`, through the
    /// path's memo. Every path id is one [`SpanRanges::push_path`]
    /// returned or one below `memos.len()`, and `push_path` files a memo
    /// with each path, so every id has a memo; an id without one reads no
    /// item.
    fn item_on_path(
        &self,
        id: u32,
        keys: &[SyllableKey],
        tones: &[u8],
        token: u32,
    ) -> Result<Option<TrellisItem>, EngineError> {
        let Some(memo) = self.memos.get(id as usize) else {
            return Ok(None);
        };
        self.dictionary
            .trellis_item_with_facts(keys, tones, token, memo, &mut self.facts.borrow_mut())
            .map_err(|_| EngineError::Failure)
    }

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
        let item = self.item_on_path(slot.path, keys, tones, token)?;
        let cached = item.and_then(|item| {
            self.sums
                .borrow()
                .get(&token)
                .filter(|sum| sum.chars == item.chars)
                .map(|sum| sum.pronunciation)
        });
        let entry = if let (Some(item), Some(pronunciation)) = (item, cached) {
            Some(SpanEntry {
                token,
                path: slot.path,
                keys: keys.len().try_into().unwrap_or(u32::MAX),
                chars: item.chars,
                pronunciation,
            })
        } else if let Some(item) = item {
            let mut path = 0_u32;
            let pronunciation = self.generation.sum_paths(
                self.span.start,
                self.span.end,
                MAX_PHRASE_LENGTH,
                &mut SmallVec::new(),
                &mut |_| {
                    let id = path;
                    path += 1;
                    let (keys, tones) = self.paths.path(id);
                    if keys.len() != item.chars as usize {
                        return Ok(0.0);
                    }
                    let priced = if id == slot.path {
                        Some(item)
                    } else {
                        self.item_on_path(id, keys, tones, token)?
                    };
                    Ok(priced.map_or(0.0, |priced| {
                        priced.pronunciation.map_or(1.0, |(matched, total)| {
                            if total == 0 {
                                0.0
                            } else {
                                matched as f32 / total as f32
                            }
                        })
                    }))
                },
            )?;
            self.sums.borrow_mut().insert(
                token,
                TokenSum {
                    chars: item.chars,
                    pronunciation,
                },
            );
            Some(SpanEntry {
                token,
                path: slot.path,
                keys: keys.len().try_into().unwrap_or(u32::MAX),
                chars: item.chars,
                pronunciation,
            })
        } else {
            None
        };
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

    /// ONESTEP reads the forced item, not its membership in the ranges
    /// (074a2219 phonetic_lookup.h:552-554,601-612). Keep a path of the
    /// item's length for its key count and final text; pricing still sums
    /// every matrix path, including paths whose table search missed.
    fn forced_slot(&self, token: u32) -> Result<Option<LazySlot>, EngineError> {
        for path in 0..self.memos.len() {
            let slot = LazySlot::new(u32::try_from(path).unwrap_or(u32::MAX));
            if self
                .price(&slot, token)?
                .is_some_and(|entry| entry.keys == entry.chars)
            {
                return Ok(Some(slot));
            }
        }
        Ok(None)
    }
}

/// Computes the sentence rows for the input covered by `matrix`.
///
/// `matrix` is the scan matrix ([`crate::engine::session`]'s key columns), `bound`
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
    matrix: &ScanMatrix,
    bound: usize,
    dictionary: &D,
    model: &L,
    history: &[PhraseToken],
    constraints: Option<&ConstraintStore>,
    shape: NbestShape,
) -> Result<Vec<NbestRow>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
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

/// Decodes a prepared scan matrix with the requested generation shape.
///
/// # Errors
/// Returns an engine error when dictionary or model access fails.
pub fn nbest_sentences_generation<D, L>(
    input: GenerationInput<'_>,
    dictionary: &D,
    model: &L,
    history: &[PhraseToken],
    constraints: Option<&ConstraintStore>,
    shape: NbestShape,
) -> Result<Vec<NbestRow>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    // The sentence path's single-seed law: the constrained decode seeds
    // the virtual start; the free walk seeds the history's last token.
    let seed = match constraints {
        Some(_) => SENTENCE_START,
        None => history.last().map_or(SENTENCE_START, |token| *token),
    };
    nbest_sentences_with_seeds_generation(input, dictionary, model, &[seed], constraints, shape)
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
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
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
            columns: topology,
            zeros: &topology.zeros,
        }
    } else {
        owned = crate::engine::session::matrix::ParsedMatrix::from_scan(
            matrix,
            bound,
            physical_separators,
        );
        GenerationView {
            columns: &owned,
            zeros: &owned.zeros,
        }
    };
    let seed_values: Vec<u32> = seeds.to_vec();
    let mut trellis = Trellis::with_seeds(bound, &seed_values, shape);
    // One path scratch for every span search of the call.
    let mut scratch = GenerationPath::default();
    let facts = RefCell::new(SentenceFacts::default());
    let cell_at = |position: usize| constraints.and_then(|store| store.cell(position));

    let env = NbestEnv {
        generation: &generation,
        facts: &facts,
        bound,
        dictionary,
        model,
    };
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
            // `search_bigram2`/`search_unigram2`'s ONESTEP branches,
            // entered on any `SEARCH_OK` (`phonetic_lookup.h:775-788`) —
            // then move on: no widening, no other token starts there.
            if end > position && end <= bound {
                let span = position..end;
                expand_forced_span(&env, token, span, &beam, &mut scratch, &mut trellis)?;
            }
            position += 1;
            continue;
        }

        widen_free_span(
            &env,
            constraints,
            position,
            &beam,
            &mut scratch,
            &mut trellis,
        )?;
        position += 1;
    }

    nbest_rows(&mut trellis, dictionary, &facts, bound)
}

/// One span search to a forced step's fixed end, for the forced token only:
/// `search_bigram2`/`search_unigram2`'s ONESTEP branches, entered on any
/// `SEARCH_OK` (`phonetic_lookup.h:775-788`). No widening follows, and no
/// other token starts at the span's start.
fn expand_forced_span<D, L>(
    env: &NbestEnv<'_, D, L>,
    token: PhraseToken,
    span: std::ops::Range<usize>,
    beam: &[BeamEntry],
    scratch: &mut GenerationPath,
    trellis: &mut Trellis,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    let mut found = SpanRanges::default();
    env.generation
        .search(span.start, span.end, env.dictionary, scratch, &mut found)?;
    let pricing = Pricing {
        ranges: &found.ranges,
        dictionary: env.dictionary,
        paths: &found.paths,
        memos: &found.memos,
        facts: env.facts,
        generation: env.generation,
        span: span.clone(),
        sums: &found.sums,
    };
    if found.ranges.iter().next().is_some() {
        let mut forced = PhraseIndexRanges::new();
        forced.convert(pricing.forced_slot(token)?.map(|slot| (token, Some(slot))));
        let forced = Pricing {
            ranges: &forced,
            ..pricing
        };
        expand_entries(&forced, beam, span, env.model, trellis, Some(token))?;
    }
    Ok(())
}

/// The n-best rows of a finished walk over `bound` columns: one row per
/// trellis tail whose every span has a present phrase item.
fn nbest_rows<D>(
    trellis: &mut Trellis,
    dictionary: &D,
    facts: &RefCell<SentenceFacts>,
    bound: usize,
) -> Result<Vec<NbestRow>, EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    let span = bound;
    let mut rows = Vec::new();
    let mut texts = TokenMap::default();
    for tail in trellis.tails() {
        let spans = trellis.extract(&tail);
        let text = trellis.text(dictionary, &mut texts, &spans, facts)?;
        // Each span needs a present item. It keeps the stored length for
        // constraint writes and training even if its text does not convert.
        let Some(phrase_spans) = spans
            .iter()
            .map(|(start, token)| {
                let phrase = texts.get(token)?.as_ref()?;
                Some(PhraseSpan {
                    start: *start,
                    token: *token,
                    text: phrase.text.clone().unwrap_or_else(|| {
                        PhraseText::from_stored_parts(Vec::new(), phrase.phrase_length)
                    }),
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
        });
    }
    Ok(rows)
}

/// Scan data and separator policy for one decoder invocation.
pub struct GenerationInput<'a> {
    /// Packed scan edges.
    pub matrix: &'a ScanMatrix,
    /// Last byte column reached by the decode.
    pub bound: usize,
    /// Whether separator columns occupy positions in the result.
    pub physical_separators: bool,
    /// Previously placed columns, when available.
    pub topology: Option<&'a crate::engine::session::matrix::ParsedMatrix>,
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
    columns: &'a ParsedMatrix,
    zeros: &'a [bool],
}

impl GenerationView<'_> {
    /// Sum at each recursion frame, as the pin does, rather than flattening
    /// the leaves into a differently rounded sum (074a2219
    /// `phonetic_key_matrix.cpp:534-585`). Leaves and their indices are in
    /// the same order as `walk`; no path is gated on its own probability.
    fn sum_paths(
        &self,
        start: usize,
        end: usize,
        length: usize,
        keys: &mut SmallVec<[ChewingKey; 16]>,
        leaf: &mut impl FnMut(&[ChewingKey]) -> Result<f32, EngineError>,
    ) -> Result<f32, EngineError> {
        if start > end || keys.len() > length {
            return Ok(0.0);
        }
        if start == end {
            return if keys.is_empty() { Ok(0.0) } else { leaf(keys) };
        }
        if self.zeros.get(start).copied().unwrap_or(false) {
            return self.sum_paths(start + 1, end, length, keys, leaf);
        }
        let mut sum = 0.0_f32;
        if let Some(column) = self.columns.get(start) {
            for key in column {
                if key.to > start && key.to <= end {
                    keys.push(key.key.chewing_key().with_tone(key.tone));
                    sum += self.sum_paths(key.to, end, length, keys, leaf)?;
                    keys.pop();
                }
            }
        }
        Ok(sum)
    }

    fn search<D>(
        &self,
        start: usize,
        end: usize,
        dictionary: &D,
        path: &mut GenerationPath,
        into: &mut SpanRanges<D::PathMemo>,
    ) -> Result<bool, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey>,
    {
        let Some(column) = self.columns.get(start) else {
            return Ok(false);
        };
        if column.is_empty() && !self.zeros.get(start).copied().unwrap_or(false) {
            return Ok(false);
        }
        // search_matrix's empty-end-column gate is independent of paths.
        if end + 1 < self.columns.column_count()
            && self.columns.get(end).is_none_or(<[ScanKey]>::is_empty)
            && !self.zeros[end]
        {
            return Ok(true);
        }
        // A failed search leaves the scratch mid-path.
        path.keys.clear();
        path.tones.clear();
        path.ranges.clear();
        // search_matrix appends every path's ranges, system table then user
        // table, to the per-library arrays in walk order.
        self.walk(start, end, dictionary, path, into)
    }

    /// Answers whether any searched step continued past this walk.
    fn walk<D>(
        &self,
        start: usize,
        end: usize,
        dictionary: &D,
        path: &mut GenerationPath,
        into: &mut SpanRanges<D::PathMemo>,
    ) -> Result<bool, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey>,
    {
        let mut continued = false;
        if start > end {
            return Ok(continued);
        }
        if start == end {
            if path.keys.len() > MAX_PHRASE_LENGTH {
                return Ok(continued);
            }
            if path.keys.is_empty() {
                return Ok(true);
            }
            // `search_matrix_recur`'s leaf searches the table, which asserts
            // on a toned first initial-only key (`pinyin_phrase3.h:152`).
            crate::engine::abort_toned_incomplete(&path.keys, &path.tones);
            let hit = dictionary
                .trellis_ranges_continued(
                    &path.keys,
                    &path.tones,
                    &mut path.ranges,
                    &mut path.value,
                )
                .map_err(|_| EngineError::Failure)?;
            let id = into.push_path(&path.keys, &path.tones);
            // Drains the scratch, keeping its capacity for the next path.
            into.ranges
                .append_from(&mut path.ranges, |()| LazySlot::new(id));
            return Ok(hit);
        }
        if self.zeros.get(start).copied().unwrap_or(false) {
            return self.walk(start + 1, end, dictionary, path, into);
        }
        let Some(column) = self.columns.get(start) else {
            return Ok(continued);
        };
        for key in column {
            if key.to > end {
                continued = true;
                continue;
            }
            if key.to <= start {
                continue;
            }
            path.keys.push(key.key);
            path.tones.push(key.tone);
            if path.keys.len() <= MAX_PHRASE_LENGTH {
                continued |= self.walk(key.to, end, dictionary, path, into)?;
            }
            path.keys.pop();
            path.tones.pop();
        }
        Ok(continued)
    }
}

/// The shared, immutable environment of one `nbest_sentences` walk: the
/// scan matrix and its bound, the dictionary, and the language model.
struct NbestEnv<'a, D, L> {
    /// The scan matrix columns.
    generation: &'a GenerationView<'a>,
    facts: &'a RefCell<SentenceFacts>,
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
    trellis: &mut Trellis,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
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
            facts: env.facts,
            generation: env.generation,
            span: position..end,
            sums: &found.sums,
        };
        expand_entries(&pricing, beam, position..end, env.model, trellis, None)?;
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
    parsed: &'a ParsedMatrix,
}

impl<'a> ConstraintSpelling<'a> {
    /// Borrow the parse's already placed matrix. No second matrix is built.
    pub(crate) const fn new(parsed: &'a ParsedMatrix) -> Self {
        Self { parsed }
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
    /// Add each path's f32 ratio in recursive matrix order and clear only
    /// below f32::EPSILON (phonetic_lookup.cpp:159-163).
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
    ) -> Result<bool, EngineError>
    where
        D: Dictionary<Syllable = SyllableKey>,
    {
        let dictionary_error = |_: D::Error| EngineError::Failure;
        let PhraseItemRead::Item(rows) = dictionary
            .phrase_item_pronunciations(token)
            .map_err(dictionary_error)?
        else {
            return Ok(false);
        };
        let parsed = self.parsed;
        let columns = parsed;
        if end >= columns.column_count()
            || column_is_empty(columns, &parsed.zeros, start)
            || column_is_empty(columns, &parsed.zeros, end)
        {
            return Ok(false);
        }
        let generation = GenerationView {
            columns,
            zeros: &parsed.zeros,
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
        let sum =
            generation.sum_paths(start, end, length, &mut SmallVec::new(), &mut |reading| {
                if reading.len() != length {
                    return Ok(0.0);
                }
                let matched = rows
                    .iter()
                    .filter(|(stored, _)| reading_matches(reading, stored))
                    .fold(0_u32, |matched, (_, frequency)| {
                        matched.wrapping_add(*frequency)
                    });
                Ok(matched as f32 / total as f32)
            })?;
        Ok(sum >= f32::EPSILON)
    }
}

/// `pinyin_compare_with_tones(query, stored, phrase_length) == 0`
/// (`src/storage/pinyin_phrase3.h:108-144` at 074a2219), key by key
/// (the three passes only reorder which difference is reported): initials
/// equal (`:68-71`); middle and final equal, or either side the
/// incomplete `(ZERO_MIDDLE, ZERO_FINAL)` pair (`:73-94`, the incomplete
/// test at `:80-86`); tones equal, or either side zero (`:96-105`).
pub(crate) fn reading_matches(query: &[ChewingKey], stored: &[ChewingKey]) -> bool {
    query.len() == stored.len()
        && query.iter().zip(stored).all(|(q, s)| {
            q.initial == s.initial
                && ((q.middle == s.middle && q.final_ == s.final_)
                    || (q.middle == 0 && q.final_ == 0)
                    || (s.middle == 0 && s.final_ == 0))
                && (q.tone == s.tone || q.tone == 0 || s.tone == 0)
        })
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
    trellis: &mut Trellis,
    one_step: Option<u32>,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    // #lizard forgives(cyclomatic_complexity): the predecessor loop (row load,
    // fault check, forced or ranged selection) and the unigram pass after it
    // are one ordered walk of the decoder; a further split would leave the
    // same paths to test and make the order harder to follow.
    let entries = pricing.ranges;
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
    let mut selected: Vec<Selected<'_>> = Vec::new();
    // The best predecessor's row, which the unigram pass below hands to the
    // model again.
    let mut head_row = crate::model::NbestStepRow::default();
    for (index, predecessor) in beam.iter().enumerate() {
        selected.clear();
        let prev = predecessor.value.token;
        // One load per predecessor and span: the pin loads both grams for
        // every beam value in every `search_bigram2` call, `sentence_start`
        // included (`phonetic_lookup.h:588-599` at 074a2219). No row is
        // kept for the step's next span.
        let mut loaded = crate::model::NbestStepRow::default();
        let row = &mut loaded;
        let traversal = match model
            .nbest_successors_in_row(&prev, row)
            .map_err(|error| EngineError::row_read(model, &error))?
        {
            None => false,
            Some(NbestSuccessors::Miss) => true,
            Some(NbestSuccessors::Row(gram)) => {
                if let Some(fault) = gram.fault() {
                    return Err(EngineError::lookup_gram(fault));
                }
                match one_step {
                    Some(token) => select_forced(pricing, gram, token, &mut selected)?,
                    None => select_ranged(pricing, gram, &mut items, &mut selected)?,
                }
                true
            }
        };
        if traversal {
            for &(entry, item) in &selected {
                let branch = Branch::Bigram(Some(item));
                expand_one(
                    entry,
                    StepCtx {
                        paths: pricing.paths,
                        span: span.clone(),
                        model,
                        facts: pricing.facts,
                    },
                    predecessor,
                    row,
                    trellis,
                    branch,
                )?;
            }
        } else {
            // Models without row traversal retain their existing scalar path.
            let branch = Branch::Bigram(None);
            expand_span_entries(pricing, &span, model, predecessor, row, trellis, branch)?;
        }
        if index == 0 {
            head_row = loaded;
        }
    }
    // The pin's independent unigram pass uses only the best predecessor
    // (search_unigram2, phonetic_lookup.h:560-572).
    if let Some(predecessor) = beam.first() {
        let row = &mut head_row;
        let branch = Branch::Unigram;
        expand_span_entries(pricing, &span, model, predecessor, row, trellis, branch)?;
    }
    Ok(())
}

/// One trellis step on `branch` from `predecessor` for each eligible entry
/// of the span, in library and range order.
fn expand_span_entries<D, L>(
    pricing: &Pricing<'_, D>,
    span: &std::ops::Range<usize>,
    model: &L,
    predecessor: &BeamEntry,
    row: &mut crate::model::NbestStepRow,
    trellis: &mut Trellis,
    branch: Branch,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
    L: LanguageModel,
{
    for (token, slot) in pricing.ranges.walk_tokens() {
        let Some(entry) = pricing.price(slot, token)?.filter(|entry| eligible(entry)) else {
            continue;
        };
        expand_one(
            entry,
            StepCtx {
                paths: pricing.paths,
                span: span.clone(),
                model,
                facts: pricing.facts,
            },
            predecessor,
            row,
            trellis,
            branch,
        )?;
    }
    Ok(())
}

/// One bigram step that a predecessor's merged row selects: the span entry
/// and the item's `(freq, total)`.
type Selected<'a> = (&'a SpanEntry, (u32, u32));

/// Whether the pin steps this entry: its pronunciation possibility is not
/// below `FLT_EPSILON`.
fn eligible(entry: &SpanEntry) -> bool {
    entry.pronunciation >= f32::EPSILON
}

/// The forced token's steps from one predecessor row `gram`: the row's
/// frequency for `token` (`get_freq`), then every eligible entry of the span
/// for that token, in library and range order.
fn select_forced<'a, D>(
    pricing: &Pricing<'a, D>,
    gram: &MergedGram,
    token: u32,
    selected: &mut Vec<Selected<'a>>,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    let refuse = EngineError::lookup_gram;
    let Some(freq) = gram.get_freq(token).map_err(refuse)? else {
        return Ok(());
    };
    let total = gram.total().map_err(refuse)?;
    for (ranged, slot) in pricing.ranges.walk_tokens() {
        if ranged != token {
            continue;
        }
        if let Some(entry) = pricing.price(slot, ranged)?
            && eligible(entry)
        {
            selected.push((entry, (freq, total)));
        }
    }
    Ok(())
}

/// The ranged steps from one predecessor row `gram`: per library and range
/// in append order, the row's items in the range (`search_into`), each with
/// its own frequency, duplicates included. `items` is scratch.
fn select_ranged<'a, D>(
    pricing: &Pricing<'a, D>,
    gram: &MergedGram,
    items: &mut Vec<(u32, u32)>,
    selected: &mut Vec<Selected<'a>>,
) -> Result<(), EngineError>
where
    D: Dictionary<Syllable = SyllableKey>,
{
    let refuse = EngineError::lookup_gram;
    let entries = pricing.ranges;
    for range in entries.iter() {
        items.clear();
        gram.search_into(range.first(), range.last(), items)
            .map_err(refuse)?;
        if items.is_empty() {
            continue;
        }
        let total = gram.total().map_err(refuse)?;
        // The walk checks the range end only (ngram.cpp:167-173): out of
        // order, it can yield a token below the range's first. The pin steps
        // any yielded token whose phrase spells this span
        // (`bigram_gen_next_step`'s `get_phrase_item` and pronunciation
        // check), so such a token takes this span's entry for it, if any.
        for &(token, freq) in items.iter() {
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
    facts: &'a RefCell<SentenceFacts>,
    paths: &'a SpanPaths,
    span: std::ops::Range<usize>,
    model: &'a L,
}

/// One trellis step on `branch`.
fn expand_one<L: LanguageModel>(
    entry: &SpanEntry,
    ctx: StepCtx<'_, L>,
    predecessor: &BeamEntry,
    row: &mut crate::model::NbestStepRow,
    trellis: &mut Trellis,
    branch: Branch,
) -> Result<(), EngineError> {
    let StepCtx {
        paths,
        span,
        model,
        facts,
    } = ctx;
    let prev = predecessor.value.token;
    let step = model
        .nbest_item_costs_with_facts(
            &prev,
            &entry.token,
            match branch {
                Branch::Bigram(item) => item,
                Branch::Unigram => None,
            },
            row,
            &mut facts.borrow_mut(),
        )
        .map_err(|error| EngineError::row_read(model, &error))?;
    let pronunciation = entry.pronunciation;
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
    // Every priced span reaches a real key in the graph's matrix by
    // walking forward. Its start therefore fits below the reserved SEED_STEP.
    let from = span.start as u32;
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
mod tests;
