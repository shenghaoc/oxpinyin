# SegmentGraph SPEC

Date: 2026-08-09 · Status: **frozen for W4-T1**

`docs/findings/parser-spec-contradiction-incomplete-keys.md` ends with a
decision rather than a fix: mid-position incomplete keys are **not** to be
enumerated by the foundation parser, because explicit enumeration is
exponential, and are to be represented as edges on W4's graph instead. This
finding is that graph.

## The problem it solves, stated once

Under the parity profile `0x18a` the pin treats an initial-only key as a
first-class key usable at any cursor, any number of times, interleaved with
complete keys. It selected `ying, ch, o, n` for `yingchon` and chained eight
`z` keys across `zzzzzzzz`. 483 of the 491 measured divergences are that one
behaviour.

An explicit `Vec<Parse>` cannot hold it. A graph over byte positions can:
edges are `O(n × max_syllable_len)` and nobody enumerates paths at all — the
decoder's `k` bounds the output at the layer where "too many" is meaningful.

## Shape

Nodes are byte positions `0..=input.len()`. Every edge runs strictly forward,
so the graph is acyclic by construction with no cycle check. Arenas are
index-based, never references: `edges` is a flat `Vec`, and `starts[node]`
delimits that node's outgoing range.

```rust
pub struct Edge {
    from: u32,            // node the edge leaves
    to: u32,              // node it enters
    syllable_start: u32,  // where the key's own text begins
    key: SyllableKey,
    kind: EdgeKind,
}
```

`from` may be one byte before `syllable_start`. A consumed apostrophe
separator rides on the edge that follows it, so the graph stays a plain walk
over byte positions while the segment a path reports still matches the capture
notation: `chang'an` yields an edge `5 → 8` whose reported segment is
`an@6:8`, exactly as `fixtures/foundation/f-a.txt` records it.

## Edge kinds

| Kind | Meaning |
|---|---|
| `Exact` | the longest complete syllable matching at this position |
| `Segmentation` | a shorter complete syllable where a longer one also matches |
| `Incomplete` | an initial-only key |

`Fuzzy`, `Typo` and `Abbrev` are Stage 2 and are deliberately **not declared**.
A variant nothing produces is a promise the decoder would have to keep;
`EdgeKind` is `#[non_exhaustive]`, so adding them later is not a break.

Separating `Exact` from `Segmentation` is what lets a scorer charge for
choosing a split without the scorer having to re-derive which split was
greedy. The capture shows why that matters: for `fangan` the pin's own
candidate list opens `方案` (`fang` + `an`) and `反感` (`fan` + `gan`) — two
different segmentations ranked against each other, which is only expressible
if both are edges.

## The incomplete-key inventory

An initial-only key is a non-empty proper prefix of one of the 405 complete
syllables that contains no vowel byte (`a`, `e`, `i`, `o`, `u`, `v` — `v`
spells `ü`, so `lv` is not an initial) and is not itself complete. Applied to
the frozen inventory that rule yields exactly **23** keys:

```text
b c ch d f g h j k l m n p q r s sh t w x y z zh
```

`parser-spec.md` states there are 23 without listing them; the rule above
derives the list mechanically, and
`crates/pinyin-core/src/syllables.rs` asserts it against the complete
inventory rather than trusting the transcription.

**Measured against the pin.** Every `partial` segment the oracle emitted
across the 10,465-input corpus is one of these keys. Nineteen appear:

| Key | Count | Key | Count | Key | Count | Key | Count |
|---|---:|---|---:|---|---:|---|---:|
| `n` | 191 | `r` | 49 | `t` | 42 | `d` | 42 |
| `h` | 37 | `c` | 37 | `q` | 32 | `z` | 31 |
| `ch` | 29 | `s` | 28 | `k` | 27 | `g` | 26 |
| `zh` | 25 | `p` | 20 | `w` | 18 | `l` | 18 |
| `b` | 15 | `sh` | 12 | `f` | 8 | | |

`j`, `m`, `x` and `y` are members by the same rule and do not happen to occur
in this corpus. Stating both halves matters: the inventory is derived, and its
coverage is measured, and those are different claims.

## Emission order

For each node ascending, emit by key length descending. At a fixed position a
given length matches at most one key — the complete and initial-only
inventories are disjoint sets of exact spellings — so there is at most one
edge per `(node, length)` and the order is total. Edge ids follow that order,
which is what makes the k-best tie-break in `kbest-search.md` well defined.

## Reachability and consumed length

`consumed()` is the furthest node reachable from node 0. It is the graph's
answer to `pinyin_get_parsed_input_length`, and it agrees with the pin on
10,457 of the 10,459 corpus inputs the fixture carries.

An input builds a graph whatever its bytes are. Junk, malformed UTF-8, an
empty input and a 4,096-byte run of `!` all produce a graph, possibly with no
edges. The only refusal is `GraphError::InputTooLong` beyond
`MAX_GRAPH_INPUT` (65,535 bytes), which is where the oracle's `guint16`
positions stop being able to describe an answer at all.

## Apostrophes, and the two cases left open

An apostrophe at a node is a separator: a key may begin at the following byte
and the edge covers both. Two cases are deliberately excluded:

- a **leading** apostrophe (`'ni`), and
- a **doubled** apostrophe (`ni''hao`).

The pin consumes both; `parser-path-set.md` freezes both as
remainder-starting; and maintainer decision 3 in
`parser-spec-contradiction-incomplete-keys.md` leaves the disagreement open.
The graph follows the frozen SPEC. These are the only two corpus inputs where
the graph and the pin disagree, they are named in
`crates/pinyin-oracle/tests/graph_paths.rs`, and that list is where the
decision will show up when it is made.

Settling an open question as a side effect of building something else would
have hidden it.

## Relationship to the foundation parser

The graph is a **superset** of the frozen parser path set, not a replacement
for it. `FullPinyinParser` keeps its `Vec<Parse>` contract — complete-syllable
segmentations with an optional trailing partial — because that is a useful and
independently tested enumeration, and because `MAX_PARSE_RESULTS` remains
meaningful there. The graph is where mid-position and repeated partials live.

## The recovered-path fixture

`fixtures/w4/oracle-paths.txt` carries the pin's selected segmentation for all
10,459 non-sentinel corpus inputs, so W4 can check itself against the pin in
portable CI with no oracle build.

It was recovered from a W2-T3 live run, in two halves:

- **485 inputs** — the `path-set` divergences — have their path spelled out
  verbatim as `theirs_path` in `divergences.tsv`.
- **9,974 inputs** agreed, and `comparisons.tsv` records the `rank` of the
  oracle's path inside our ordered path set. Our parser is frozen and
  deterministic, so the path at that rank *is* the oracle's path.
- **6 inputs** are `oracle-sentinel` (the F-E-14 apostrophe abort and the
  F-E-01 missing-string shape). They have no usable path and are excluded,
  which is why the fixture holds 10,459 and not 10,465.

Regenerate with:

```bash
bash tools/oracle/build-oracle.sh --prefix "$HOME/.local/opt/pinyin-oracle"
cargo run -p pinyin-oracle --features oracle-ffi --bin parity-diff -- target/parity
cargo run -p pinyin-oracle --bin oracle-paths -- target/parity fixtures/w4/oracle-paths.txt
```

Format: a leading `#` header block, then one record per line with three
TAB-separated fields — escaped input, consumed length, and the path in
`pinyin-capture-v1` segment notation (`-` when there is no segment). A reader
identifies a record by its TABs rather than by the absence of a leading `#`,
because the junk stratum contains inputs like `#nishaozhan` and escaping never
emits a raw TAB.

## Acceptance

Measured by `crates/pinyin-oracle/tests/graph_paths.rs`, in portable CI:

| Check | Result |
|---|---|
| Captured oracle paths that walk the graph | 10,457 of 10,459 |
| Failures | the two open apostrophe cases, named |
| Paths with a non-final initial-only key, all walking | **483** |
| Consumed length agreeing with the pin | 10,457 of 10,459 |

The 483 is the population `parser-spec-contradiction-incomplete-keys.md`
measured. It is the number this graph was built to move, and it moved to zero
outstanding.

Plus synthetic goldens in `crates/pinyin-core/src/graph.rs` covering the empty
input, ordering, tie cases, apostrophes, junk, chained initials and
over-length refusal.
