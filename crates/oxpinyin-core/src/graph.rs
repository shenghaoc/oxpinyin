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

use crate::{
    Completeness, FULL_PINYIN_SYLLABLES, INCOMPLETE_PINYIN_KEYS, MAX_SYLLABLE_LEN, SyllableKey,
};

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
        if input.len() > MAX_GRAPH_INPUT {
            return Err(GraphError::InputTooLong {
                len: input.len(),
                limit: MAX_GRAPH_INPUT,
            });
        }

        let node_count = input.len() + 1;
        let mut edges: Vec<Edge> = Vec::new();
        let mut starts = Vec::with_capacity(node_count + 1);

        for node in 0..node_count {
            starts.push(u32::try_from(edges.len()).unwrap_or(u32::MAX));
            emit_edges(input, node, &mut edges);
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

    /// The furthest node reachable from the start.
    ///
    /// This is the graph's answer to `pinyin_get_parsed_input_length`.
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// Whether every byte of the input is covered by some path.
    #[must_use]
    pub const fn fully_consumed(&self) -> bool {
        self.consumed == self.input_len
    }

    /// Fewest-keys path to [`SegmentGraph::consumed`].
    ///
    /// Longest parsed length, then fewest keys, first-found ties in
    /// left-to-right shortest-key-first order — the selection
    /// `candidate-construction.md` §8.1 freezes. Incomplete edges are
    /// included only when `allow_incomplete`. Empty when no such path
    /// exists, including the empty input.
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

        let mut path = Vec::new();
        let mut node = bound;
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
/// At a fixed position a given length matches at most one key, because the
/// complete and initial-only inventories are disjoint sets of exact spellings.
/// So one edge per length, and the emission order is total.
fn emit_edges(input: &[u8], node: usize, edges: &mut Vec<Edge>) {
    let Some(syllable_start) = key_start(input, node) else {
        return;
    };

    let available = input.len() - syllable_start;
    let mut longest_complete = None;

    // Each `lookup` below is a linear scan of the 405-entry inventory, so
    // building the graph is O(n × MAX_SYLLABLE_LEN × 405) — acceptable for W4;
    // trie lookup is Stage 2 if this becomes hot. It has not been: the whole
    // 10,459-input corpus differential builds a graph per input in about ten
    // seconds, and a session's inputs are a few dozen bytes.
    //
    // Two passes so `Exact` can mean "the longest complete syllable here"
    // without the caller having to look at its neighbours.
    for length in (1..=MAX_SYLLABLE_LEN.min(available)).rev() {
        let text = &input[syllable_start..syllable_start + length];
        if !text.iter().all(u8::is_ascii_lowercase) {
            continue;
        }
        if longest_complete.is_none() && lookup(text, &FULL_PINYIN_SYLLABLES).is_some() {
            longest_complete = Some(length);
            break;
        }
    }

    for length in (1..=MAX_SYLLABLE_LEN.min(available)).rev() {
        let end = syllable_start + length;
        let text = &input[syllable_start..end];
        if !text.iter().all(u8::is_ascii_lowercase) {
            continue;
        }

        let kind = if lookup(text, &FULL_PINYIN_SYLLABLES).is_some() {
            if longest_complete == Some(length) {
                EdgeKind::Exact
            } else {
                EdgeKind::Segmentation
            }
        } else if lookup(text, &INCOMPLETE_PINYIN_KEYS).is_some() {
            EdgeKind::Incomplete
        } else {
            continue;
        };

        let Some(key) = ascii_key(text) else {
            continue;
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
        });
    }
}

/// Where a key may begin for an edge leaving `node`, or `None` if none can.
///
/// An apostrophe at `node` is a separator and the key begins after it. A
/// *leading* apostrophe is not a separator: `parser-path-set.md` freezes it as
/// remainder-starting, and `parser-spec-contradiction-incomplete-keys.md`
/// leaves the apostrophe-tolerance question open as maintainer decision 3.
/// The graph does not quietly settle it.
fn key_start(input: &[u8], node: usize) -> Option<usize> {
    let byte = *input.get(node)?;
    if byte == b'\'' {
        if node == 0 {
            return None;
        }
        let next = *input.get(node + 1)?;
        if next == b'\'' {
            return None;
        }
        return Some(node + 1);
    }
    Some(node)
}

fn lookup(text: &[u8], table: &[&'static str]) -> Option<&'static str> {
    table.iter().copied().find(|entry| entry.as_bytes() == text)
}

fn ascii_key(text: &[u8]) -> Option<SyllableKey> {
    core::str::from_utf8(text)
        .ok()
        .and_then(SyllableKey::from_text)
}

#[cfg(test)]
mod tests {
    use super::{EdgeKind, FewestKeys, GraphError, MAX_GRAPH_INPUT, SegmentGraph};
    use crate::SyllableKey;

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
    fn the_open_apostrophe_cases_are_left_open() {
        // Maintainer decision 3 in parser-spec-contradiction-incomplete-keys.md
        // keeps these two divergent from the pin. The graph must not settle
        // them by accident.
        let leading = SegmentGraph::build(b"'ni").expect("valid");
        assert_eq!(
            leading.consumed(),
            0,
            "a leading apostrophe is not a separator"
        );

        let doubled = SegmentGraph::build(b"ni''hao").expect("valid");
        assert_eq!(
            doubled.consumed(),
            2,
            "a doubled apostrophe is not a separator"
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
