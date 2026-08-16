# K-best search SPEC

Date: 2026-08-09 · Status: **frozen for W4-T2**

The search over `SegmentGraph`. `k` is a parameter, the tie order is total and
documented, and nothing enumerates paths.

## What "k-best" has to mean here

`docs/findings/divergence-taxonomy.md` measured 468 corpus inputs where the
pin selected a segmentation our frozen greedy order does not put first, and
recorded that number as the W4 baseline for the `tie-swap` class. Reproducing
the pin's *choice* is what a decoder is for, and a choice needs an ordered set
of alternatives to choose among — not just the best one. Hence k.

The k-best result is also what a candidate list is built from: the pin's own
candidates for `fangan` mix `fang`+`an` and `fan`+`gan`, so a decoder that
kept only one segmentation could not produce the pin's list at all.

## Cost model

Costs are **first-order**: the cost of an edge may depend on the edge taken
immediately before it.

```rust
pub trait EdgeCost {
    fn cost(&self, previous: Option<&Edge>, edge: &Edge) -> Cost;
}
```

That is exactly what an interpolated bigram needs. It is also the reason the
dynamic-programming state is not the node:

> With first-order costs, the cheapest prefix reaching a node is **not**
> necessarily part of the cheapest path through it. A dearer prefix that ends
> in a different edge can extend more cheaply.

So the state is `(node, last edge)` — equivalently "arrived here via this
edge" — and the sweep keeps up to `k` entries **per state**, not per node.
Ranking per node is a wrong answer that looks right on small examples; the
`k = 1` property test in `crates/oxpinyin-core/src/kbest.rs` is what catches it,
and it caught it.

Keeping `k` per state is exact. Any path in the true top `k` ends in some
state, and among paths ending in that state it ranks at most `k`th, so it
survives every truncation on the way.

## Sweep

Nodes are byte positions and every edge runs forward, so ascending node order
*is* topological order: no sort, no visited set, no recursion.

1. For each node ascending to the target, rank each state at that node by cost
   and truncate to `k`.
2. Extend every surviving entry along every outgoing edge, adding
   `cost(previous, edge)` with saturating arithmetic.
3. At the target, concatenate the states in ascending edge-id order, rank by
   cost, truncate to `k`, and reconstruct through backpointers.

Entries live in one flat arena and refer to each other by index, matching the
graph.

## Tie order

Total, and a function of the input alone:

1. **lower total cost**;
2. then the path whose **last edge starts earlier** — which is the longer
   edge, since both end at the same node;
3. then the better-ranked prefix within that state;
4. then ascending edge id.

Rules 2–4 fall out of one implementation choice: candidates are generated in
state order and ranked with a *stable* sort by cost. Edge ids ascend by source
node and then by descending key length, so ascending state order is exactly
"longer last edge first".

Rule 2 is not arbitrary. It matches the greedy-longest preference
`parser-path-set.md` already froze for the parser's path order, so at equal
cost the decoder and the parser agree about which alternative reads first.

## Bounds and totality

- `k` above `MAX_K` (4,096) returns `DecodeError::KTooLarge`. The sweep holds
  up to `k` entries per state, so an unbounded `k` is an unbounded allocation;
  the limit mirrors `MAX_PARSE_RESULTS`, which bounds the same kind of thing
  one layer up.
- `k = 0` is an empty result, not an error.
- An unreachable or out-of-range target is an empty result, not an error.
- A graph with no edges yields one empty path of cost zero. The empty
  segmentation is a real answer, exactly as the empty parse is in
  `parser-path-set.md`.
- Cost accumulation saturates. No addition panics, whatever a scorer returns.

## Acceptance

Goldens in `crates/oxpinyin-core/src/kbest.rs`, including tie cases, first-order
costs that change the winner, the chained-initial path for `zzzzzzzz`, and the
bound.

Four `proptest` properties over random short inputs and a scrambled scorer:

| Property | What it pins |
|---|---|
| `k = 1` equals brute force | the search is optimal, not merely plausible |
| the `k` best are the `k` cheapest | truncation loses nothing |
| every returned path is a real path of its stated cost | backpointers and arithmetic agree |
| searching is deterministic | constitution item 6 |
