---
inclusion: always
---
# oxpinyin improvements — what is ours

Everything oxpinyin does that the pinned libpinyin does not. The target
behaviour is in `pinned-behaviour.md`; this file is the complete list of
where oxpinyin may differ from it and the rule that admits each
difference. Canonical policy text: `docs/findings/compatibility-policy.md`.

## Observable differences — three classes, nothing else

Anything observable outside these is a defect to revert, not an
improvement to record. A new entry is argued into a class; none is
admitted by analogy.

- **(a) MATH** — platform-dependent floating-point accumulation: a
  transcendental in the accumulation, not merely a float in the call
  graph. The sentence surface's remaining difference lives here — six
  rows, after lane D ported the trellis selection rules bug for bug
  (policy row 11, #535).
- **(b) MEMORY SAFETY** — the pin's behaviour is undefined and Rust
  structurally prevents reproducing it.
- **(c) AVAILABILITY** — the pin asserts or aborts on input a caller or a
  file can supply. oxpinyin returns `false`/`Err` **and** emits exactly
  one `g_warning`-level log line per occurrence (#525, #549). It covers
  aborts, never wrong-but-defined answers, and applies uniformly to every
  verified abort or segfault site; an unverified site is reproduced
  first. There is no fourth "tolerate and warn" class.
- **(d) CONSUMER SCOPE** — retired 2026-09-06.

**Registered standing divergences** sit outside the classes, one by
name each, with its ruling (the language-quirk exception — tkrzw's
`SYSTEM_ERROR`/`UNKNOWN_ERROR` collapse through its C API). Rust-mechanism
residue goes to `docs/findings/upstream-divergences.md` and is reported
upstream after the rewrite.

## Internal improvements — no observable change

- The constitution (`/AGENTS.md`): nothing panics on any input; public
  Rust APIs return `Result`; `unsafe` only on the allowlist, each block
  with `// SAFETY:`; output a pure function of (input, user state,
  config).
- Loose coupling: oxpinyin exists to undo libpinyin's tight coupling
  while keeping its behaviour. The 20 crates stay; the aim is fewer
  unnecessary edges, not fewer crates, and no unrelated workspace
  restructuring. Carrying a concrete type through the code that uses it
  is not a "generic-runtime rewrite": type erasure (`dyn`) stays only
  where a runtime heterogeneity, a standard or foreign interface, or a
  measured trade-off calls for it, never to keep a Rust signature.
- Rust interfaces are not a compatibility boundary (maintainer ruling
  2026-10-18 UTC; `/AGENTS.md`, "External interfaces"). Only the C
  APIs/ABIs are supported externally; Rust signatures change freely,
  with every in-tree caller updated in the same change.
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
