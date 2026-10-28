---
inclusion: always
---
# oxpinyin improvements — what is ours

Everything oxpinyin does that the pinned libpinyin does not. The target
behaviour is in `pinned-behaviour.md`; this file is the complete list of
where oxpinyin may differ from it and the rule that admits each
difference. Canonical policy text: `docs/findings/compatibility-policy.md`.

## Observable differences — three classes and one recorded exception

Anything observable outside these is a defect to revert, not an
improvement to record. The decision order is in
`docs/findings/compatibility-policy.md`: the C-facing contract first,
then classes (a)–(c), then the external-I/O pin-call rule or a recorded
internal language/runtime quirk, then a defect or Stage 3. A new entry
is argued into a class or admitted by its own discrepancy record; none
is admitted by analogy.

- **(a) MATH** — platform-dependent floating-point accumulation: a
  transcendental in the accumulation, not merely a float in the call
  graph. The sentence surface's remaining difference lives here — six
  rows, after lane D ported the trellis selection rules bug for bug
  (policy row 11, #535). The math rule is part of this class: oxpinyin
  uses Rust's standard maths at full speed; a last-bit difference from
  the pin that follows from using it is class (a), recorded and not
  chased.
- **(b) MEMORY SAFETY** — the pin's behaviour is undefined and Rust
  structurally prevents reproducing it.
- **(c) AVAILABILITY** — the pin asserts or aborts on input a caller or a
  file can supply. oxpinyin returns `false`/`Err` **and** emits exactly
  one `g_warning`-level log line per occurrence (#525, #549). It covers
  aborts, never wrong-but-defined answers, and applies uniformly to every
  verified abort or segfault site; an unverified site is reproduced
  first. There is no fourth "tolerate and warn" class.
- **(d) CONSUMER SCOPE** — retired 2026-09-06.

See `docs/findings/compatibility-policy.md`, "Native Rust implementation" (#1303).

The language/runtime-quirk exception does not change the C ABI,
ownership (including real `GArray` and `g_free`), the on-disk formats
or data ownership. It is outside the classes
and is admitted only by a discrepancy record from
`docs/findings/language-runtime-discrepancy-template.md`, registered by
name in `docs/findings/upstream-divergences.md` and carried by a
`POLICY-ACCEPTED` differential case. One record admits no other by
analogy. An internal language artefact is not reproduced, on valid or
malformed input, and needs no approval. External I/O uses the pin's call
unless source proves a Rust equivalent on every input. The same call
does not reproduce undefined reads or aborts: class (b)/(c) stands. A
product-level change or an unruled reversal of a prior ruling is a STOP.

**Registered standing divergences** are individual entries under that
exception (tkrzw's `SYSTEM_ERROR`/`UNKNOWN_ERROR` collapse through its C
API is the existing one). The register is
`docs/findings/upstream-divergences.md`; the notes are reported upstream
after the rewrite.

## Internal improvements — no observable change

- The constitution (`/AGENTS.md`): nothing panics on any input; public
  Rust APIs return `Result`; `unsafe` only on the allowlist, each block
  with `// SAFETY:`; output a pure function of (input, user state,
  config).
- Loose coupling: oxpinyin exists to undo libpinyin's tight coupling
  while keeping its behaviour. The `oxpinyin-input` boundary is
  an additional crate; the aim is fewer unnecessary edges, not fewer
  crates, and no unrelated workspace restructuring. Carrying a concrete type through the code that uses it
  is not a "generic-runtime rewrite": type erasure (`dyn`) stays only
  where a runtime heterogeneity, a standard or foreign interface, or a
  measured trade-off calls for it, never to keep a Rust signature.
- Rust interfaces are not a compatibility boundary (maintainer ruling
  2026-10-18 UTC; `/AGENTS.md`, "External interfaces"). Only the C
  APIs/ABIs are supported external library interfaces; the tools are
  drop-in pin-utility replacements. Rust signatures change freely, with
  every in-tree caller updated in the same change.
- Complexity: an internal change never worsens time and space both; a
  regression in one is traded against a gain in the other, minimized
  and justified.

## Performance (Stage 2)

Performance, memory and safety work that changes nothing observable is
internal, and Stage 2 allows it. oxpinyin may be faster or smaller than
the pin by any amount. The floor is the complexity rule above, measured
against the pin (axis L, #757). Landed work is recorded in `ROADMAP.md`.

## Stage 3 — opt-in departures (after Stage 2)

A departure changes observable output, the files on disk or the ABI on
purpose: a new language model, different candidate behaviour, an
incompatible data format. Each sits behind an opt-in cargo feature, off
by default. The default build is always the bug-for-bug binary. A build
that breaks the ABI or the data formats does not claim libpinyin's
binary identity. Candidates: `.kiro/specs/stage3-departures/`.
