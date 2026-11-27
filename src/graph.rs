//! Weighted segment graphs for full-pinyin parsing and decoding.
//!
//! [`SegmentGraph`] stores byte-position nodes and [`Edge`] alternatives.
//! [`EdgeKind`] distinguishes exact, segmentation and incomplete input.
//!
//! `docs/findings/segment-graph.md` specifies what this builds and why. The
//! short version: the pin admits an initial-only key at *any* cursor position,
//! any number of times, and enumerating every path is
//! exponential. A graph over byte positions represents the same path set in
//! `O(n × max_syllable_len)` edges and lets the decoder's `k` bound the
//! output instead.
//!
//! Arenas are index-based: no references between nodes and edges, so the
//! structure is trivially `Clone`, has no lifetime, and its ordering is a
//! function of the input alone.

use crate::OptionBits;
use crate::parser::Completeness;
use crate::syllables::MAX_SYLLABLE_LEN;
use crate::vocab::SyllableKey;

/// Largest input whose byte positions fit an [`Edge`].
///
/// This is the width of the graph's positions, not a product limit: an
/// [`Edge`] keeps its positions in `u32`s, and `u32::MAX` stays free for
/// the decoder's seed step. It is wider than the pin's own parser, whose
/// lengths are `int`s (`pinyin_parser2.h:131` at 074a2219). No product
/// rule bounds the input
/// (`docs/findings/upstream-divergences.md#the-full-pinyin-parses-16-bit-steps`).
pub const MAX_GRAPH_POSITION: usize = u32::MAX as usize - 1;

/// The edge vector's starting capacity: one edge per node, at most this
/// many. It bounds the reservation made before any edge exists, not the
/// input; past it the vector doubles as edges are emitted, so the peak
/// stays within about twice the edges the input actually produces.
const INITIAL_EDGE_CAPACITY: usize = 4_096;

/// A pre-parsed syllable, its tone and the byte span of its text. A separating apostrophe
/// run belongs to the following graph edge and is excluded from this span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExactSegment {
    start: usize,
    end: usize,
    key: SyllableKey,
    tone: u8,
}

impl ExactSegment {
    /// Stores the byte span `start..end`, key and tone without validation. Supply nonempty,
    /// ordered, nonoverlapping spans inside the input, with only apostrophes in the gaps.
    #[must_use]
    pub const fn new(start: usize, end: usize, key: SyllableKey, tone: u8) -> Self {
        Self {
            start,
            end,
            key,
            tone,
        }
    }

    /// Byte offset where the key's own text begins.
    #[must_use]
    pub const fn start(&self) -> usize {
        self.start
    }

    /// Byte offset one past the key's own text.
    #[must_use]
    pub const fn end(&self) -> usize {
        self.end
    }

    /// The tone consumed with the key (`0` when toneless).
    #[cfg(test)]
    #[must_use]
    pub const fn tone(&self) -> u8 {
        self.tone
    }
}

/// Classifies a match as a complete syllable, a shorter segmentation alternative or an
/// initial.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum EdgeKind {
    /// The longest complete syllable that matches at this position.
    Exact,
    /// A shorter complete syllable at a position where a longer one also
    /// matches — the alternative a segmentation choice takes.
    Segmentation,
    /// An initial-only key.
    Incomplete,
}

/// A key match spanning a byte range. [`Self::from`] precedes [`Self::syllable_start`] by
/// the length of any apostrophe run consumed before the syllable.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Edge {
    from: u32,
    to: u32,
    syllable_start: u32,
    key: SyllableKey,
    kind: EdgeKind,
    tone: u8,
    distance: u8,
}

impl Edge {
    /// Node this edge leaves.
    #[must_use]
    pub const fn from(&self) -> usize {
        self.from as usize
    }

    /// Node this edge enters.
    #[must_use]
    pub const fn to(&self) -> usize {
        self.to as usize
    }

    /// Byte offset where the key's own text begins.
    #[must_use]
    pub const fn syllable_start(&self) -> usize {
        self.syllable_start as usize
    }

    /// The key this edge matched.
    #[must_use]
    pub const fn key(&self) -> SyllableKey {
        self.key
    }

    /// What kind of match this is.
    #[must_use]
    pub const fn kind(&self) -> EdgeKind {
        self.kind
    }

    /// The tone consumed with this key under `USE_TONE`, `0` when the span
    /// carried no trailing digit (`ChewingKey.m_tone`; `CHEWING_ZERO_TONE`
    /// is 0).
    #[must_use]
    pub const fn tone(&self) -> u8 {
        self.tone
    }

    /// The matched index row's correction distance: 1 for a
    /// `PINYIN_CORRECT_*` or `PINYIN_AMB_S_SH` / `PINYIN_AMB_Z_ZH` alias,
    /// 0 otherwise ([`SyllableKey::from_option_text_with_distance`]).
    /// Pre-parsed edges (`SegmentGraph::build_exact`) are 0.
    #[must_use]
    pub const fn distance(&self) -> u8 {
        self.distance
    }

    /// Whether a separator byte was consumed before the key.
    #[must_use]
    pub(crate) const fn crosses_separator(&self) -> bool {
        self.syllable_start > self.from
    }
}

/// A weighted directed acyclic graph over the byte positions of one input.
///
/// Nodes are `0..=input.len()`. Every edge runs strictly forward, so the graph
/// is acyclic by construction and needs no cycle check.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SegmentGraph {
    edges: Vec<Edge>,
    /// `starts[node]..starts[node + 1]` indexes [`SegmentGraph::edges`],
    /// one entry per node and one past the end.
    starts: Vec<usize>,
    consumed: usize,
}

impl SegmentGraph {
    /// Builds the segmentation graph for the supplied bytes and parser options.
    ///
    /// Options control spelling aliases and tone digits. The graph retains initial-only
    /// edges. [`Self::fewest_keys`] filters them when selecting a path. Matrix-level fuzzy
    /// alternatives are added by the engine.
    ///
    /// Returns `None` if the input exceeds [`MAX_GRAPH_POSITION`].
    /// Unrecognized bytes can leave the graph with no reachable keys.
    #[must_use]
    pub fn build_with_options(input: &[u8], options: OptionBits) -> Option<Self> {
        if input.len() > MAX_GRAPH_POSITION {
            return None;
        }

        let node_count = input.len() + 1;
        // One edge per node up to `INITIAL_EDGE_CAPACITY`, and the vector
        // grows past it. Growing from empty cost short inputs up to a third
        // more time in `core_graph_build`; a long apostrophe or junk input,
        // which emits few or no edges, reserves no more than the constant.
        let mut edges: Vec<Edge> = Vec::with_capacity(node_count.clamp(8, INITIAL_EDGE_CAPACITY));
        let mut starts = Vec::with_capacity(node_count + 1);

        // One forward pass over each apostrophe run keeps the build linear:
        // every node inside a run shares the run's end.
        let mut run_end = 0;
        for node in 0..node_count {
            starts.push(edges.len());
            if let Some(syllable_start) = key_start(input, node, &mut run_end) {
                emit_edges(input, node, syllable_start, options, &mut edges);
            }
        }
        starts.push(edges.len());

        let mut reachable = vec![false; node_count];
        reachable[0] = true;
        let mut consumed = 0;
        for node in 0..node_count {
            if !reachable[node] {
                continue;
            }
            consumed = node;
            // The pin's `'` propagation (`pinyin_parser2.cpp:237-251`): an
            // apostrophe at a reachable position advances the parse one
            // byte, unconditionally and counted — so a trailing or
            // standalone run is consumed even though no key covers it
            // (`ni'` consumes 3, `'''` consumes 3).
            if node + 1 < node_count && input[node] == b'\'' {
                reachable[node + 1] = true;
            }
            let range = starts[node]..starts[node + 1];
            for edge in &edges[range] {
                reachable[edge.to()] = true;
            }
        }

        Some(Self {
            edges,
            starts,
            consumed,
        })
    }

    /// Builds the graph for an already-parsed input: one [`EdgeKind::Exact`]
    /// edge per [`ExactSegment`], and nothing else.
    ///
    /// This is the decoder input for schemes whose keys come from their own
    /// parser — zhuyin, double pinyin, and the Luoma and secondary-zhuyin
    /// full-pinyin indexes. Upstream's decoder receives those
    /// exact `ChewingKey`s and never re-segments them, so the joined
    /// full-pinyin text must not go through [`Self::build_with_options`]:
    /// the pinyin inventory would re-split ambiguous spellings (`bie` into
    /// `bi`+`e`) and truncate spellings that exist only in the scheme's own
    /// index (`den`, `nia`, `eng` — no pinyin key, so the text would
    /// re-parse as a shorter syllable plus leftovers).
    ///
    /// A separator byte between two segments rides on the following edge
    /// (`from` = the previous segment's end), matching the parsed graph's
    /// `crosses_separator` convention.
    ///
    /// Returns `None` beyond [`MAX_GRAPH_POSITION`] or for out-of-order,
    /// overlapping, out-of-bounds or empty spans, or non-apostrophe gaps.
    pub(crate) fn build_exact(input: &[u8], segments: &[ExactSegment]) -> Option<Self> {
        if input.len() > MAX_GRAPH_POSITION {
            return None;
        }

        let mut edges: Vec<Edge> = Vec::with_capacity(segments.len());
        let mut previous_end = 0_usize;
        for segment in segments {
            if segment.end <= segment.start
                || segment.start < previous_end
                || segment.end > input.len()
                || !input[previous_end..segment.start]
                    .iter()
                    .all(|byte| *byte == b'\'')
            {
                return None;
            }
            // The first segment starts at node 0: `previous_end` is 0 there.
            let from = previous_end;
            let (Ok(from), Ok(to), Ok(start)) = (
                u32::try_from(from),
                u32::try_from(segment.end),
                u32::try_from(segment.start),
            ) else {
                return None;
            };
            edges.push(Edge {
                from,
                to,
                syllable_start: start,
                key: segment.key,
                kind: EdgeKind::Exact,
                tone: segment.tone,
                distance: 0,
            });
            previous_end = segment.end;
        }

        let node_count = input.len() + 1;
        let mut starts = Vec::with_capacity(node_count + 1);
        let mut cursor = 0_usize;
        for node in 0..=node_count {
            while cursor < edges.len() && edges[cursor].from() < node {
                cursor += 1;
            }
            starts.push(cursor);
        }

        let mut reachable = vec![false; node_count];
        reachable[0] = true;
        let mut consumed = 0_usize;
        for edge in &edges {
            if reachable[edge.from()] {
                reachable[edge.to()] = true;
                consumed = consumed.max(edge.to());
            }
        }

        Some(Self {
            edges,
            starts,
            consumed,
        })
    }

    /// Number of nodes: one per input byte, plus the end node.
    #[cfg(test)]
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.starts.len() - 1
    }

    /// Every edge, ordered by source node ascending then key length
    /// descending.
    #[cfg(test)]
    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// Edges leaving `node`, in the graph's frozen order.
    ///
    /// An out-of-range node has no edges rather than panicking.
    #[must_use]
    pub fn outgoing(&self, node: usize) -> &[Edge] {
        let Some(start) = self.starts.get(node) else {
            return &[];
        };
        let Some(end) = self.starts.get(node + 1) else {
            return &[];
        };
        &self.edges[*start..*end]
    }

    /// Returns the furthest reachable byte position, before incomplete edges are filtered.
    ///
    /// For a graph from [`Self::build_with_options`], this includes reachable trailing
    /// apostrophes and can be nonzero for input that is only separators. For example,
    /// `ni'` and `'''` each give 3.
    ///
    /// For a graph from exact segments, it stops at the end of the last reachable segment.
    /// A separator after that segment is not counted. For example, `b"ni'"` with one
    /// segment `0..2` gives 2.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    // FullPinyinParser2::final_step, pinyin_parser2.cpp:237–251,291,305–309,346–380;
    // docs/findings/candidate-construction.md §8.1.
    /// Selects a path covering the longest byte prefix. Among these paths it prefers fewer
    /// keys, then smaller total [`Edge::distance`], then the first path found. Initial-only
    /// edges participate when `options` holds [`OptionBits::PINYIN_INCOMPLETE`]. Separators
    /// contribute to the consumed prefix without emitting a key.
    #[must_use]
    pub fn fewest_keys(&self, options: OptionBits) -> Vec<Edge> {
        let allow_incomplete = options.contains(OptionBits::PINYIN_INCOMPLETE);
        let bound = self.consumed();
        // A step's rank is (key count, total correction distance) packed into
        // one integer, the count above bit 32: one comparison orders them
        // lexicographically. Both are bounded by the input length, which
        // `MAX_GRAPH_POSITION` keeps below 2^32, so the distance never carries
        // into the count.
        const ONE_KEY: u64 = 1 << 32;
        // steps[node] = (rank, incoming edge); the incoming edge's `from` is
        // the previous node. Node 0 is the root with rank zero.
        let mut steps: Vec<Option<(u64, Edge)>> = vec![None; bound + 1];

        for node in 0..bound {
            let rank = if node == 0 {
                0
            } else {
                match steps[node] {
                    Some((rank, _)) => rank,
                    None => continue,
                }
            };
            // At a fixed node each length matches at most one edge, so the
            // edges here reach distinct nodes and share this node's rank.
            // Within-node order cannot change the selection (competition is
            // resolved across nodes by the strict `candidate < seen` check in
            // ascending node order), so no per-node sort is needed.
            //
            // Every reachable step at a node has parsed the same length, so
            // the pin's three rules (`pinyin_parser2.cpp:296-309` at
            // 074a2219) reduce to (fewer keys, then lower distance), the
            // packed rank's order.
            for edge in self
                .outgoing(node)
                .iter()
                .filter(|edge| allow_incomplete || edge.kind() != EdgeKind::Incomplete)
                .copied()
            {
                let to = edge.to();
                if to > bound {
                    continue;
                }
                let candidate = rank + ONE_KEY + u64::from(edge.distance());
                let replace = steps[to].as_ref().is_none_or(|(seen, _)| candidate < *seen);
                if replace {
                    steps[to] = Some((candidate, edge));
                }
            }
        }

        // `consumed()` includes incomplete edges. Reconstruct from the
        // furthest node the *filtered* DP reached (`final_step`).
        let mut node = bound;
        while node > 0 && steps[node].is_none() {
            node -= 1;
        }

        let mut path = Vec::new();
        while node > 0 {
            let Some((_, edge)) = steps[node] else {
                break;
            };
            path.push(edge);
            node = edge.from();
        }
        path.reverse();
        path
    }
}

/// Emits every edge leaving `node`, longest key first.
///
/// At a fixed position a given length matches at most one key: the canonical,
/// initial-only, and option-gated alias spellings are disjoint exact strings.
/// So one edge per length, and the emission order is total.
fn emit_edges(
    input: &[u8],
    node: usize,
    syllable_start: usize,
    options: OptionBits,
    edges: &mut Vec<Edge>,
) {
    let available = input.len() - syllable_start;
    // The pin always tries max_full_pinyin_length = 7 (including tone),
    // pinyin_parser2.cpp:82,269. A toneless span of seven bytes can still
    // parse when strcmp stops at an embedded NUL before its explicit end.
    let use_tone = options.contains(OptionBits::USE_TONE);
    let force_tone = use_tone && options.contains(OptionBits::FORCE_TONE);
    let max_span = MAX_SYLLABLE_LEN + 1;
    // Resolve each bounded span once. Classification needs the longest
    // complete key before emission, but a second lookup would repeat UTF-8
    // validation, PHF hashing and option-alias work for the same spelling.
    // The fixed-size stack array keeps graph construction O(n * max_span)
    // in time and O(n * max_span) in space, with unchanged edge order.
    let parsed_spans: [Option<(SyllableKey, u8, u8)>; MAX_SYLLABLE_LEN + 1] =
        core::array::from_fn(|index| {
            let length = index + 1;
            if length > max_span.min(available) {
                return None;
            }
            let text = &input[syllable_start..syllable_start + length];
            let (core, tone) = tone_split(text, use_tone, force_tone)?;
            key_for_spelling(core, options).map(|(key, distance)| (key, tone, distance))
        });
    let longest_complete = parsed_spans
        .iter()
        .rposition(|span| {
            span.is_some_and(|(key, _, _)| key.completeness() == Completeness::Complete)
        })
        .map(|index| index + 1);

    for (index, span) in parsed_spans.into_iter().enumerate().rev() {
        let Some((key, tone, distance)) = span else {
            continue;
        };
        let length = index + 1;
        let end = syllable_start + length;
        let kind = match key.completeness() {
            Completeness::Complete => {
                if longest_complete == Some(length) {
                    EdgeKind::Exact
                } else {
                    EdgeKind::Segmentation
                }
            }
            Completeness::Partial => EdgeKind::Incomplete,
        };
        let (Ok(from), Ok(to), Ok(start)) = (
            u32::try_from(node),
            u32::try_from(end),
            u32::try_from(syllable_start),
        ) else {
            continue;
        };

        edges.push(Edge {
            from,
            to,
            syllable_start: start,
            key,
            kind,
            tone,
            distance,
        });
    }
}

/// `FullPinyinParser2::parse_one_key`'s tone scan
/// (`pinyin_parser2.cpp:176-190`): under `USE_TONE` a span whose last byte
/// is an ASCII `1..=5` is that syllable's tone and is consumed with the
/// span; the remaining C-string prefix must be lowercase letters. The
/// digit-stripped core then goes through the ordinary option-gated lookup —
/// completeness is *not* a precondition, so an initial-only key carries a
/// tone exactly like a complete one. Any other digit (`0`, `6`–`9`) stays
/// in the core, fails the lookup, and the span falls back to the shorter
/// toneless parse.
///
/// `force_tone` carries the pin's `FORCE_TONE` rejection, nested inside the
/// same `USE_TONE` branch (`pinyin_parser2.cpp:185-189`): a span without a
/// tone digit does not parse at all. Dead without `USE_TONE` — the caller
/// derives it as `use_tone && FORCE_TONE`.
fn tone_split(text: &[u8], use_tone: bool, force_tone: bool) -> Option<(&[u8], u8)> {
    let (core, tone) = match text.last().copied() {
        Some(digit @ b'1'..=b'5') if use_tone => (&text[..text.len() - 1], digit - b'0'),
        _ => (text, 0),
    };
    if force_tone && tone == 0 {
        return None;
    }
    // parse_one_key's g_strndup keeps the explicit span, while the index
    // lookup's strcmp stops at NUL (pinyin_parser2.cpp:169-210, 074a2219).
    let core = &core[..core
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(core.len())];
    if !core.is_empty() && core.iter().all(u8::is_ascii_lowercase) {
        Some((core, tone))
    } else {
        None
    }
}

/// Where a key may begin for an edge leaving `node`, or `None` if none can.
///
/// Upstream's `FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`)
/// treats every apostrophe as a zero-width step-propagation: if a
/// position is reachable and its byte is `'`, the next position is also
/// reachable. This applies at *every* reachable node, including node 0.
/// A *run* of consecutive apostrophes collapses to a single hop, so
/// `'ni` and `ni''hao` both consume their full input on the pin side.
/// Skip the whole run and let the key begin at the next byte.
///
/// `run_end` caches the end of the run last scanned, so the nodes of one
/// run share a single scan and a build stays linear in the input length.
fn key_start(input: &[u8], node: usize, run_end: &mut usize) -> Option<usize> {
    let byte = *input.get(node)?;
    if byte == b'\'' {
        if *run_end <= node {
            *run_end = input[node..]
                .iter()
                .position(|byte| *byte != b'\'')
                .map_or(input.len(), |offset| node + offset);
        }
        return (*run_end < input.len()).then_some(*run_end);
    }
    Some(node)
}

fn key_for_spelling(text: &[u8], options: OptionBits) -> Option<(SyllableKey, u8)> {
    core::str::from_utf8(text)
        .ok()
        .and_then(|spelling| SyllableKey::from_option_text_with_distance(spelling, options))
}

#[cfg(test)]
mod tests;
