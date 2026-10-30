# Design — Oxidation record

## Overview

Three human documents describe the work:

- [Source policy](../../source-policy.md): the rules and method.
- [Algorithm record](../../findings/libpinyin-structures-and-algorithms.md): pin and oxpinyin comparisons, row by row.
- [Oxidation journey](../../oxidation-journey.md): the dated narrative for its named tree.

Update the record whenever the tree changes.

## The record

Follow the pin's layers so readers can map a source file to a row:

| § | Content |
|---|---|
| 0 | Component layout and crate split |
| 1 | Storage |
| 2 | Parsing |
| 3 | Phonetic key matrix |
| 4 | Sentence decoding |
| 5 | Candidate list |
| 6 | Training and user phrases |
| 7 | Object model and persistence |
| 8 | Offline tooling |
| 9 | Coefficients |
| 10 | Retained pin code shapes |
| 11 | Open gaps |

Read the pin at `074a2219` and the oxpinyin source before adding or changing a row.
Read assembly or binary contents only when source cannot settle the question.
Do not derive rows by running inputs.
After derivation, use the surface's oracle differential as the gate.

## Arguments for non-identical rows

Move a structurally different row from "unverified" to "argued" with a short argument below its table.
State the pin's output-reaching guarantees, such as orders, tie rules and product quirks.
Record language artefacts instead of reproducing them.
Explain how oxpinyin preserves the guarantees.
Compare both implementations' time and space complexity.
Put arguments longer than a paragraph in a separate finding linked from the row.

## The distance tie-break

The pin sums distance over parse steps and uses it after parsed length and key count.
The citation is `src/storage/pinyin_parser2.cpp:296-310`.
Correction rows in `pinyin_index`, and `sua`/`zua` ambiguity rows, carry distance 1.
Implemented in #832: each Edge carries its row's distance.
`SegmentGraph::fewest_keys` ranks steps by key count and total distance.
The option-sweep differential gained Requirement 4's inputs and a pair sweep (record §2 and §11).

## Sentence scoring

The pin sums `log` with `gfloat` scores over expressions mixing `gfloat` and `gdouble` (record §11).
The specified path preserves those types and conversion points as `f32`/`f64` in the same order.
These scoring types, conversion points and ordering rules remain authoritative.

The earlier design said to resolve `log` as the pin build does.
The 2026-10-29 UTC maintainer ruling qualifies that:
oxpinyin uses Rust's standard maths (`f64::ln`), records any last-bit difference from the pin's `log` as class (a),
and does not imitate the arm64 compiler's contraction with `mul_add`.
See [float scoring §5](../../findings/float-scoring-1378.md#5-compatibility-policy-disposition--ruled-2026-10-29-utc).

Historical: the design proposed either replacing the fixed-point scale in `oxpinyin-core/src/cost.rs` or adding a parallel float path.
What was implemented is the replacement on the real-table sentence path:
the decoder accumulates `f32` scores (#1378 PR B), the pre-frequency fallback scorer keeps its own `Cost`,
and the remaining rational path is scheduled for removal (#1378 PR C).

## Fitted coefficients

`ScoringConfig::default` in `oxpinyin-core/src/scoring.rs` contains sweep-selected weights.
Sessions read them only when the model has no real unigram data.
No C-ABI configuration takes that branch: its language model always reports real unigrams.
The weights therefore reach no parity-path output (record §11).
Their removal does not close a C-ABI compatibility gap.
Removal remains internal work, subject to checking in-tree Rust callers and test/tool consumers.
Rust-signature changes need no interface approval since 2026-10-18 UTC.

## Idiom review

Record §10 lists C-shaped code and whether behaviour depends on the shape.
Rewrite independent shapes idiomatically when next touching their code, or through their own task.
