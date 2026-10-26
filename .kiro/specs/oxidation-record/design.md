# Design — Oxidation record

## Overview

Two documents and one steering file carry the method:

```text
.kiro/steering/oxidation-method.md   — the rules, always loaded by agents
docs/findings/libpinyin-structures-and-algorithms.md
                                     — the record: pin vs oxpinyin, row by row
docs/oxidation-journey.md            — the narrative for human readers
```

The record is a register: it changes whenever the tree does. The
journey is dated and describes the tree it names.

## The record

Sections follow the pin's own layers, so a reader can go from a pin
file to its row:

| § | Content |
| --- | --- |
| 0 | libpinyin's component layout and oxpinyin's crate split |
| 1 | storage |
| 2 | parsing |
| 3 | the phonetic key matrix |
| 4 | sentence decoding |
| 5 | the candidate list |
| 6 | training and user phrases |
| 7 | object model and persistence |
| 8 | offline tooling |
| 9 | coefficients |
| 10 | where oxpinyin keeps the pin's code shape |
| 11 | open gaps |

A row is added or changed by reading the pin's source at `074a2219` —
its assembly or binary only where the source does not settle the
question — and oxpinyin's code. It is not derived by running inputs.
Once a row's behaviour is derived, the oracle differential for that
surface is the gate.

## Arguments for non-identical rows

Each row whose structure is "different" moves from "unverified" to
"argued" by getting a short argument in the record, beneath its table:
what the pin's structure guarantees that
reaches output (an order, a tie rule, a product quirk; not a language
artefact, which is recorded instead), how oxpinyin's
structure keeps that guarantee, and the time and space complexity of
both. When the argument needs more than a paragraph it moves to its own
finding and the row links to it.

## The distance tie-break

The pin carries a distance per parse step, summed along the path, and
uses it as the third key after parsed length and key count
(`src/storage/pinyin_parser2.cpp:296-310`). Correction rows in
`pinyin_index` carry distance 1, as do the `sua` and `zua` ambiguity
rows. Implemented in #832: each `Edge` carries its row's distance and
`SegmentGraph::fewest_keys` ranks a step by (key count, total distance);
the option-sweep differential gained the Requirement 4 inputs and a
pair sweep (record §2 and §11).

## Sentence scoring

The pin's sentence scores are `gfloat` sums of `log` over expressions
mixing `gfloat` and `gdouble` (record §11). The faithful path keeps
those types and conversion points as `f32` and `f64` in the same order
and resolves `log` as the pin's build does, verified for the identified
artifacts rather than assumed (record §11). Whether it replaces the
fixed-point scale (`oxpinyin-core/src/cost.rs`) on the parity path or
sits beside it is the implementing lane's design; either way the
default build scores as the pin does.

## Fitted coefficients

`ScoringConfig::default` (`oxpinyin-core/src/scoring.rs`) holds
sweep-chosen weights. The session reads them only on the branch taken
when the model has no real unigram data. No C-ABI configuration takes
it — the ABI's language model always reports real unigrams — so the
weights reach no parity-path output (record §11). These weights do not affect either C ABI under the established runtime
configuration, so their removal is not required to close a C-ABI
compatibility gap. Removal remains internal-work scope,
subject to checking in-tree Rust callers and test and tool consumers
(a Rust interface change needs no approval since 2026-10-18 UTC).

## Idiom review

§10 of the record lists the C-shaped code. Each entry says whether
behaviour depends on the shape. Entries that do not are rewritten as
idiomatic Rust when their code is next touched, or by their own task.
