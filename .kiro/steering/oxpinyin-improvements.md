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
  unnecessary edges, not fewer crates, and no generic-runtime rewrite.
- Complexity: an internal change never worsens time and space both; a
  regression in one is traded against a gain in the other, minimized
  and justified.

## Stage 3 — performance (after Stage 2)

Smaller binary, faster execution and much lower RAM than libpinyin —
requirements, design direction and task list in
`.kiro/specs/stage3-performance/`. The default build is always the
bug-for-bug binary: a Stage 3 change that is not bug-for-bug — any
change to observable output or to the files on disk — sits behind an
opt-in cargo feature, off by default. No performance work happens
in Stage 2; ideas found there are reported as Stage 3 leads. Work that landed
early is recorded in `ROADMAP.md`.
