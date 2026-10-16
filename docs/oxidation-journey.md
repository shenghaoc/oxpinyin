# The oxidation journey so far

Date: 2026-10-16 UTC · Pin: libpinyin `074a2219` (2.11.92) · Tree:
`origin/main` at `3b2bfdf`.

A look back at how libpinyin has been turned into Rust, what that cost,
and the method that follows from it. The guiding principle, stated in
hindsight: reproduce the existing system's observable behaviour before
improving its internal design — optimise for compatibility in the first
release, and for elegance, extensibility and performance afterwards.
The method itself is `.kiro/steering/oxidation-method.md`; the
structure-by-structure record is
`docs/findings/libpinyin-structures-and-algorithms.md`.

## Where it stands

Most of the decoder, the candidate list and the scheme parsers are now
close re-expressions of the pin, down to its quirks: a full trellis node
evicts its *best* value, the beam uses libstdc++'s heap routines because
the comparator is cyclic, and the final sort is GLib's merge sort with
the cost difference truncated to `gint`. The candidate surface is
bit-identical to the pin on the corpus, and the sentence surface
matches on 500 of 500 comparable inputs.

Storage reads the pin's own files with the same keys and layouts. The
user and write side is a different design — an in-memory transactional
store with ordered overlays that writes the pin's file formats at save —
and the full-pinyin parser is re-expressed as "build the whole lattice,
then select".

## How it got here

The project did not start from the pin's algorithms. It started from a
clean-room rule and designed its own, then measured them against the
oracle and moved towards the pin as the gaps showed up.

| When (UTC) | What happened | Record |
| --- | --- | --- |
| 2026-08-09 | A segmentation search of our own (k-best over a segment graph) was specified, because 468 corpus inputs had the pin choosing a segmentation our greedy order did not put first | `findings/kbest-search.md`, `findings/divergence-taxonomy.md` |
| 2026-08-10 | Scoring weights of our own (`segmentation_penalty`, `incomplete_penalty`, `phrase_key_bonus`) were swept against the oracle; top-1 against the pin stood at 63% | `findings/scoring-constant-sweep.md`, `findings/parity-climb-residual.md` |
| 2026-08-11 | A further constant, `UNIGRAM_TIEBREAK_SCALE`, was swept | `testing/f2-unigram-tiebreak-sweep.md` |
| 2026-08-14 | The pin's window scan was reproduced from a source-verified specification: top-1 went to 99% | `findings/candidate-construction.md` |
| 2026-08-22 | The candidate surface became bit-identical on all 10,190 corpus rows at depth 10 | `testing/corpus-tail.md` |
| 2026-08-28 | The compatibility policy: a divergence must fit a named class, everything else is reverted | `findings/compatibility-policy.md`, `findings/revert-plan.md` |
| 2026-09-01 | A faster on-disk format of our own was measured and rejected for breaking file compatibility; the clean-room rule was by then retired | `findings/perf-mmap-system-indexes-2026-08-31.md`, `findings/libpinyin-system-data-formats-2026-09-01.md` |
| 2026-09-02 | The compatibility layer was replaced by reading the pin's data in place | `findings/runtime-direct-libpinyin-data-2026-09-02.md` |
| 2026-09-04 | The sentence-surface residual was frozen as floating-point arithmetic (class (a)) at 491/396/390 | `findings/sentence-surface.md` |
| 2026-09-10 | The scan's probe count was traced to oxpinyin expanding every incomplete key into all its completions, where the pin probes the incomplete keyspace once; the fix adopted the pin's approach | `findings/scan-matrix-fanout-2026-09-10.md` |
| 2026-09-20 | The redb and LMDB backends were removed: nothing they did could be checked against the pin | `findings/backend-selection-audit.md` |
| 2026-09-27 | An audit found that two rules inside the "arithmetic" residual were selection logic | `findings/upstream-divergences.md` |
| 2026-10-04 | The pin's trellis selection was ported whole; the sentence gate went to 495/495/495, then 500/500/500 | `findings/sentence-surface.md` (lane D) |
| 2026-10-16 | A source-level survey of every subsystem found one more selection rule missing — the full-pinyin parser's distance tie-break — and fitted scoring weights still in the tree, off the parity path | `findings/libpinyin-structures-and-algorithms.md` §11 |

## What it taught

**The crate split was worth keeping.** The first design broke the work
into crates with explicit dependencies, and that survived every later
change of direction. libpinyin builds one monolithic library per API,
switches its backend with the preprocessor, and keeps its output logic
in the same file as its ABI glue; oxpinyin keeps the pin's layers but
gives each a crate, puts the backend behind a seam, and has both ABIs
share one facade (the record's §0). Behaviour comes from the pin;
structure need not.

**A library is not a frontend.** The first design built a
framework-neutral IME session — key input, preedit, commit — because
that is what a new Rust input method would want. libpinyin never sees a
key: its frontends own keys and drive it through parse, guess, select,
train and save. The C ABI ended up built on those operations alone, and
the key-driven layer is used only by tests and oracle tools, yet it
still sits on the same `Session` type. For the first release the
operations are the surface; a key-driven API is an optional adapter
afterwards.

**Reading the source beats searching for the answer.** Sweeping our own
constants and classifying residuals brought top-1 to 63–64%. Reading
how the pin builds its candidate list brought it to 99% four days
later. Every time the project measured its way towards the pin
instead of reading the pin, it went slower and left something behind:
the fitted `ScoringConfig` weights are still in the tree, now only on a
fallback path the C ABI never takes.

**A residual labelled "arithmetic" can hide logic — and the arithmetic
need not be a residual.** The sentence residual was frozen as
floating-point accumulation for over three weeks before an audit found
two selection rules inside it. What remained is `gfloat` and libm's
`log`: the pin's float types and conversion points can be written in
Rust exactly, and the same libm gives the same `log` on the same host.
The fixed-point scale that made it a divergence was our choice, made
for cross-platform determinism before the pin's behaviour was
reproduced. A record of the
pin's selection logic, made before the port, would have shown both at
once — and would have shown the parser's distance tie-break, which went
unnoticed until a structure-by-structure survey.

**Anything without a pin counterpart cannot be verified.** The custom
on-disk format, the redb and LMDB backends and the Python wrappers were
all built, measured and then removed, because there was nothing to check
them against. The work that stayed is the work that has an oracle.

**Compatibility first, design second.** Several of the detours above —
our own scoring, our own on-disk format, extra backends — were
improvements attempted before the behaviour they were meant to improve
had been reproduced. Each had to be measured against a moving target
and most were undone. Reproducing the pin first, and improving the
design once the behaviour is pinned down, would have made every one of
those steps checkable.

**Writing it down first would have saved most of this.** None of the
gaps above needed the oracle to find; each was visible in the pin's
source. The order of work is now record, then design and analyse, then
implement, with the oracle as the gate rather than the search tool.

## Rust is not C

libpinyin is C++ written in a C style, and much of its shape exists
because of C: byte buffers edited in place, GLib arrays and hash tables,
raw back-pointers between objects, integer status codes, `assert` on bad
input. Transliterating that shape into Rust tends to be both unidiomatic
and slower. Where oxpinyin chose Rust's shape, it gained:

- spelling lookups through compile-time `phf` maps, O(1) where the pin
  binary-searches an array;
- system files mapped read-only, with edits held beside them, so no
  copy-on-write page duplication;
- `Result` instead of `abort`, refused before any state changes.

Where it departed from the pin's algorithm without need, it lost: the
scan's probe fan-out above, closed by going back to the pin's approach,
and a fresh allocation for every pinyin-index record on every lookup
where the pin reuses one buffer.

And in a few places oxpinyin keeps the pin's C shape because the
behaviour depends on it: hand-written heap routines and GLib's merge
sort for the cyclic comparator, and `SingleGram` operations over raw
bytes because those bytes are the file format. The rule is now explicit:
idiomatic Rust by default, the pin's code shape only where behaviour
depends on it, and when there is a performance gap, the pin's algorithm
is the first thing tried — written as idiomatic Rust, and kept only if
it is better.

## What is still open

The open gaps are listed in the record's §11 and tracked as tasks in
`.kiro/specs/oxidation-record/`: the distance tie-break, removing the
fitted weights after the first release, and the equivalence arguments
the survey found plausible but did not write down.
