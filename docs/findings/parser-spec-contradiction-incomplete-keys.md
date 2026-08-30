# Findings — frozen parser SPEC contradicts the pin on incomplete keys

Date: 2026-08-09 · Source tier: Architect observation from the W2-T3 corpus run.
Status: **resolved-by-architecture** (maintainer decisions 2026-08-09).

Two layers of resolution:

1. **Oracle-driven SPEC correction** on the field invariant in
   `docs/findings/parser-spec.md`: partial segments may appear at any
   position, multiple times (evidence below). That freeze edit stands.
2. **Architecture resolution** of the path-set / `MAX_PARSE_RESULTS`
   follow-up: do **not** extend the foundation parser's explicit
   `Vec<Parse>` enumeration to mid-position partials. That work belongs
   to W4's `SegmentGraph` + decoder. See §Resolved by architecture.

This finding remains as the measurement that justified both decisions.

## The contradiction

`docs/findings/parser-spec.md` line 130 freezes this field invariant:

> A result contains at most one partial segment, and it is the last segment.

and §"Boundaries, partials, and junk" freezes:

> Complete segmentations take precedence. A terminal lowercase group emits
> partial-tail paths only when no complete segmentation consumes that whole
> group.

The pinned oracle does not behave that way. With `PINYIN_INCOMPLETE` set — which
is on in the F-A profile `0x18a` that Stage 1 parity runs under — it treats an
initial-only key as a **first-class key usable at any position, any number of
times**, freely interleaved with complete keys.

## Evidence

From the W2-T3 live run over the full 10,465-input parity corpus against the
pin-built oracle. Notation is the frozen `capture-fixtures.md` segment form.

| Input | Oracle selected path | Our frozen path set |
|---|---|---|
| `yingchon` | `ying@0:4:C, ch@4:6:P, o@6:7:C, n@7:8:P` | `ying@0:4:C, chon@4:8:P` · `yi@0:2:C, ng@2:4:C, chon@4:8:P` |
| `fanrufe` | `fan@0:3:C, ru@3:5:C, f@5:6:P, e@6:7:C` | `fan@0:3:C, ru@3:5:C, fe@5:7:P` |
| `ngannve` | `n@0:1:P, gan@1:4:C, nve@4:7:C` | `ng@0:2:C, an@2:4:C, nve@4:7:C` · … |
| `zzzzzzzz` | eight `z@n:n+1:P` keys, consuming all 8 bytes | `z@0:1:P`, consuming 1 byte |
| `qqqq…` (32) | thirty-two `q@n:n+1:P` keys, consuming all 32 | `q@0:1:P`, consuming 1 byte |

`ch`, `f`, `n`, `z` and `q` are initial-only strings. Under the frozen SPEC they
are partials, so each of these oracle paths violates either the "at most one"
clause, the "it is the last segment" clause, or both.

## Measured scale

Live run, pin
`libpinyin-2.11.91-0c5e80e1200f84fab185d1c5bde458b770a0636c+model20-59c68e89d43ff85f5a309489499cbcde282d2b04bd91888734884b7defcb1155+dbm-tkrzw`,
flags `0x18a`:

```text
compared      10465
agreements     9974   95.31%
divergences     491    4.69%
  path-absent         481
  consumed-length       4
  oracle-sentinel       6
```

Every one of the 481 `path-absent` records has this single root cause:

| Oracle path shape | Count |
|---|---:|
| one partial, not in final position | 315 |
| several partials, at least one not final | 166 |

Two of the four `consumed-length` records are the same cause seen through
length rather than shape: `zzzzzzzz` and `qqqq…` (32), where the oracle chains
initial-only keys across the whole input and consumes it entirely, while our
parser consumes one byte.

So **483 of 491 divergences (98.4%) are this one contradiction.** It is
systematic, not incidental.

## Why Phase 0 did not catch it

F-A froze fifteen cases. Its only incomplete-input cases are `incomplete-nih`
(`ni` + `h`) and `incomplete-zhongg` (`zhong` + `g`). Both are a *single*
partial in *final* position.

The frozen invariant is therefore a true description of every observation Phase
0 had, and a false generalisation beyond them. No F-A case could have
distinguished "at most one trailing partial" from "any number of partials
anywhere". The parity corpus's `06-partial-tails.txt` stratum, which places
partial prefixes after zero to three complete syllables, is what separated them:
463 of the 481 `path-absent` records come from that stratum.

This is the W2 workstream doing what it exists for. Parity was a claim; it is
now a measurement.

## The remaining divergences

Recorded here so the 491 are fully accounted for, but they are separate issues
and are **not** part of this contradiction:

| Reason | Input | Note |
|---|---|---|
| `consumed-length` | `'ni` | oracle consumes 3, skipping a leading apostrophe. Closed 2026-08-21: `parser-path-set.md` now enumerates `[ni@1:3:C]` with an empty remainder, matching the pin. |
| `consumed-length` | `ni''hao` | oracle consumes all 7 across a doubled apostrophe. Closed 2026-08-21: `parser-path-set.md` now enumerates both `[ni, hao]` and `[ni, ha, o]` with an empty remainder. |
| `oracle-sentinel` | `'`, `''`, `'''` | abort guard, see `docs/testing/oracle-apostrophe-abort.md`. |
| `oracle-sentinel` | `ni'`, `ni'!`, `ni'i` | live F-E-01 shape: a key column with no usable pinyin string. |

The apostrophe-tolerance pair is a second, much smaller SPEC-versus-pin
question and deserves its own decision; it is not folded into this one.

## Ranking, separately

Not a divergence, but measured on the same run and relevant to the W2-T5 budget:

```text
oracle path rank within our ordered path set
  rank 0    9506
  rank 1     249
  rank 2      92
  rank >2    127   (tail out to rank 128)
```

468 inputs (4.47% of the corpus) agree but at a positive rank — the oracle picks
a path we enumerate, just not the one our greedy order puts first. That is the
`tie-swap` population at parse level, and it is nine times the 0.5% auto-accept
budget the W2-T5 card sets. See `docs/findings/divergence-taxonomy.md` for why
that budget cannot be assessed until a decoder exists.

## What is not being done

- The foundation parser is **not** extended to mid-position partials. Its
  `Vec<Parse>` contract stays complete-syllable segmentations with optional
  trailing partial — the scope it can represent without exponential
  materialisation.
- Goldens and `parser-path-set.md` are **not** rewritten to claim mid-path
  incomplete enumeration.
- The comparison is **not** weakened to absorb these. All 491 appear in the
  divergence log; W2-T5 classifies the 483 as `path-set`. That count is the
  documented W2 baseline until W4.
- The corpus is **not** trimmed to avoid the stratum that found this.

## Resolved by architecture

Explicit path enumeration is exponential under mid-position partials. No
`MAX_PARSE_RESULTS` value and no retention rule repairs that: the foundation
parser's `Vec<Parse>` contract is the wrong data structure for the problem.

Upstream does not materialise a Cartesian path set either. It builds a key
matrix (edges at each byte position); the decoder then selects via dynamic
programming. The oracle's selected segmentation is a **decoder choice from a
graph**, not one member of an exhaustively listed set.

**W4's `SegmentGraph` is the answer.** Edges are
`O(n × max_syllable_length × edge_types)` — polynomial. The decoder's `k`
bounds output. Nobody enumerates all paths. Mid-position incomplete keys
become valid graph edges; the 483 cases become selectable graph paths without
explosion.

Until then:

- The foundation parser retains `Vec<Parse>` for **complete-syllable
  segmentations with optional trailing partial** (its current, correct
  scope).
- `MAX_PARSE_RESULTS` stays at **4,096** for that foundation contract.
- The decoder's `k` parameter bounds candidate output at the layer where
  "too many" is meaningful.
- The **483 `path-set` divergences are expected** until W4 lands. They are
  documented in the W2 parity / divergence log and are not a foundation
  parser defect relative to the retained scope.

Next work after W2 merges: **W3** (data loading) and **W4** (`SegmentGraph`
+ decoder) in parallel.

## Maintainer decisions (2026-08-09)

1. **Field invariant — oracle-driven SPEC correction.** `parser-spec.md`
   records that partials may appear at any position, multiple times, matching
   the pin. That freeze edit stands as the description of pin policy.
2. **Path-set / `MAX_PARSE_RESULTS` follow-up — resolved by architecture.**
   Do not extend foundation `Vec<Parse>` enumeration to mid-position
   partials. Represent mid-position incomplete edges on W4's `SegmentGraph`;
   keep `MAX_PARSE_RESULTS = 4_096` for the foundation parser; treat the 483
   as the W2 baseline until W4.
3. **Apostrophe-tolerance pair** (`'ni`, `ni''hao`) — both halves now
   closed. The doubled half (`ni''hao`) closed on 2026-08-21 by
   aligning with the pin: a run of consecutive apostrophes after a
   fully consumed left group is a single separator. The leading half
   (`'ni`) closed on 2026-08-21 by the same law: upstream's DP
   propagates across every apostrophe at a reachable position,
   including position 0, so `'ni` consumes all three bytes. Details
   in `docs/findings/parser-spec.md` architect correction log
   (2026-08-21) and `docs/testing/corpus-tail.md`.
4. **F-A / fixture coverage** for non-terminal and repeated partials —
   still open for W4 capture against the graph, not for a foundation
   path-set rewrite.
