//! K-best paths over a [`SegmentGraph`].
//!
//! `docs/findings/kbest-search.md` freezes the algorithm, the tie order and
//! the bound. The graph's nodes are byte positions and every edge runs
//! forward, so ascending node order *is* topological order and no sort is
//! needed before the sweep.
//!
//! Costs are first-order: the cost of an edge may depend on the edge taken
//! before it. That is what a bigram needs, and it is why an entry carries its
//! last edge rather than only its node.

use core::fmt;

use crate::Cost;
use crate::graph::{Edge, EdgeId, SegmentGraph};

/// Largest `k` accepted.
///
/// The sweep keeps up to `k` entries per state, so an unbounded `k` is an
/// unbounded allocation. The limit mirrors [`crate::MAX_PARSE_RESULTS`], which
/// bounds the same kind of thing one layer up.
pub const MAX_K: usize = 4_096;

/// Why a search could not run.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum DecodeError {
    /// `k` is beyond [`MAX_K`].
    KTooLarge {
        /// The `k` that was asked for.
        k: usize,
        /// Largest `k` accepted.
        limit: usize,
    },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::KTooLarge { k, limit } => write!(formatter, "k of {k} exceeds the limit {limit}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Supplies the cost of taking one edge after another.
///
/// `previous` is `None` at the start of a path. Implementations must be
/// deterministic and must not panic; a cost they cannot compute is a large
/// finite cost, not a panic and not an infinity that poisons the arithmetic.
pub trait EdgeCost {
    /// Cost of taking `edge` when `previous` was the edge before it.
    fn cost(&self, previous: Option<&Edge>, edge: &Edge) -> Cost;
}

impl<F> EdgeCost for F
where
    F: Fn(Option<&Edge>, &Edge) -> Cost,
{
    fn cost(&self, previous: Option<&Edge>, edge: &Edge) -> Cost {
        self(previous, edge)
    }
}

/// One path through the graph, cheapest first in a k-best result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedPath {
    cost: Cost,
    edges: Vec<EdgeId>,
}

impl DecodedPath {
    /// Total cost of the path.
    #[must_use]
    pub const fn cost(&self) -> Cost {
        self.cost
    }

    /// The edges taken, in input order.
    #[must_use]
    pub fn edges(&self) -> &[EdgeId] {
        &self.edges
    }

    /// Number of edges.
    #[must_use]
    pub fn len(&self) -> usize {
        self.edges.len()
    }

    /// Whether the path takes no edge at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }
}

/// One partial path: a cost, the edge that ended it, and where it came from.
///
/// Index-based, like the graph: entries live in one arena and point at each
/// other by index.
#[derive(Clone, Copy, Debug)]
struct Entry {
    cost: Cost,
    last: Option<EdgeId>,
    previous: Option<u32>,
}

/// The `k` best paths from the start of the input to its consumed end.
///
/// # Errors
///
/// Returns [`DecodeError::KTooLarge`] for a `k` beyond [`MAX_K`].
pub fn k_best(
    graph: &SegmentGraph,
    scorer: &impl EdgeCost,
    k: usize,
) -> Result<Vec<DecodedPath>, DecodeError> {
    k_best_to(graph, scorer, k, graph.consumed())
}

/// The `k` best paths from the start of the input to `target`.
///
/// An unreachable or out-of-range `target` has no paths, which is an empty
/// result rather than an error. `k` of zero is likewise empty.
///
/// # Errors
///
/// Returns [`DecodeError::KTooLarge`] for a `k` beyond [`MAX_K`].
pub fn k_best_to(
    graph: &SegmentGraph,
    scorer: &impl EdgeCost,
    k: usize,
    target: usize,
) -> Result<Vec<DecodedPath>, DecodeError> {
    if k > MAX_K {
        return Err(DecodeError::KTooLarge { k, limit: MAX_K });
    }
    if k == 0 || target >= graph.node_count() {
        return Ok(Vec::new());
    }

    let incoming = incoming_index(graph);
    let mut arena: Vec<Entry> = vec![Entry {
        cost: 0,
        last: None,
        previous: None,
    }];

    // One bucket per state, where a state is "arrived here via this edge".
    // Bucket 0 is the start. Ranking per *node* would be wrong: with
    // first-order costs a dearer prefix ending in a different edge can extend
    // more cheaply, so the state has to carry the last edge.
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); graph.edges().len() + 1];
    buckets[0].push(0);

    for node in 0..=target {
        // Every edge runs forward, so nothing can arrive at `node` after the
        // sweep has passed it; ranking here is final.
        for state in states_at(&incoming, node) {
            // Stable sort by cost alone is what makes the tie order the frozen
            // one: within a state, entries were generated in source-state
            // order, and source states are ordered by ascending edge id.
            buckets[state].sort_by_key(|index| arena[*index as usize].cost);
            buckets[state].truncate(k);
        }

        if node == target {
            break;
        }

        for state in states_at(&incoming, node) {
            // Cloned because the loop below pushes into other buckets, which
            // would alias a borrow of this one. ≤ k entries (max 4096), and
            // they are u32 indices, so the copy is bounded and cheap.
            let ranked = buckets[state].clone();
            for entry_index in ranked {
                let entry = arena[entry_index as usize];
                let previous = entry.last.and_then(|id| graph.edge(id)).copied();

                for edge_id in graph.outgoing_ids(node) {
                    let Some(edge) = graph.edge(edge_id) else {
                        continue;
                    };
                    if edge.to() > target {
                        continue;
                    }

                    let Ok(index) = u32::try_from(arena.len()) else {
                        continue;
                    };
                    arena.push(Entry {
                        cost: entry
                            .cost
                            .saturating_add(scorer.cost(previous.as_ref(), edge)),
                        last: Some(edge_id),
                        previous: Some(entry_index),
                    });
                    buckets[edge_id.index() + 1].push(index);
                }
            }
        }
    }

    // States at the target in ascending edge-id order, which is ascending
    // source node — so on equal cost the path with the longer last edge comes
    // first, matching the parser's frozen greedy-longest order.
    let mut finalists: Vec<u32> = Vec::new();
    for state in states_at(&incoming, target) {
        finalists.extend_from_slice(&buckets[state]);
    }
    finalists.sort_by_key(|index| arena[*index as usize].cost);
    finalists.truncate(k);

    Ok(finalists
        .iter()
        .map(|index| reconstruct(&arena, *index))
        .collect())
}

/// Edge ids entering each node, ascending, in one flat arena.
struct Incoming {
    starts: Vec<u32>,
    ids: Vec<u32>,
}

fn incoming_index(graph: &SegmentGraph) -> Incoming {
    let mut starts = vec![0_u32; graph.node_count() + 1];
    for edge in graph.edges() {
        starts[edge.to() + 1] += 1;
    }
    for node in 0..graph.node_count() {
        starts[node + 1] += starts[node];
    }

    let mut cursor = starts.clone();
    let mut ids = vec![0_u32; graph.edges().len()];
    for (index, edge) in graph.edges().iter().enumerate() {
        let slot = &mut cursor[edge.to()];
        ids[*slot as usize] = u32::try_from(index).unwrap_or(u32::MAX);
        *slot += 1;
    }

    Incoming { starts, ids }
}

/// Bucket indices for every state that can be current at `node`.
fn states_at(incoming: &Incoming, node: usize) -> Vec<usize> {
    let mut states = Vec::new();
    if node == 0 {
        states.push(0);
    }
    let (Some(start), Some(end)) = (incoming.starts.get(node), incoming.starts.get(node + 1))
    else {
        return states;
    };
    states.extend(
        incoming.ids[*start as usize..*end as usize]
            .iter()
            .map(|id| *id as usize + 1),
    );
    states
}

/// Walks backpointers into a path in input order.
fn reconstruct(arena: &[Entry], index: u32) -> DecodedPath {
    let mut edges = Vec::new();
    let mut cursor = Some(index);

    while let Some(current) = cursor {
        let entry = arena[current as usize];
        if let Some(edge) = entry.last {
            edges.push(edge);
        }
        cursor = entry.previous;
    }
    edges.reverse();

    DecodedPath {
        cost: arena[index as usize].cost,
        edges,
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::{DecodeError, DecodedPath, EdgeCost, MAX_K, k_best, k_best_to};
    use crate::graph::{Edge, EdgeId, EdgeKind, SegmentGraph};
    use crate::{Cost, SyllableKey};

    /// One cost unit per byte the edge covers: the cheapest path is the one
    /// with the fewest edges, and ties are everywhere.
    fn per_edge(_previous: Option<&Edge>, _edge: &Edge) -> Cost {
        1
    }

    /// A deterministic pseudo-random cost derived from the edge itself.
    fn scrambled(seed: u64) -> impl Fn(Option<&Edge>, &Edge) -> Cost {
        move |previous, edge| {
            let key = edge.key().index() as u64;
            let previous_key = previous.map_or(0, |edge| edge.key().index() as u64);
            let mixed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(key.wrapping_mul(1_442_695_040_888_963_407))
                .wrapping_add(previous_key.wrapping_mul(2_862_933_555_777_941_757))
                .wrapping_add(match edge.kind() {
                    EdgeKind::Exact => 1,
                    EdgeKind::Segmentation => 2,
                    _ => 3,
                });
            Cost::from(u16::try_from(mixed % 4_096).unwrap_or(u16::MAX))
        }
    }

    fn graph(input: &str) -> SegmentGraph {
        SegmentGraph::build(input.as_bytes()).expect("test inputs are short")
    }

    /// Spells a path as the keys it takes, for readable goldens.
    fn spell(graph: &SegmentGraph, path: &DecodedPath) -> String {
        path.edges()
            .iter()
            .map(|id| graph.edge(*id).expect("id from this graph").key().text())
            .collect::<Vec<_>>()
            .join("+")
    }

    /// Every path from node zero to `target`, by exhaustive search.
    fn brute_force(
        graph: &SegmentGraph,
        scorer: &impl EdgeCost,
        target: usize,
    ) -> Vec<(Cost, Vec<EdgeId>)> {
        fn walk(
            graph: &SegmentGraph,
            scorer: &impl EdgeCost,
            node: usize,
            target: usize,
            cost: Cost,
            taken: &mut Vec<EdgeId>,
            found: &mut Vec<(Cost, Vec<EdgeId>)>,
        ) {
            if node == target {
                found.push((cost, taken.clone()));
                return;
            }
            for id in graph.outgoing_ids(node) {
                let edge = graph.edge(id).expect("id from this graph");
                if edge.to() > target {
                    continue;
                }
                let previous = taken
                    .last()
                    .map(|id| *graph.edge(*id).expect("id from this graph"));
                let step = scorer.cost(previous.as_ref(), edge);
                taken.push(id);
                walk(
                    graph,
                    scorer,
                    edge.to(),
                    target,
                    cost.saturating_add(step),
                    taken,
                    found,
                );
                taken.pop();
            }
        }

        let mut found = Vec::new();
        walk(graph, scorer, 0, target, 0, &mut Vec::new(), &mut found);
        found
    }

    #[test]
    fn the_empty_graph_has_one_empty_path() {
        let graph = graph("");
        let paths = k_best(&graph, &per_edge, 4).expect("k is small");
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].cost(), 0);
        assert!(paths[0].is_empty());
        assert_eq!(paths[0].len(), 0);
    }

    #[test]
    fn fewer_edges_win_when_every_edge_costs_the_same() {
        let graph = graph("nihao");
        let paths = k_best(&graph, &per_edge, 4).expect("k is small");
        let spelled: Vec<String> = paths.iter().map(|path| spell(&graph, path)).collect();

        assert_eq!(spelled[0], "ni+hao");
        assert_eq!(paths[0].cost(), 2);
        assert!(
            paths
                .windows(2)
                .all(|pair| pair[0].cost() <= pair[1].cost()),
            "results are ordered by cost"
        );
    }

    #[test]
    fn a_longer_last_edge_wins_a_tie() {
        // Frozen tie order: equal cost, then the path whose last edge starts
        // earlier — which is the longer edge, since both end at the target.
        // nihao at cost-per-edge has ni+hao (2 edges) alone at cost 2, so use
        // a scorer where several three-edge paths tie.
        let graph = graph("nihao");
        let flat = |_: Option<&Edge>, _: &Edge| 0;
        let paths = k_best(&graph, &flat, 8).expect("k is small");

        assert!(paths.iter().all(|path| path.cost() == 0));
        assert_eq!(spell(&graph, &paths[0]), "ni+hao");
        assert_eq!(spell(&graph, &paths[1]), "ni+h+ao");
    }

    #[test]
    fn k_bounds_the_result_and_zero_asks_for_nothing() {
        let graph = graph("fangan");
        let flat = |_: Option<&Edge>, _: &Edge| 0;

        assert!(k_best(&graph, &flat, 0).expect("k is small").is_empty());
        for k in 1..6 {
            assert!(k_best(&graph, &flat, k).expect("k is small").len() <= k);
        }
        assert_eq!(
            k_best(&graph, &flat, MAX_K + 1),
            Err(DecodeError::KTooLarge {
                k: MAX_K + 1,
                limit: MAX_K
            })
        );
    }

    #[test]
    fn an_unreachable_target_has_no_paths() {
        let graph = graph("ni!hao");
        let flat = |_: Option<&Edge>, _: &Edge| 0;

        assert_eq!(graph.consumed(), 2);
        assert!(!k_best(&graph, &flat, 4).expect("k is small").is_empty());
        assert!(
            k_best_to(&graph, &flat, 4, 5)
                .expect("k is small")
                .is_empty(),
            "node 5 is past the junk"
        );
        assert!(
            k_best_to(&graph, &flat, 4, 99)
                .expect("k is small")
                .is_empty(),
            "an out-of-range target is empty, not an error"
        );
    }

    #[test]
    fn a_first_order_cost_sees_the_previous_edge() {
        // Charge nothing for hao after ni, and a lot otherwise: the bigram
        // must be able to change which path wins.
        let graph = graph("nihao");
        let ni = SyllableKey::from_text("ni").expect("frozen key");
        let hao = SyllableKey::from_text("hao").expect("frozen key");
        let bigram =
            move |previous: Option<&Edge>, edge: &Edge| match (previous.map(Edge::key), edge.key())
            {
                (Some(first), second) if first == ni && second == hao => 0,
                _ => 10,
            };

        let paths = k_best(&graph, &bigram, 4).expect("k is small");
        assert_eq!(spell(&graph, &paths[0]), "ni+hao");
        assert_eq!(paths[0].cost(), 10, "ni costs 10, hao after ni costs 0");
    }

    #[test]
    fn the_incomplete_chain_is_searchable() {
        let graph = graph("zzzzzzzz");
        let paths = k_best(&graph, &per_edge, 4).expect("k is small");
        assert_eq!(paths.len(), 1, "only one path chains eight z keys");
        assert_eq!(paths[0].len(), 8);
        assert_eq!(paths[0].cost(), 8);
    }

    #[test]
    fn reported_costs_are_the_sum_of_their_edges() {
        let graph = graph("yingchon");
        let scorer = scrambled(7);

        for path in k_best(&graph, &scorer, 8).expect("k is small") {
            let mut total: Cost = 0;
            let mut previous: Option<Edge> = None;
            for id in path.edges() {
                let edge = *graph.edge(*id).expect("id from this graph");
                total = total.saturating_add(scorer(previous.as_ref(), &edge));
                previous = Some(edge);
            }
            assert_eq!(total, path.cost());
        }
    }

    proptest! {
        #[test]
        fn k_of_one_equals_the_brute_force_best(
            input in prop::collection::vec(
                prop::sample::select(&b"nihaozg'!"[..]),
                0..8_usize,
            ),
            seed in any::<u64>(),
        ) {
            let graph = SegmentGraph::build(&input).expect("short input");
            let scorer = scrambled(seed);
            let target = graph.consumed();

            let best = k_best(&graph, &scorer, 1).expect("k is small");
            let exhaustive = brute_force(&graph, &scorer, target);

            prop_assert_eq!(best.len(), usize::from(!exhaustive.is_empty()));
            if let Some(cheapest) = exhaustive.iter().map(|(cost, _)| *cost).min() {
                prop_assert_eq!(best[0].cost(), cheapest);
            }
        }

        #[test]
        fn every_returned_path_is_a_real_path_of_its_stated_cost(
            input in prop::collection::vec(
                prop::sample::select(&b"nihaox'"[..]),
                0..8_usize,
            ),
            seed in any::<u64>(),
            k in 1..6_usize,
        ) {
            let graph = SegmentGraph::build(&input).expect("short input");
            let scorer = scrambled(seed);
            let paths = k_best(&graph, &scorer, k).expect("k is small");

            prop_assert!(paths.len() <= k);
            for path in &paths {
                let mut node = 0;
                let mut total: Cost = 0;
                let mut previous: Option<Edge> = None;
                for id in path.edges() {
                    let edge = *graph.edge(*id).expect("id from this graph");
                    prop_assert_eq!(edge.from(), node);
                    total = total.saturating_add(scorer(previous.as_ref(), &edge));
                    node = edge.to();
                    previous = Some(edge);
                }
                prop_assert_eq!(node, graph.consumed());
                prop_assert_eq!(total, path.cost());
            }
            prop_assert!(paths.windows(2).all(|pair| pair[0].cost() <= pair[1].cost()));
        }

        #[test]
        fn searching_is_deterministic(
            input in prop::collection::vec(
                prop::sample::select(&b"nihaoq!"[..]),
                0..10_usize,
            ),
            seed in any::<u64>(),
        ) {
            let graph = SegmentGraph::build(&input).expect("short input");
            let scorer = scrambled(seed);
            prop_assert_eq!(
                k_best(&graph, &scorer, 5).expect("k is small"),
                k_best(&graph, &scorer, 5).expect("k is small")
            );
        }

        #[test]
        fn the_k_best_are_the_k_cheapest(
            input in prop::collection::vec(
                prop::sample::select(&b"nihao"[..]),
                0..7_usize,
            ),
            seed in any::<u64>(),
        ) {
            let graph = SegmentGraph::build(&input).expect("short input");
            let scorer = scrambled(seed);
            let target = graph.consumed();

            let mut exhaustive: Vec<Cost> = brute_force(&graph, &scorer, target)
                .into_iter()
                .map(|(cost, _)| cost)
                .collect();
            exhaustive.sort_unstable();

            let found: Vec<Cost> = k_best(&graph, &scorer, 4)
                .expect("k is small")
                .iter()
                .map(DecodedPath::cost)
                .collect();
            prop_assert_eq!(found.as_slice(), &exhaustive[..found.len()]);
        }
    }
}
