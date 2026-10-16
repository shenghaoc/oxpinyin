---
inclusion: always
---
# Oxidation method

Maintainer ruling 2026-10-16 UTC. `/AGENTS.md` wins on conflict, and
`compatibility-policy.md` decides what may count as a divergence. This
file says how a piece of libpinyin is turned into Rust.

## Guiding principle

Reproduce the existing system's observable behaviour before improving
its internal design. Optimise for compatibility in the first release,
and for elegance, extensibility and performance afterwards.

Everything below serves that order. Until the first release, a change
earns its place by bringing behaviour closer to the pin; internal
redesign that leaves behaviour unchanged waits, unless the code it
touches is being changed for compatibility anyway.

## Bug for bug

The default build behaves as the pinned libpinyin does, bugs included,
measured against the pin-built oracle. Where a Rust language mechanism
makes a pin behaviour impossible to reproduce, record it in
`docs/findings/upstream-divergences.md` as `/AGENTS.md` says and stop
working on that area.

**Floating point is not a language limit.** Rust's `f32` and `f64` are
the same IEEE formats as `gfloat` and `gdouble`. Reproduce the pin's
float types, conversion points and order of operations, and call the
platform libm where the pin does. A difference that remains is a host
whose libm differs from the pin's, not a reason to change arithmetic.

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
- **Performance gap: try the pin's algorithm, idiomatically.** This
  belongs to the work after the first release. When
  oxpinyin is measurably slower or larger than the pin for an
  operation, first try the pin's own structure or algorithm, written as
  idiomatic Rust, and measure it (one run, base and pin side by side,
  as `/AGENTS.md` requires). The pin's algorithm is the first candidate,
  not a mandate: if its Rust expression is no better, keep or find the
  idiomatic alternative that is. A transliteration that is slower than
  the idiomatic version is not adopted for fidelity's sake when the
  behaviour is the same.
