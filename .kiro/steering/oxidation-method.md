---
inclusion: always
---
# Oxidation method

Maintainer ruling 2026-10-16 UTC. `/AGENTS.md` wins on conflict, and
`pinned-behaviour.md` and `oxpinyin-improvements.md` (canonical text:
`docs/findings/compatibility-policy.md`) decide what may differ. This
file says how a piece of libpinyin is turned into Rust.

Amended 2026-10-18 UTC: binary size follows Constitution §2's recording
and disclosure rule, separately from runtime time/space comparisons.

## Guiding principle

Stage 2 delivers bug-for-bug compatibility with the pinned libpinyin,
under the existing compatibility exceptions. Internal refactoring,
idiomatic Rust rewrites, and performance or memory improvements are
permitted during Stage 2 when compatibility is preserved.

Compatibility is an acceptance requirement, not a requirement to defer
all internal improvements until after release. Performance has the
floor defined in `/AGENTS.md` and no ceiling. An improvement does not
close, excuse or reclassify an unrelated compatibility gap.

Intentional departures from the pin that are not covered by an
approved compatibility exception belong to Stage 3, behind an explicit
opt-in that is off by default. Fixing an oxpinyin defect so that its
output moves towards the pin's is Stage 2 work, not a departure.

## Bug for bug

The default build behaves as the pinned libpinyin does, bugs included,
measured against the pin-built oracle. Where a Rust language mechanism
makes a pin behaviour impossible to reproduce, record it in
`docs/findings/upstream-divergences.md` as `/AGENTS.md` says and stop
working on that area.

**Floating point is not, by itself, a Rust language limitation.**
Rust's `f32` and `f64` are the same IEEE formats as `gfloat` and
`gdouble`. Reproduce the pin's types, conversion points and operation
order. Verify the relevant compiler transformations (multiply-add
contraction, constant folding) and the resolved logarithm
implementation for the identified artifacts. Investigate any remaining
discrepancy; do not classify it automatically as a host difference.

## The source is the specification

oxpinyin and libpinyin are both GPL-3.0-or-later. This is not a reverse
engineering project: read the pin's source (and its assembly or binary
when the source does not settle a question) to learn the rule, then
implement the rule. The oracle differential gates the result. It is not
a search tool — do not converge on pin behaviour by running inputs and
adjusting until the outputs agree.

**Coefficients.** Every constant the pin uses — seeds, factors, ceilings,
thresholds, beam and store widths, epsilons, penalties, discounts — is
taken at the pin's value. Never sweep, tune or fit a coefficient to make
outputs match. Each one is recorded with its pin citation in
`docs/findings/libpinyin-structures-and-algorithms.md`.

## Order of work for a subsystem

1. **Record.** Write down the pin's data structures and algorithms in
   `docs/findings/libpinyin-structures-and-algorithms.md`: layout,
   algorithm, constants, tie orders and any quirk that reaches output,
   with time and space complexity, each cited `file:line` at `074a2219`.
2. **Design and analyse.** Choose oxpinyin's structure and algorithm.
   State why it produces the pin's behaviour — including tie orders,
   integer wraparound and quirks — and its time and space complexity
   against the pin's.
3. **Implement** to imitate the pin, and gate on the oracle differential.

Work that predates this ruling is brought into the record by
`.kiro/specs/oxidation-record/`.

## What may differ, and the bounds

- **Same behaviour, not same code.** The implementation need not be
  identical to the pin's; the externally observable behaviour must be.
- **Idiomatic Rust is strongly preferred.** libpinyin is C++ written in
  a C style: mutable byte buffers (`MemoryChunk`), GLib containers,
  raw back-pointers, integer status codes, `assert` on bad input. A
  line-for-line transliteration of that into Rust is usually neither
  idiomatic nor fast — it fights the borrow checker, adds `unsafe` or
  copies, and loses what the Rust compiler and standard library do
  well. Express the pin's *behaviour* in idiomatic Rust: owned or
  borrowed slices, enums and `Result`, iterators, the standard
  collections. Reproduce the pin's *code shape* only where the
  behaviour depends on it (a tie order, a wraparound, a quirk).
- **Never both worse.** For the same operation, oxpinyin's time and
  space complexity are never both worse than the pin's. A regression in
  one is accepted only when traded for a gain in the other, kept as
  small as possible, and justified in the change's report.
- **Performance gap: try the pin's algorithm, idiomatically.** Stage 2
  work under `/AGENTS.md` (its floor is the never-both-worse rule). When
  oxpinyin is measurably slower or larger than the pin for an
  operation, first try the pin's own structure or algorithm, written as
  idiomatic Rust, and measure it (one run, base and pin side by side,
  as `/AGENTS.md` requires, repeated within that run). Judge every
  candidate on time and peak memory, never on speed alone: the kept
  version is never worse than the pin in both runtime time and space.
  Record binary file sizes and note increases over base in the PR under
  /AGENTS.md Constitution §2, outside the runtime time/space trade rule.
  The pin's
  algorithm is the first candidate, not a mandate — if its Rust
  expression is no better on those measures, keep or find the idiomatic
  alternative that is, and prefer the idiomatic version on a tie.
