//! The weighted segment graph.
//!
//! `docs/findings/segment-graph.md` freezes what this builds and why. The
//! short version: the pin admits an initial-only key at *any* cursor position,
//! any number of times, and enumerating that as an explicit `Vec<Parse>` is
//! exponential. A graph over byte positions represents the same path set in
//! `O(n × max_syllable_len)` edges and lets the decoder's `k` bound the
//! output instead.
//!
//! Arenas are index-based: no references between nodes and edges, so the
//! structure is trivially `Clone`, has no lifetime, and its ordering is a
//! function of the input alone.

use core::fmt;

use crate::{Completeness, MAX_SYLLABLE_LEN, OptionBits, SyllableKey, USE_TONE};

/// Largest input the graph accepts, in bytes.
///
/// The pinned oracle reports positions as `guint16`, so an input beyond this
/// cannot be compared against it at all. Refusing here keeps the graph and the
/// differential runner agreeing about what is representable.
pub const MAX_GRAPH_INPUT: usize = 65_535;

/// Why a graph could not be built.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum GraphError {
    /// The input is longer than [`MAX_GRAPH_INPUT`].
    InputTooLong {
        /// Length that was offered.
        len: usize,
        /// Largest length accepted.
        limit: usize,
    },
}

impl fmt::Display for GraphError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InputTooLong { len, limit } => {
                write!(
                    formatter,
                    "input of {len} bytes exceeds the {limit}-byte limit"
                )
            }
        }
    }
}

impl std::error::Error for GraphError {}

/// What kind of match an edge represents.
///
/// `Fuzzy`, `Typo` and `Abbrev` are reserved for Stage 2 and deliberately not
/// declared: an unimplemented variant is a promise the decoder would have to
/// keep. The enum is `#[non_exhaustive]` so adding them is not a break.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[non_exhaustive]
pub enum EdgeKind {
    /// The longest complete syllable that matches at this position.
    Exact,
    /// A shorter complete syllable at a position where a longer one also
    /// matches — the alternative a segmentation choice takes.
    Segmentation,
    /// An initial-only key.
    Incomplete,
}

impl EdgeKind {
    /// Wire spelling, matching the `pinyin-capture-v1` completeness tokens
    /// where they overlap.
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Segmentation => "segmentation",
            Self::Incomplete => "incomplete",
        }
    }

    /// Whether an edge of this kind carries a complete syllable.
    #[must_use]
    pub const fn completeness(self) -> Completeness {
        match self {
            Self::Exact | Self::Segmentation => Completeness::Complete,
            Self::Incomplete => Completeness::Partial,
        }
    }
}

/// Index of an edge in a [`SegmentGraph`].
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EdgeId(u32);

impl EdgeId {
    /// The index as a `usize`.
    #[must_use]
    pub const fn index(self) -> usize {
        self.0 as usize
    }
}

/// One key match spanning a byte range.
///
/// `from` may be one byte before `syllable_start`: a consumed apostrophe
/// separator rides on the edge that follows it, so the graph stays a plain
/// walk over byte positions while the reported segment range still matches the
/// capture notation (`chang'an` reports `an@6:8`).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Edge {
    from: u32,
    to: u32,
    syllable_start: u32,
    key: SyllableKey,
    kind: EdgeKind,
    tone: u8,
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

    /// Whether a separator byte was consumed before the key.
    #[must_use]
    pub const fn crosses_separator(&self) -> bool {
        self.syllable_start > self.from
    }
}

/// A weighted directed acyclic graph over the byte positions of one input.
///
/// Nodes are `0..=input.len()`. Every edge runs strictly forward, so the graph
/// is acyclic by construction and needs no cycle check.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SegmentGraph {
    input_len: usize,
    edges: Vec<Edge>,
    /// `starts[node]..starts[node + 1]` indexes [`SegmentGraph::edges`].
    starts: Vec<u32>,
    reachable: Vec<bool>,
    consumed: usize,
}

impl SegmentGraph {
    /// Builds the graph for `input`.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::InputTooLong`] for an input beyond
    /// [`MAX_GRAPH_INPUT`]. Every other byte sequence — junk, malformed UTF-8,
    /// stray apostrophes, the empty input — builds a graph, possibly one with
    /// no edges at all.
    pub fn build(input: &[u8]) -> Result<Self, GraphError> {
        Self::build_with_options(input, OptionBits::default())
    }

    /// Builds the graph for `input` under parser option bits.
    ///
    /// [`Self::build`] is this method with [`OptionBits::default`], so the
    /// all-off path stays bit-identical to the frozen parser. The option bits
    /// only admit correction/ambiguity *spellings*; matrix-level fuzzy
    /// alternates are added later by the engine scan matrix.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::InputTooLong`] for an input beyond
    /// [`MAX_GRAPH_INPUT`].
    pub fn build_with_options(input: &[u8], options: OptionBits) -> Result<Self, GraphError> {
        if input.len() > MAX_GRAPH_INPUT {
            return Err(GraphError::InputTooLong {
                len: input.len(),
                limit: MAX_GRAPH_INPUT,
            });
        }

        let node_count = input.len() + 1;
        let mut edges: Vec<Edge> =
            Vec::with_capacity(input.len().saturating_mul(MAX_SYLLABLE_LEN).max(8));
        let mut starts = Vec::with_capacity(node_count + 1);

        for node in 0..node_count {
            starts.push(u32::try_from(edges.len()).unwrap_or(u32::MAX));
            emit_edges(input, node, options, &mut edges);
        }
        starts.push(u32::try_from(edges.len()).unwrap_or(u32::MAX));

        let mut reachable = vec![false; node_count];
        reachable[0] = true;
        let mut consumed = 0;
        for node in 0..node_count {
            if !reachable[node] {
                continue;
            }
            consumed = node;
            let range = starts[node] as usize..starts[node + 1] as usize;
            for edge in &edges[range] {
                reachable[edge.to()] = true;
            }
        }

        Ok(Self {
            input_len: input.len(),
            edges,
            starts,
            reachable,
            consumed,
        })
    }

    /// Length of the input this graph was built for.
    #[must_use]
    pub const fn input_len(&self) -> usize {
        self.input_len
    }

    /// Number of nodes, which is `input_len() + 1`.
    #[must_use]
    pub const fn node_count(&self) -> usize {
        self.input_len + 1
    }

    /// Every edge, ordered by source node ascending then key length
    /// descending.
    #[must_use]
    pub fn edges(&self) -> &[Edge] {
        &self.edges
    }

    /// The edge with this id, or `None` when the id is not from this graph.
    #[must_use]
    pub fn edge(&self, id: EdgeId) -> Option<&Edge> {
        self.edges.get(id.index())
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
        &self.edges[*start as usize..*end as usize]
    }

    /// Ids of the edges leaving `node`.
    pub fn outgoing_ids(&self, node: usize) -> impl Iterator<Item = EdgeId> + use<> {
        let start = self.starts.get(node).copied().unwrap_or(0);
        let end = self
            .starts
            .get(node + 1)
            .copied()
            .unwrap_or(start)
            .max(start);
        (start..end).map(EdgeId)
    }

    /// Whether `node` can be reached from the start of the input.
    #[must_use]
    pub fn is_reachable(&self, node: usize) -> bool {
        self.reachable.get(node).copied().unwrap_or(false)
    }

    /// The furthest node reachable from the start on the full graph.
    ///
    /// Incomplete edges always participate. Filtered parse length is the
    /// last byte of [`SegmentGraph::fewest_keys`].
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// Whether every byte of the input is covered by some path.
    #[must_use]
    pub const fn fully_consumed(&self) -> bool {
        self.consumed == self.input_len
    }

    /// Fewest-keys path under the incomplete-edge filter.
    ///
    /// Longest prefix that a filtered path from byte zero actually covers,
    /// then fewest keys, first-found ties — `pinyin_parser2.cpp` `final_step`
    /// plus `candidate-construction.md` §8.1. Incomplete edges are included
    /// only when `allow_incomplete`. Empty when no such path exists,
    /// including the empty input.
    #[must_use]
    pub fn fewest_keys(&self, allow_incomplete: bool) -> Vec<Edge> {
        let bound = self.consumed();
        // steps[node] = (key count, incoming edge, previous node); node 0 is
        // the root with count zero.
        let mut steps: Vec<Option<(usize, Edge, usize)>> = vec![None; bound + 1];

        for node in 0..bound {
            let count = if node == 0 {
                0
            } else {
                match steps[node] {
                    Some((count, _, _)) => count,
                    None => continue,
                }
            };
            // At a fixed node each length matches at most one edge, so the
            // edges here reach distinct nodes and share this node's key count.
            // Within-node order cannot change the selection (competition is
            // resolved across nodes by the strict `candidate < seen` check in
            // ascending node order), so no per-node sort is needed.
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
                let candidate = count + 1;
                let replace = steps[to]
                    .as_ref()
                    .is_none_or(|(seen, _, _)| candidate < *seen);
                if replace {
                    steps[to] = Some((candidate, edge, node));
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
            let Some((_, edge, previous)) = steps[node] else {
                break;
            };
            path.push(edge);
            node = previous;
        }
        path.reverse();
        path
    }

    /// Finds the edge leaving `from` that matches this segment exactly.
    ///
    /// The byte range is the key's own range, as the capture notation writes
    /// it, so a segment after an apostrophe is looked up by where its text
    /// starts rather than where its edge does.
    #[must_use]
    pub fn find_edge(
        &self,
        from: usize,
        key: SyllableKey,
        syllable_start: usize,
    ) -> Option<EdgeId> {
        self.outgoing_ids(from).find(|id| {
            self.edge(*id)
                .is_some_and(|edge| edge.key == key && edge.syllable_start() == syllable_start)
        })
    }
}

/// Fewest-keys complete-syllable parse of a pinyin string.
///
/// Longest parsed prefix from byte zero, fewest complete keys to reach
/// it, first-found ties. Trailing bytes that do not extend the reachable
/// prefix are ignored. Incomplete initial-only keys are not admitted —
/// the import ABI and the engine's complete-keys walk share this
/// selection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FewestKeys {
    keys: Vec<SyllableKey>,
}

impl FewestKeys {
    /// Parse `pinyin`.
    ///
    /// `None` when the input exceeds [`MAX_GRAPH_INPUT`] or no complete-key
    /// path reaches the furthest reachable node.
    #[must_use]
    pub fn parse(pinyin: &str) -> Option<Self> {
        let graph = SegmentGraph::build(pinyin.as_bytes()).ok()?;
        let edges = graph.fewest_keys(false);
        if edges.is_empty() && graph.consumed() > 0 {
            return None;
        }
        Some(Self {
            keys: edges.iter().map(Edge::key).collect(),
        })
    }

    /// The selected keys, in input order.
    #[must_use]
    pub fn keys(&self) -> &[SyllableKey] {
        &self.keys
    }

    /// `'`-joined syllable spellings (`ni'hao`), matching a stored
    /// pronunciation row.
    #[must_use]
    pub fn canonical(&self) -> String {
        self.keys
            .iter()
            .map(|key| key.text())
            .collect::<Vec<_>>()
            .join("'")
    }
}

/// Emits every edge leaving `node`, longest key first.
///
/// At a fixed position a given length matches at most one key: the canonical,
/// initial-only, and option-gated alias spellings are disjoint exact strings.
/// So one edge per length, and the emission order is total.
fn emit_edges(input: &[u8], node: usize, options: OptionBits, edges: &mut Vec<Edge>) {
    let Some(syllable_start) = key_start(input, node) else {
        return;
    };

    let available = input.len() - syllable_start;
    // The pin's window is `max_full_pinyin_length = 7`, whose own comment
    // says "include tone" (`pinyin_parser2.cpp:82`): one byte beyond the
    // longest 6-letter syllable, reachable only through the `USE_TONE`
    // digit scan. With the bit clear the window stays 6 and no digit ever
    // joins a span, so the toneless edge set is bit-identical to the
    // frozen parser.
    let use_tone = options.contains(USE_TONE);
    let max_span = MAX_SYLLABLE_LEN + usize::from(use_tone);
    let mut longest_complete = None;

    // Each `lookup` below is a linear scan of the 405-entry inventory, so
    // building the graph is O(n × MAX_SYLLABLE_LEN × 405) — acceptable for W4;
    // trie lookup is Stage 2 if this becomes hot. It has not been: the whole
    // 10,459-input corpus differential builds a graph per input in about ten
    // seconds, and a session's inputs are a few dozen bytes.
    //
    // Two passes so `Exact` can mean "the longest complete syllable here"
    // without the caller having to look at its neighbours.
    for length in (1..=max_span.min(available)).rev() {
        let text = &input[syllable_start..syllable_start + length];
        let Some((core, _)) = tone_split(text, use_tone) else {
            continue;
        };
        if longest_complete.is_none()
            && key_for_spelling(core, options)
                .is_some_and(|key| key.completeness() == Completeness::Complete)
        {
            longest_complete = Some(length);
            break;
        }
    }

    for length in (1..=max_span.min(available)).rev() {
        let end = syllable_start + length;
        let text = &input[syllable_start..end];
        let Some((core, tone)) = tone_split(text, use_tone) else {
            continue;
        };

        let Some(key) = key_for_spelling(core, options) else {
            continue;
        };
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
        });
    }
}

/// `FullPinyinParser2::parse_one_key`'s tone scan
/// (`pinyin_parser2.cpp:176-190`): under `USE_TONE` a span whose last byte
/// is an ASCII `1..=5` is that syllable's tone and is consumed with the
/// span; every remaining byte must be a lowercase letter. The
/// digit-stripped core then goes through the ordinary option-gated lookup —
/// completeness is *not* a precondition, so an initial-only key carries a
/// tone exactly like a complete one. Any other digit (`0`, `6`–`9`) stays
/// in the core, fails the lookup, and the span falls back to the shorter
/// toneless parse.
fn tone_split(text: &[u8], use_tone: bool) -> Option<(&[u8], u8)> {
    let (core, tone) = match text.last().copied() {
        Some(digit @ b'1'..=b'5') if use_tone => (&text[..text.len() - 1], digit - b'0'),
        _ => (text, 0),
    };
    if !core.is_empty() && core.iter().all(u8::is_ascii_lowercase) {
        Some((core, tone))
    } else {
        None
    }
}

/// Where a key may begin for an edge leaving `node`, or `None` if none can.
///
/// An apostrophe at `node` is a separator and the key begins after it. A
/// *leading* apostrophe is not a separator: `parser-path-set.md` freezes it
/// as remainder-starting, and half of maintainer decision 3 in
/// `parser-spec-contradiction-incomplete-keys.md` — the leading-apostrophe
/// half — is still open. The graph does not quietly settle it.
///
/// A *run* of consecutive apostrophes after a consumed group IS a separator:
/// upstream's `FullPinyinParser2::parse` (`pinyin_parser2.cpp:237-250`)
/// treats every apostrophe as a zero-width step-propagation, so `ni''hao`
/// consumes the whole input on the pin side. Skip the whole run and let the
/// key begin at the next lowercase byte.
fn key_start(input: &[u8], node: usize) -> Option<usize> {
    let byte = *input.get(node)?;
    if byte == b'\'' {
        if node == 0 {
            return None;
        }
        let mut cursor = node + 1;
        while *input.get(cursor)? == b'\'' {
            cursor += 1;
        }
        return Some(cursor);
    }
    Some(node)
}

fn key_for_spelling(text: &[u8], options: OptionBits) -> Option<SyllableKey> {
    core::str::from_utf8(text)
        .ok()
        .and_then(|spelling| SyllableKey::from_option_text(spelling, options))
}

#[cfg(test)]
mod tests {
    use super::{EdgeKind, FewestKeys, GraphError, MAX_GRAPH_INPUT, SegmentGraph};
    use crate::{OptionBits, SyllableKey, USE_TONE};

    /// Renders the graph as `from-to:key:kind`, one edge per entry.
    fn rendered(input: &str) -> Vec<String> {
        SegmentGraph::build(input.as_bytes())
            .expect("the test inputs are short")
            .edges()
            .iter()
            .map(|edge| {
                format!(
                    "{}-{}:{}:{}",
                    edge.from(),
                    edge.to(),
                    edge.key().text(),
                    edge.kind().as_wire()
                )
            })
            .collect()
    }

    /// Renders the graph as `from-to:key:kind:tone` under `options`.
    fn rendered_with_options(input: &str, options: OptionBits) -> Vec<String> {
        SegmentGraph::build_with_options(input.as_bytes(), options)
            .expect("the test inputs are short")
            .edges()
            .iter()
            .map(|edge| {
                format!(
                    "{}-{}:{}:{}:{}",
                    edge.from(),
                    edge.to(),
                    edge.key().text(),
                    edge.kind().as_wire(),
                    edge.tone()
                )
            })
            .collect()
    }

    /// `PINYIN_INCOMPLETE | USE_TONE` — the W15 tone profile.
    fn tone_options() -> OptionBits {
        OptionBits::from_bits(crate::PINYIN_INCOMPLETE | USE_TONE)
    }

    #[test]
    fn use_tone_consumes_a_trailing_digit_with_the_span() {
        // `pinyin_parser2.cpp:176-214`: the digit is inside the consumed
        // span, rides the key as its tone, and outranks the shorter
        // toneless parse at the same position. The shorter parses stay
        // available as segmentation alternatives.
        assert_eq!(
            rendered_with_options("zai4", tone_options()),
            [
                "0-4:zai:exact:4",
                "0-3:zai:segmentation:0",
                "0-2:za:segmentation:0",
                "0-1:z:incomplete:0",
                "1-4:ai:exact:4",
                "1-3:ai:segmentation:0",
                "1-2:a:segmentation:0",
            ]
        );
    }

    #[test]
    fn use_tone_reaches_the_seventh_byte_of_a_span() {
        // `max_full_pinyin_length = 7` "include[s] tone"
        // (`pinyin_parser2.cpp:82`): zhuang4 is a 7-byte span and its
        // toneless 6-byte sibling becomes the segmentation alternative.
        let edges = rendered_with_options("zhuang4", tone_options());
        assert!(edges.contains(&"0-7:zhuang:exact:4".to_owned()));
        assert!(edges.contains(&"0-6:zhuang:segmentation:0".to_owned()));
        assert!(
            SegmentGraph::build_with_options(b"zhuang4", tone_options())
                .expect("valid")
                .fully_consumed()
        );
    }

    #[test]
    fn use_tone_rejects_the_non_tone_digits() {
        // 0 and 6-9 are not tones: they stay in the core, fail the lookup,
        // and the span falls back to the shorter toneless parse with the
        // digit left as junk.
        for input in ["zai6", "zai0", "zai9"] {
            let graph =
                SegmentGraph::build_with_options(input.as_bytes(), tone_options()).expect("valid");
            assert_eq!(graph.consumed(), 3, "input {input}");
            assert!(!graph.fully_consumed(), "input {input}");
            assert!(
                graph
                    .edges()
                    .iter()
                    .all(|edge| edge.tone() == 0 && edge.to() <= 3),
                "input {input}"
            );
        }
    }

    #[test]
    fn use_tone_leaves_junk_after_the_digit_unparsed() {
        let graph = SegmentGraph::build_with_options(b"zai4Q", tone_options()).expect("valid");
        assert_eq!(graph.consumed(), 4);
        assert!(!graph.fully_consumed());
        let path: Vec<(usize, &str, u8)> = graph
            .fewest_keys(true)
            .iter()
            .map(|edge| (edge.to(), edge.key().text(), edge.tone()))
            .collect();
        assert_eq!(path, [(4, "zai", 4)]);
    }

    #[test]
    fn use_tone_carries_a_digit_over_an_apostrophe_boundary() {
        let graph = SegmentGraph::build_with_options(b"zai4'an", tone_options()).expect("valid");
        assert!(graph.fully_consumed());
        let path: Vec<(&str, u8)> = graph
            .fewest_keys(true)
            .iter()
            .map(|edge| (edge.key().text(), edge.tone()))
            .collect();
        assert_eq!(path, [("zai", 4), ("an", 0)]);
    }

    #[test]
    fn use_tone_admits_a_tone_on_an_incomplete_key_only_with_the_bit() {
        // The scan's only precondition is the option-gated index hit, so
        // the initial-only "n" carries a tone like any complete syllable.
        let edges = rendered_with_options("n4", tone_options());
        assert!(edges.contains(&"0-2:n:incomplete:4".to_owned()));

        // Incomplete edges are emitted unconditionally and filtered by the
        // walk (`fewest_keys(false)`, the session's `walk`), so the tone
        // rides an edge the incomplete-off profile drops whole.
        let without_incomplete = OptionBits::from_bits(USE_TONE);
        let graph = SegmentGraph::build_with_options(b"n4", without_incomplete).expect("valid");
        assert_eq!(
            graph
                .edges()
                .iter()
                .map(|edge| (edge.kind().as_wire(), edge.tone()))
                .collect::<Vec<_>>(),
            [("incomplete", 4), ("incomplete", 0)]
        );
        assert!(graph.fewest_keys(false).is_empty());

        // A lone digit is an empty core and never parses.
        assert!(
            SegmentGraph::build_with_options(b"4", tone_options())
                .expect("valid")
                .edges()
                .is_empty()
        );
    }

    #[test]
    fn use_tone_clear_keeps_the_frozen_edge_set() {
        // The invariant: with the bit clear every toned input builds the
        // same edges the toneless parser always did — no digit spans, no
        // tone fields, no window growth.
        for input in ["zai4", "zhuang4", "n4", "a4", "zai4'an", "ni3hao3"] {
            let frozen = SegmentGraph::build(input.as_bytes()).expect("valid");
            let cleared = SegmentGraph::build_with_options(
                input.as_bytes(),
                OptionBits::from_bits(crate::PINYIN_INCOMPLETE),
            )
            .expect("valid");
            assert_eq!(frozen.edges(), cleared.edges(), "input {input}");
            assert!(cleared.edges().iter().all(|edge| edge.tone() == 0));
        }
    }

    #[test]
    fn the_empty_input_is_one_node_and_no_edges() {
        let graph = SegmentGraph::build(b"").expect("empty is valid");
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edges().len(), 0);
        assert_eq!(graph.consumed(), 0);
        assert!(graph.fully_consumed());
        assert!(graph.is_reachable(0));
        assert!(!graph.is_reachable(1));
        assert!(graph.outgoing(0).is_empty());
        assert!(graph.outgoing(99).is_empty());
    }

    #[test]
    fn a_single_syllable_carries_its_initial_as_an_incomplete_edge() {
        assert_eq!(rendered("ni"), ["0-2:ni:exact", "0-1:n:incomplete"]);
    }

    #[test]
    fn edges_are_ordered_by_position_then_by_descending_length() {
        assert_eq!(
            rendered("nihao"),
            [
                "0-2:ni:exact",
                "0-1:n:incomplete",
                "2-5:hao:exact",
                "2-4:ha:segmentation",
                "2-3:h:incomplete",
                "3-5:ao:exact",
                "3-4:a:segmentation",
                "4-5:o:exact",
            ]
        );
    }

    #[test]
    fn a_longer_complete_match_makes_the_shorter_ones_segmentation() {
        // xian is complete and so is xi, so xi is the segmentation
        // alternative — which is how the pin reaches 西安 for this input.
        let edges = rendered("xian");
        assert!(edges.contains(&"0-4:xian:exact".to_owned()));
        assert!(edges.contains(&"0-2:xi:segmentation".to_owned()));
        assert!(edges.contains(&"0-1:x:incomplete".to_owned()));
        assert!(edges.contains(&"2-4:an:exact".to_owned()));
    }

    #[test]
    fn an_apostrophe_rides_on_the_edge_that_follows_it() {
        let graph = SegmentGraph::build(b"chang'an").expect("valid");
        let crossing: Vec<_> = graph
            .edges()
            .iter()
            .filter(|edge| edge.crosses_separator())
            .map(|edge| {
                (
                    edge.from(),
                    edge.syllable_start(),
                    edge.to(),
                    edge.key().text(),
                )
            })
            .collect();
        assert_eq!(crossing, [(5, 6, 8, "an"), (5, 6, 7, "a")]);
        assert!(graph.fully_consumed());
    }

    #[test]
    fn the_leading_apostrophe_case_is_left_open() {
        // The leading half of maintainer decision 3 in
        // parser-spec-contradiction-incomplete-keys.md is still open; the
        // graph must not settle it by accident.
        let leading = SegmentGraph::build(b"'ni").expect("valid");
        assert_eq!(
            leading.consumed(),
            0,
            "a leading apostrophe is not a separator"
        );

        // The doubled half is now resolved: consecutive apostrophes after a
        // consumed group are a single separator, matching the pin.
        let doubled = SegmentGraph::build(b"ni''hao").expect("valid");
        assert_eq!(
            doubled.consumed(),
            7,
            "consecutive apostrophes act as one separator"
        );

        let trailing = SegmentGraph::build(b"ni'").expect("valid");
        assert_eq!(trailing.consumed(), 2);
    }

    #[test]
    fn junk_stops_the_reachable_prefix_without_stopping_the_build() {
        let graph = SegmentGraph::build(b"ni!hao").expect("valid");
        assert_eq!(graph.consumed(), 2);
        assert!(!graph.fully_consumed());
        assert!(graph.is_reachable(2));
        assert!(!graph.is_reachable(3));

        let leading = SegmentGraph::build(b"!ni").expect("valid");
        assert_eq!(leading.consumed(), 0);
        assert!(leading.edges().iter().all(|edge| edge.from() > 0));
    }

    #[test]
    fn repeated_initials_chain_the_whole_way() {
        // The shape the pin produces for zzzzzzzz and qqqq…, which the
        // foundation parser could only consume one byte of.
        let graph = SegmentGraph::build(b"zzzzzzzz").expect("valid");
        assert_eq!(graph.consumed(), 8);
        assert!(graph.fully_consumed());
        assert!(
            graph
                .edges()
                .iter()
                .all(|edge| edge.kind() == EdgeKind::Incomplete)
        );
    }

    #[test]
    fn a_mid_position_initial_is_an_ordinary_edge() {
        // yingchon: the pin selects ying, ch, o, n — two non-final partials.
        let graph = SegmentGraph::build(b"yingchon").expect("valid");
        for (from, key, start) in [(0, "ying", 0), (4, "ch", 4), (6, "o", 6), (7, "n", 7)] {
            let key = SyllableKey::from_text(key).expect("frozen key");
            assert!(
                graph.find_edge(from, key, start).is_some(),
                "missing edge {from} {}",
                key.text()
            );
        }
        assert!(graph.fully_consumed());
    }

    #[test]
    fn ties_between_equal_length_alternatives_are_ordered_by_position() {
        // fangan admits fang+an, fan+gan and fa+ng+an. Every one of those
        // edges exists, and the order is a function of the input alone.
        let first = rendered("fangan");
        let second = rendered("fangan");
        assert_eq!(first, second);
        for wanted in [
            "0-4:fang:exact",
            "0-3:fan:segmentation",
            "0-2:fa:segmentation",
            "3-6:gan:exact",
            "4-6:an:exact",
            "2-4:ng:exact",
        ] {
            assert!(first.contains(&wanted.to_owned()), "missing {wanted}");
        }
    }

    #[test]
    fn edge_ids_address_the_edges_they_index() {
        let graph = SegmentGraph::build(b"nihao").expect("valid");
        for node in 0..graph.node_count() {
            let by_slice = graph.outgoing(node);
            let by_id: Vec<_> = graph
                .outgoing_ids(node)
                .map(|id| *graph.edge(id).expect("ids come from this graph"))
                .collect();
            assert_eq!(by_slice, by_id.as_slice(), "node {node}");
        }
    }

    #[test]
    fn an_over_long_input_is_refused_rather_than_truncated() {
        let long = vec![b'a'; MAX_GRAPH_INPUT + 1];
        assert_eq!(
            SegmentGraph::build(&long),
            Err(GraphError::InputTooLong {
                len: MAX_GRAPH_INPUT + 1,
                limit: MAX_GRAPH_INPUT,
            })
        );
        assert!(SegmentGraph::build(&long[..MAX_GRAPH_INPUT]).is_ok());
    }

    #[test]
    fn fewest_keys_picks_the_shortest_complete_path() {
        let graph = SegmentGraph::build(b"nihao").expect("valid");
        let path = graph.fewest_keys(false);
        assert_eq!(
            path.iter()
                .map(|edge| edge.key().text())
                .collect::<Vec<_>>(),
            ["ni", "hao"]
        );

        let with_incomplete = SegmentGraph::build(b"n").expect("valid");
        assert!(with_incomplete.fewest_keys(false).is_empty());
        assert_eq!(
            with_incomplete
                .fewest_keys(true)
                .iter()
                .map(|edge| edge.key().text())
                .collect::<Vec<_>>(),
            ["n"]
        );

        let tail = SegmentGraph::build(b"nih").expect("valid");
        assert_eq!(
            tail.fewest_keys(false)
                .iter()
                .map(|edge| edge.key().text())
                .collect::<Vec<_>>(),
            ["ni"]
        );
        assert_eq!(
            tail.fewest_keys(true)
                .iter()
                .map(|edge| edge.key().text())
                .collect::<Vec<_>>(),
            ["ni", "h"]
        );

        let graph = SegmentGraph::build(b"xian").expect("valid");
        assert_eq!(
            graph
                .fewest_keys(false)
                .iter()
                .map(|edge| edge.key().text())
                .collect::<Vec<_>>(),
            ["xian"]
        );
    }

    #[test]
    fn fewest_keys_parse_canonicalizes_unseparated_and_trailing_bytes() {
        let parsed = FewestKeys::parse("nihaoXYZ").expect("parses");
        assert_eq!(
            parsed
                .keys()
                .iter()
                .map(|key| key.text())
                .collect::<Vec<_>>(),
            ["ni", "hao"]
        );
        assert_eq!(parsed.canonical(), "ni'hao");
        assert_eq!(
            FewestKeys::parse("ni'hao").map(|parsed| parsed.canonical()),
            Some("ni'hao".to_owned())
        );
        assert_eq!(FewestKeys::parse("n"), None);
        assert_eq!(
            FewestKeys::parse("nih").map(|parsed| parsed.canonical()),
            Some("ni".to_owned())
        );
        assert_eq!(
            FewestKeys::parse("").map(|parsed| parsed.keys().len()),
            Some(0)
        );
    }

    #[test]
    fn malformed_utf8_and_control_bytes_build_a_graph() {
        for input in [
            &[0xff, b'n', b'i'][..],
            &[b'n', b'i', 0x00, b'h'][..],
            &[0x80, 0x80][..],
            b"NIHAO",
            b"ni2hao3",
        ] {
            let graph = SegmentGraph::build(input).expect("every byte string builds");
            assert!(graph.consumed() <= graph.input_len());
        }
    }
}
