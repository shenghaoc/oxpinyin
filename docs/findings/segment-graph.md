# SegmentGraph SPEC

Date: 2026-08-09 · Status: **frozen for W4-T1**

What binds: the graph behaviour that this SPEC derives from the pin — which keys it admits at each byte position, the 23-key incomplete inventory, the emission order, the consumed length and the input it refuses — as far as it is observable through the C APIs ([frozen-SPEC ruling](compatibility-policy.md#frozen-specs)).
A change to that behaviour needs a maintainer ruling.
The graph itself is an internal structure; it binds nothing beyond that behaviour.
The Rust struct layout, the edge-kind names, the test names and the measurement tables are current reference for oxpinyin's implementation and its recorded evidence.

The 2026-08-09 partial-placement correction in the [parser-spec.md correction log](parser-spec.md#architect-correction-log) ends with a decision, not a fix.
The foundation parser does **not** enumerate mid-position incomplete keys, because explicit enumeration is exponential.
A graph represents them as edges instead.
This SPEC defines that graph.

## The problem it solves

Under the parity profile `0x18a`, the pin treats an initial-only key as a first-class key.
It is usable at any cursor, any number of times, interleaved with complete keys.
The pin selected `ying, ch, o, n` for `yingchon` and chained eight `z` keys across `zzzzzzzz`.
Recorded evidence: 483 of the 491 measured divergences are that one behaviour.

An explicit `Vec<Parse>` cannot hold it.
A graph over byte positions can: edges are `O(n × max_syllable_len)`, and no component enumerates paths.
The decoder's `k` bounds the output at the layer where "too many" has a meaning.

## Shape

Nodes are byte positions `0..=input.len()`.
Every edge runs strictly forward, so the graph is acyclic by construction and needs no cycle check.
Arenas are index-based, never references: `edges` is a flat `Vec`, and `starts[node]` delimits the outgoing range of that node.

```rust
pub struct Edge {
    from: u32,            // node the edge leaves
    to: u32,              // node it enters
    syllable_start: u32,  // where the key's own text begins
    key: SyllableKey,
    kind: EdgeKind,
}
```

`from` may be one byte before `syllable_start`.
A consumed apostrophe separator rides on the edge that follows it.
So the graph stays a plain walk over byte positions, and the segment that a path reports still matches the capture notation.
`chang'an` gives an edge `5 → 8` whose reported segment is `an@6:8`, exactly as `fixtures/foundation/f-a.txt` records it.

## Edge kinds

| Kind | Meaning |
|---|---|
| `Exact` | the longest complete syllable matching at this position |
| `Segmentation` | a shorter complete syllable where a longer one also matches |
| `Incomplete` | an initial-only key |

`Fuzzy`, `Typo` and `Abbrev` are Stage 3, and the enum does **not declare** them.
A variant that nothing produces is a promise that the decoder must keep.
`EdgeKind` is `#[non_exhaustive]`, so a later addition is not a break.

The split between `Exact` and `Segmentation` lets a scorer charge for a split choice without re-deriving which split was greedy.
The capture shows why that is necessary.
For `fangan`, the pin's candidate list opens with `方案` (`fang` + `an`) and `反感` (`fan` + `gan`).
These are two different segmentations ranked against each other, which the graph can express only if both are edges.

## The incomplete-key inventory

An initial-only key is a non-empty proper prefix of one of the 405 complete syllables that:

- contains no vowel byte (`a`, `e`, `i`, `o`, `u`, `v` — `v` spells `ü`, so `lv` is not an initial), and
- is not itself complete.

Applied to the frozen inventory, that rule gives exactly **23** keys:

```text
b c ch d f g h j k l m n p q r s sh t w x y z zh
```

[parser-spec.md](parser-spec.md) states that there are 23 but does not list them.
The rule above derives the list mechanically.
`crates/oxpinyin-core/src/syllables.rs` asserts it against the complete inventory, so the transcription is checked, not trusted.

**Measured against the pin.** Every `partial` segment in `fixtures/w4/oracle-paths.txt` is one of these keys.
The fixture holds 1,283 `partial` segments, and all 23 keys occur.
The table counts the 687 `partial` segments in the 483 records that have a non-final `partial` segment.
Nineteen keys appear there:

| Key | Count | Key | Count | Key | Count | Key | Count |
|---|---:|---|---:|---|---:|---|---:|
| `n` | 191 | `r` | 49 | `t` | 42 | `d` | 42 |
| `h` | 37 | `c` | 37 | `q` | 32 | `z` | 31 |
| `ch` | 29 | `s` | 28 | `k` | 27 | `g` | 26 |
| `zh` | 25 | `p` | 20 | `w` | 18 | `l` | 18 |
| `b` | 15 | `sh` | 12 | `f` | 8 | | |

`j`, `m`, `x` and `y` do not occur in those 483 records.
In the fixture they occur only as the final segment of a path (`j` 16, `m` 23, `x` 22, `y` 20).
The two claims are different: the inventory is derived, and its coverage is measured.

## Emission order

For each node in ascending order, emit edges by key length, descending.
At a fixed position, a given length matches at most one key, because the complete and initial-only inventories are disjoint sets of exact spellings.
So there is at most one edge per `(node, length)`, and the order is total.
Edge ids follow that order, which makes the first-found tie-break of `SegmentGraph::fewest_keys` well defined.

## Reachability and consumed length

`consumed()` is the furthest node reachable from node 0.
It is the graph's answer to `pinyin_get_parsed_input_length`.
It agrees with the pin on all 1,788 records that the fixture carries.

An input builds a graph whatever its bytes are.
Junk, malformed UTF-8, an empty input and a 4,096-byte run of `!` all produce a graph, possibly with no edges.
The only refusal is `GraphError::InputTooLong` beyond `MAX_GRAPH_POSITION` (`u32::MAX - 1` bytes), where the `u32` positions of an edge end.
No pin rule or product rule sets a lower limit ([1501-gint16-input-cap](1501-gint16-input-cap.md)).

## Apostrophes

An apostrophe at a node is a separator: a key may begin at the following byte, and the edge covers both.
A run of consecutive apostrophes is one separator, and a key may begin at the byte after the run.
This rule applies at every reachable node, including node 0.
So the graph consumes a **leading** apostrophe (`'ni`) and a **doubled** apostrophe (`ni''hao`), as the pin does.
An apostrophe run with no key after it has no edge.

[parser-path-set.md](parser-path-set.md) freezes both cases as consumed (correction log, 2026-08-21).
Both halves of maintainer decision 3 are closed.
`OPEN_APOSTROPHE_CASES` in `crates/oxpinyin-core/tests/graph_paths.rs` is empty, so the test permits no disagreement with the pin.

## Relationship to the foundation parser

The graph is a **superset** of the frozen parser path set, not a replacement for it.
`FullPinyinParser` keeps its `Vec<Parse>` contract: complete-syllable segmentations with an optional trailing partial.
That enumeration is useful and independently tested, and `MAX_PARSE_RESULTS` still has a meaning there.
Mid-position and repeated partials are in the graph.

## The recovered-path fixture

`fixtures/w4/oracle-paths.txt` carries the pin's selected segmentation for 1,788 corpus records.
It holds every record that has a `partial` segment or a consumed length shorter than the input (1,744).
It also holds one witness for each remaining segmentation shape (44).
With it, the graph is checked against the pin in portable CI, with no oracle build.

Recorded evidence: the recovery took 10,459 records from a W2-T3 live run, in two halves, with one excluded group:

- **485 inputs** are the `path-set` divergences.
  `divergences.tsv` spells their path out verbatim as `theirs_path`.
- **9,974 inputs** agreed, and `comparisons.tsv` records the `rank` of the oracle's path in oxpinyin's ordered path set.
  The parser is frozen and deterministic, so the path at that rank *is* the oracle's path.
- **6 inputs** are `oracle-sentinel` (the F-E-14 apostrophe abort and the F-E-01 missing-string shape).
  They have no usable path and are excluded, so the recovery holds 10,459 inputs, not 10,465.

The fixture keeps 1,788 of those 10,459 records.

The committed fixture is the portable regression input.
It has no regenerator: the W2 logs that it came from no longer exist, so it stays frozen ([goldens and pins](../runbooks/goldens-and-pins.md)).

Format:

- a leading `#` header block;
- then one record per line, with three TAB-separated fields: escaped input, consumed length, and the path in `pinyin-capture-v1` segment notation (`-` when there is no segment).

A reader identifies a record by its TABs, not by the absence of a leading `#`.
The junk stratum contains inputs such as `#nishaozhan`, and escaping never emits a raw TAB.

## Acceptance

`crates/oxpinyin-core/tests/graph_paths.rs` asserts these results in portable CI.
The 483 is a count of the fixture records; the test does not assert it.

| Check | Result |
|---|---|
| Captured oracle paths that walk the graph | 1,788 of 1,788 |
| Failures | none (`OPEN_APOSTROPHE_CASES` is empty) |
| Paths with a non-final initial-only key, all walking | **483** |
| Consumed length agreeing with the pin | 1,788 of 1,788 |

The 483 is the population that the 2026-08-09 partial-placement correction in [parser-spec.md](parser-spec.md#architect-correction-log) records.
The graph was built to close it, and none of it is outstanding.

Synthetic goldens in `crates/oxpinyin-core/src/graph.rs` also cover the empty input, ordering, tie cases, apostrophes, junk, chained initials and over-length refusal.
